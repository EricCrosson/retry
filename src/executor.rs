use std::process::ExitStatus;

use tokio::time::{Duration, Interval};

use crate::cli::Retry;
use crate::decider::{Decidable, Decision, FinishedReason, UnfinishedReason};
use crate::task::Runnable;

#[derive(Debug)]
pub(crate) struct Executor<T, D>
where
    T: Runnable,
    D: Decidable,
{
    task: T,
    decider: D,
    limit: Limit,
    execution_interval: Option<Interval>,
}

impl<T, D> Executor<T, D>
where
    T: Runnable,
    D: Decidable,
{
    pub(crate) fn new(
        task: T,
        decider: D,
        limit: Limit,
        execution_interval: Option<Duration>,
    ) -> Self {
        let execution_interval = execution_interval.map(tokio::time::interval);
        Self {
            task,
            decider,
            limit,
            execution_interval,
        }
    }

    async fn tick(&mut self) {
        if let Some(interval) = &mut self.execution_interval {
            interval.tick().await;
        }
    }
}

pub(crate) trait Executable {
    async fn execute(&mut self) -> Result<(), ExecuteError>;
    async fn run_indefinitely(&mut self) -> std::io::Result<FinishedReason>;
}

impl<T, D> Executable for Executor<T, D>
where
    T: Runnable,
    D: Decidable,
{
    async fn execute(&mut self) -> Result<(), ExecuteError> {
        let mut final_unfinished_reason = UnfinishedReason::Timeout;

        let create_err = |kind: ExecuteErrorKind| ExecuteError { kind };

        match self.limit {
            Limit::NumberOfTimes(num_times) => {
                for _ in 0..num_times {
                    self.tick().await;
                    let task_outcome = self
                        .task
                        .run()
                        .await
                        .map_err(|err| create_err(ExecuteErrorKind::Spawn(err)))?;
                    let decision = self.decider.decide(task_outcome);
                    match decision {
                        Decision::Finished(finished_reason) => return finished_reason.into(),
                        Decision::Unfinished(unfinished_reason) => {
                            final_unfinished_reason = unfinished_reason;
                            continue;
                        }
                    }
                }
                // retry only up_to num_times
                Err(create_err(ExecuteErrorKind::Exhausted(
                    ExhaustionReason::RetryTimesExceeded(final_unfinished_reason),
                )))
            }
            Limit::ForDuration(duration) => {
                let task_result_or_timeout =
                    tokio::time::timeout(duration, self.run_indefinitely()).await;
                match task_result_or_timeout {
                    Ok(finished_reason) => finished_reason
                        .map_err(|err| create_err(ExecuteErrorKind::Spawn(err)))?
                        .into(),
                    // retry only up_to duration exceeded
                    Err(_timeout) => Err(create_err(ExecuteErrorKind::Exhausted(
                        ExhaustionReason::RetryTimeoutExceeded(final_unfinished_reason),
                    ))),
                }
            }
        }
    }

    async fn run_indefinitely(&mut self) -> std::io::Result<FinishedReason> {
        loop {
            self.tick().await;
            let task_outcome = self.task.run().await?;
            let decision = self.decider.decide(task_outcome);
            match decision {
                Decision::Finished(finished_reason) => return Ok(finished_reason),
                Decision::Unfinished(_) => {
                    tokio::task::yield_now().await;
                    continue;
                }
            }
        }
    }
}

impl From<FinishedReason> for Result<(), ExecuteError> {
    fn from(finished_reason: FinishedReason) -> Self {
        match finished_reason {
            FinishedReason::Success => Ok(()),
            FinishedReason::Terminated(exit_status) => Err(ExecuteError {
                kind: ExecuteErrorKind::Terminated(exit_status),
            }),
            FinishedReason::Failure(exit_status) => Err(ExecuteError {
                kind: ExecuteErrorKind::Failure(exit_status),
            }),
        }
    }
}

// FIXME: implementation bleed: this should not be public outside the crate
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ExhaustionReason {
    RetryTimesExceeded(UnfinishedReason),
    RetryTimeoutExceeded(UnfinishedReason),
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub(crate) enum Limit {
    ForDuration(Duration),
    NumberOfTimes(u64),
}

impl From<Retry> for Limit {
    fn from(retry: Retry) -> Self {
        match retry {
            Retry::ForDuration(duration) => Limit::ForDuration(duration),
            Retry::NumberOfTimes(num_times) => Limit::NumberOfTimes(num_times),
        }
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub struct ExecuteError {
    pub kind: ExecuteErrorKind,
}

impl std::fmt::Display for ExecuteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            ExecuteErrorKind::Spawn(_) => write!(f, "unable to spawn child process"),
            ExecuteErrorKind::Failure(_) => {
                write!(f, "command failed with an exit code that is not retryable")
            }
            ExecuteErrorKind::Terminated(_) => write!(f, "command terminated by a signal"),
            ExecuteErrorKind::Exhausted(_) => {
                write!(f, "command did not succeed within specified constraints")
            }
        }
    }
}

