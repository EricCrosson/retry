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
    async fn execute(&mut self) -> std::io::Result<ExecutionOutcome>;
    async fn run_indefinitely(&mut self) -> std::io::Result<FinishedReason>;
}

impl<T, D> Executable for Executor<T, D>
where
    T: Runnable,
    D: Decidable,
{
    async fn execute(&mut self) -> std::io::Result<ExecutionOutcome> {
        let mut final_unfinished_reason = UnfinishedReason::Timeout;
        Ok(match self.limit {
            Limit::NumberOfTimes(num_times) => {
                for _ in 0..num_times {
                    self.tick().await;
                    let task_outcome = self.task.run().await?;
                    let decision = self.decider.decide(task_outcome);
                    match decision {
                        Decision::Finished(finished_reason) => return Ok(finished_reason.into()),
                        Decision::Unfinished(unfinished_reason) => {
                            final_unfinished_reason = unfinished_reason;
                            continue;
                        }
                    }
                }
                // retry only up_to num_times
                ExecutionOutcome::Exhausted(ExhaustionReason::RetryTimesExceeded(
                    final_unfinished_reason,
                ))
            }
            Limit::ForDuration(duration) => {
                let task_result_or_timeout =
                    tokio::time::timeout(duration, self.run_indefinitely()).await;
                match task_result_or_timeout {
                    Ok(finished_reason) => finished_reason?.into(),
                    // retry only up_to duration exceeded
                    Err(_timeout) => ExecutionOutcome::Exhausted(
                        ExhaustionReason::RetryTimeoutExceeded(final_unfinished_reason),
                    ),
                }
            }
        })
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

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum ExecutionOutcome {
    Success,                     // The command succeeded
    Failure(ExitStatus), // The command failed and was not retried because the exit code was not in the retry_on_exit_codes set
    Terminated(ExitStatus), // The command was terminated by a signal
    Exhausted(ExhaustionReason), // The command was retried until the up_to limit
}

impl From<FinishedReason> for ExecutionOutcome {
    fn from(finished_reason: FinishedReason) -> Self {
        match finished_reason {
            FinishedReason::Success => ExecutionOutcome::Success,
            FinishedReason::Terminated(exit_status) => ExecutionOutcome::Terminated(exit_status),
            FinishedReason::Failure(exit_status) => ExecutionOutcome::Failure(exit_status),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum ExhaustionReason {
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
        let outcome = executor.execute().await.unwrap();

        // Assert
        assert_eq!(outcome, ExecutionOutcome::Success);
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
        let outcome = executor.execute().await.unwrap();

        // Assert
        assert_eq!(outcome, ExecutionOutcome::Failure(failure_exit_status));
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
        let outcome = executor.execute().await.unwrap();

        // Assert
        assert_eq!(outcome, ExecutionOutcome::Terminated(failure_exit_status));
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
        let outcome = executor.execute().await.unwrap();

        // Assert
        assert_eq!(
            outcome,
            ExecutionOutcome::Exhausted(ExhaustionReason::RetryTimesExceeded(
                UnfinishedReason::Failure(failure_exit_status)
            ))
        );
    }

    #[tokio::test]
    async fn execute_retry_times_unfinished_timeout() {
        // Arrange
        let decider = TestDecider(Decision::Unfinished(UnfinishedReason::Timeout));
        let limit = Limit::NumberOfTimes(3);
        let mut executor = Executor::new(DummyTask, decider, limit, None);

        // Act
        let outcome = executor.execute().await.unwrap();

        // Assert
        assert_eq!(
            outcome,
            ExecutionOutcome::Exhausted(ExhaustionReason::RetryTimesExceeded(
                UnfinishedReason::Timeout
            ))
        );
    }

    #[tokio::test]
    async fn execute_retry_timeout_finished_success() {
        // Arrange
        let decider = TestDecider(Decision::Finished(FinishedReason::Success));
        let limit = Limit::ForDuration(Duration::from_millis(10));
        let mut executor = Executor::new(DummyTask, decider, limit, None);

        // Act
        let outcome = executor.execute().await.unwrap();

        // Assert
        assert_eq!(outcome, ExecutionOutcome::Success);
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
        let outcome = executor.execute().await.unwrap();

        // Assert
        assert_eq!(outcome, ExecutionOutcome::Failure(failure_exit_status));
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
        let outcome = executor.execute().await.unwrap();

        // Assert
        assert_eq!(outcome, ExecutionOutcome::Terminated(failure_exit_status));
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
        let outcome = executor.execute().await.unwrap();

        // Assert
        assert_eq!(
            outcome,
            ExecutionOutcome::Exhausted(ExhaustionReason::RetryTimeoutExceeded(
                UnfinishedReason::Timeout
            ))
        );
        // Because the task races with a duration, there is no way to return an
        // ExitStatus and so no way that it can ever be an UnfinishedReason::Failure
        assert_ne!(
            outcome,
            ExecutionOutcome::Exhausted(ExhaustionReason::RetryTimeoutExceeded(
                UnfinishedReason::Failure(failure_exit_status)
            ))
        )
    }

    #[tokio::test]
    async fn execute_retry_timeout_unfinished_timeout() {
        // Arrange
        let decider = TestDecider(Decision::Unfinished(UnfinishedReason::Timeout));
        let limit = Limit::ForDuration(tokio::time::Duration::from_millis(5));
        let mut executor = Executor::new(DummyTask, decider, limit, None);

        // Act
        let outcome = executor.execute().await.unwrap();

        // Assert
        assert_eq!(
            outcome,
            ExecutionOutcome::Exhausted(ExhaustionReason::RetryTimeoutExceeded(
                UnfinishedReason::Timeout
            ))
        );
    }

    // This test may become flaky if system load is high;
    // feel free to delete or refactor this test if it becomes a problem
    #[tokio::test]
    async fn execute_retry_every() {
        // Arrange
        let number_of_times = 3;
        let execution_interval = Duration::from_millis(10);
        let min_expected_duration = execution_interval * (number_of_times - 1);
        let max_expected_duration = execution_interval * number_of_times;
        let limit = Limit::NumberOfTimes(number_of_times.into());

        let decider = TestDecider(Decision::Unfinished(UnfinishedReason::Timeout));
        let mut executor = Executor::new(DummyTask, decider, limit, Some(execution_interval));

        // Act
        let start = tokio::time::Instant::now();
        executor.execute().await.unwrap();
        let elapsed = start.elapsed();

        // Assert
        assert!(min_expected_duration < elapsed);
        assert!(elapsed < max_expected_duration);
    }
}