impl std::error::Error for ExecuteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            ExecuteErrorKind::Spawn(err) => Some(err),
            ExecuteErrorKind::Failure(_) => None,
            ExecuteErrorKind::Terminated(_) => None,
            ExecuteErrorKind::Exhausted(_) => None,
        }
    }
}

#[derive(Debug)]
pub enum ExecuteErrorKind {
    #[non_exhaustive]
    Spawn(std::io::Error),
    /// The command failed and was not retried because the exit code was not in the retry_on_exit_codes set.
    #[non_exhaustive]
    Failure(ExitStatus),
    /// The command was terminated by a signal.
    #[non_exhaustive]
    Terminated(ExitStatus),
    /// The command was retried until the up_to limit, and did not succeed.
    #[non_exhaustive]
    Exhausted(ExhaustionReason),
}

impl ExecuteErrorKind {
    pub fn exit_code(&self) -> i32 {
        match self {
            ExecuteErrorKind::Spawn(_) => 1,
            // SMELL: code() returns None when command was terminated by a
            // signal, but we're handling that in the Terminated variant, so it
            // seems that error case shouldn't be representable here?
            ExecuteErrorKind::Failure(exit_status) => exit_status.code().unwrap_or(1),
            ExecuteErrorKind::Terminated(exit_status) => exit_status.code().unwrap_or(1),
            ExecuteErrorKind::Exhausted(exhaustion_reason) => match exhaustion_reason {
                ExhaustionReason::RetryTimesExceeded(unfinished_reason)
                | ExhaustionReason::RetryTimeoutExceeded(unfinished_reason) => {
                    match unfinished_reason {
                        UnfinishedReason::Failure(exit_code) => exit_code.code().unwrap_or(1),
                        UnfinishedReason::Timeout => 1,
                    }
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt;
    use tokio::time::Duration;

    use super::*;
    use crate::task::TaskOutcome;

    struct DummyTask;

    impl Runnable for DummyTask {
        async fn eval(&self) -> std::io::Result<ExitStatus> {
            Ok(ExitStatus::from_raw(0))
        }
        async fn run(&self) -> std::io::Result<TaskOutcome> {
            Ok(TaskOutcome::Success)
        }
    }

    struct TestDecider(Decision);

    impl TestDecider {
        fn new(conclusion: Decision) -> Self {
            Self(conclusion)
        }
    }

    impl Decidable for TestDecider {
        fn decide(&self, _task_outcome: TaskOutcome) -> Decision {
            self.0
        }
    }

    #[tokio::test]
    async fn execute_retry_times_finished_success() {
        // Arrange
        let decider = TestDecider(Decision::Finished(FinishedReason::Success));
        let limit = Limit::NumberOfTimes(3);
        let mut executor = Executor::new(DummyTask, decider, limit, None);

        // Act
        let outcome = executor.execute().await;

        // Assert
        assert!(outcome.is_ok());
    }

    #[tokio::test]
    async fn execute_retry_times_finished_failure() {
        // Arrange
        let failure_exit_status = ExitStatus::from_raw(1);
        let decider = TestDecider(Decision::Finished(FinishedReason::Failure(
            failure_exit_status.clone(),
        )));
        let limit = Limit::NumberOfTimes(3);
        let mut executor = Executor::new(DummyTask, decider, limit, None);

        // Act
        let outcome = executor.execute().await;

        // Assert
        assert!(matches!(
            outcome,
            Err(ExecuteError {
                kind: ExecuteErrorKind::Failure(_)
            })
        ));
    }

    #[tokio::test]
    async fn execute_retry_times_finished_terminated() {
        // Arrange
        let failure_exit_status = ExitStatus::from_raw(1);
        let decider = TestDecider(Decision::Finished(FinishedReason::Terminated(
            failure_exit_status.clone(),
        )));
        let limit = Limit::NumberOfTimes(3);
        let mut executor = Executor::new(DummyTask, decider, limit, None);

        // Act
        let outcome = executor.execute().await;

        // Assert
        assert!(matches!(
            outcome,
            Err(ExecuteError {
                kind: ExecuteErrorKind::Terminated(_)
            })
        ));
    }

    #[tokio::test]
    async fn execute_retry_times_unfinished_failure() {
        // Arrange
        let failure_exit_status = ExitStatus::from_raw(1);
        let decider = TestDecider(Decision::Unfinished(UnfinishedReason::Failure(
            failure_exit_status.clone(),
        )));
        let limit = Limit::NumberOfTimes(3);
        let mut executor = Executor::new(DummyTask, decider, limit, None);

        // Act
        let outcome = executor.execute().await;

        // Assert
        assert!(matches!(
            outcome,
            Err(ExecuteError {
                kind: ExecuteErrorKind::Exhausted(ExhaustionReason::RetryTimesExceeded(
                    UnfinishedReason::Failure(_)
                ))
            })
        ));
    }

    #[tokio::test]
    async fn execute_retry_times_unfinished_timeout() {
        // Arrange
        let decider = TestDecider(Decision::Unfinished(UnfinishedReason::Timeout));
        let limit = Limit::NumberOfTimes(3);
        let mut executor = Executor::new(DummyTask, decider, limit, None);

        // Act
        let outcome = executor.execute().await;

        // Assert
        assert!(matches!(
            outcome,
            Err(ExecuteError {
                kind: ExecuteErrorKind::Exhausted(ExhaustionReason::RetryTimesExceeded(
                    UnfinishedReason::Timeout
                ))
            })
        ));
    }

    #[tokio::test]
    async fn execute_retry_timeout_finished_success() {
        // Arrange
        let decider = TestDecider(Decision::Finished(FinishedReason::Success));
        let limit = Limit::ForDuration(Duration::from_millis(10));
        let mut executor = Executor::new(DummyTask, decider, limit, None);

        // Act
        let outcome = executor.execute().await;

        // Assert
        assert!(outcome.is_ok());
    }

    #[tokio::test]
    async fn execute_retry_timeout_finished_failure() {
        // Arrange
        let failure_exit_status = ExitStatus::from_raw(1);
        let decider = TestDecider(Decision::Finished(FinishedReason::Failure(
            failure_exit_status.clone(),
        )));
        let limit = Limit::ForDuration(Duration::from_millis(10));
        let mut executor = Executor::new(DummyTask, decider, limit, None);

        // Act
        let outcome = executor.execute().await;

        // Assert
        assert!(matches!(
            outcome,
            Err(ExecuteError {
                kind: ExecuteErrorKind::Failure(_)
            })
        ));
    }

    #[tokio::test]
    async fn execute_retry_timeout_finished_terminated() {
        // Arrange
        let failure_exit_status = ExitStatus::from_raw(1);
        let decider = TestDecider(Decision::Finished(FinishedReason::Terminated(
            failure_exit_status.clone(),
        )));
        let limit = Limit::ForDuration(Duration::from_millis(10));
        let mut executor = Executor::new(DummyTask, decider, limit, None);

        // Act
        let outcome = executor.execute().await;

        // Assert
        assert!(matches!(
            outcome,
            Err(ExecuteError {
                kind: ExecuteErrorKind::Terminated(_)
            })
        ));
    }

    #[tokio::test]
    async fn execute_retry_timeout_unfinished_failure() {
        // Arrange
        let failure_exit_status = ExitStatus::from_raw(1);

        let decider = TestDecider::new(Decision::Unfinished(UnfinishedReason::Failure(
            failure_exit_status.clone(),
        )));
        let limit = Limit::ForDuration(Duration::from_millis(5));
        let mut executor: Executor<DummyTask, TestDecider> =
            Executor::new(DummyTask, decider, limit, None);

        // Act
        let outcome = executor.execute().await;

        // Assert
        assert!(matches!(
            outcome,
            Err(ExecuteError {
                kind: ExecuteErrorKind::Exhausted(ExhaustionReason::RetryTimeoutExceeded(
                    UnfinishedReason::Timeout
                ))
            })
        ));
        // Because the task races with a duration, there is no way to return an
        // ExitStatus and so no way that it can ever be an UnfinishedReason::Failure
        assert!(!matches!(
            outcome,
            Err(ExecuteError {
                kind: ExecuteErrorKind::Exhausted(ExhaustionReason::RetryTimeoutExceeded(
                    UnfinishedReason::Failure(_)
                ))
            })
        ))
    }

    #[tokio::test]
    async fn execute_retry_timeout_unfinished_timeout() {
        // Arrange
        let decider = TestDecider(Decision::Unfinished(UnfinishedReason::Timeout));
        let limit = Limit::ForDuration(tokio::time::Duration::from_millis(5));
        let mut executor = Executor::new(DummyTask, decider, limit, None);

        // Act
        let outcome = executor.execute().await;

        // Assert
        assert!(matches!(
            outcome,
            Err(ExecuteError {
                kind: ExecuteErrorKind::Exhausted(ExhaustionReason::RetryTimeoutExceeded(
                    UnfinishedReason::Timeout
                ))
            })
        ))
    }
}
