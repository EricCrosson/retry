use std::{collections::HashSet, process::ExitStatus};

use crate::task::TaskOutcome;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Decider {
    Default(DefaultDecider),
    RetryOnExitCodes(RetryOnExitCodesDecider),
}

impl Decider {
    pub(crate) fn new(retry_on_exit_codes: Option<Vec<i32>>) -> Self {
        match retry_on_exit_codes {
            Some(retry_on_exit_codes) => Decider::RetryOnExitCodes(RetryOnExitCodesDecider::new(
                retry_on_exit_codes.into_iter().collect(),
            )),
            None => Decider::Default(DefaultDecider::new()),
        }
    }
}

impl Default for Decider {
    fn default() -> Self {
        Decider::Default(DefaultDecider::default())
    }
}

impl Decidable for Decider {
    fn decide(&self, task_outcome: TaskOutcome) -> Decision {
        match self {
            Decider::Default(decider) => decider.decide(task_outcome),
            Decider::RetryOnExitCodes(decider) => decider.decide(task_outcome),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DefaultDecider;

impl DefaultDecider {
    pub(crate) fn new() -> Self {
        Self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RetryOnExitCodesDecider(HashSet<i32>);

impl RetryOnExitCodesDecider {
    fn new(retry_on_exit_codes: HashSet<i32>) -> Self {
        Self(retry_on_exit_codes.into())
    }
}

pub trait Decidable {
    fn decide(&self, task_outcome: TaskOutcome) -> Decision;
}

impl Decidable for DefaultDecider {
    fn decide(&self, task_outcome: TaskOutcome) -> Decision {
        match task_outcome {
            TaskOutcome::Success => Decision::Finished(FinishedReason::Success),
            TaskOutcome::Failure(exit_code) => {
                Decision::Unfinished(UnfinishedReason::Failure(exit_code))
            }
            TaskOutcome::Terminated(signal) => {
                Decision::Finished(FinishedReason::Terminated(signal))
            }
            TaskOutcome::TimeoutExceeded => Decision::Unfinished(UnfinishedReason::Timeout),
        }
    }
}

impl Decidable for RetryOnExitCodesDecider {
    fn decide(&self, task_outcome: TaskOutcome) -> Decision {
        match task_outcome {
            TaskOutcome::Success => Decision::Finished(FinishedReason::Success),
            TaskOutcome::Failure(exit_status) => {
                if self.0.contains(
                    &exit_status
                        .code()
                        .expect("TaskOutcome::Failure(ExitStatus) will never have a None ExitCode"),
                ) {
                    Decision::Unfinished(UnfinishedReason::Failure(exit_status))
                } else {
                    Decision::Finished(FinishedReason::Failure(exit_status))
                }
            }
            TaskOutcome::Terminated(signal) => {
                Decision::Finished(FinishedReason::Terminated(signal))
            }
            TaskOutcome::TimeoutExceeded => Decision::Unfinished(UnfinishedReason::Timeout),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Decision {
    Finished(FinishedReason),
    Unfinished(UnfinishedReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FinishedReason {
    Success,
    Failure(ExitStatus),
    Terminated(ExitStatus),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnfinishedReason {
    Failure(ExitStatus),
    Timeout,
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt;

    use crate::task::{Runnable, Task};

    use super::*;

    #[test]
    fn default_decider_success() {
        // Arrange
        let decider = Decider::new(None);

        // Act
        let conclusion = decider.decide(TaskOutcome::Success);

        // Assert
        assert_eq!(conclusion, Decision::Finished(FinishedReason::Success));
    }

    #[test]
    fn default_decider_failure() {
        // Arrange
        let exit_code = ExitStatus::from_raw(1);
        let task_outcome = TaskOutcome::Failure(exit_code.clone());
        let decider = Decider::new(None);

        // Act
        let conclusion = decider.decide(task_outcome);

        // Assert
        assert_eq!(
            conclusion,
            Decision::Unfinished(UnfinishedReason::Failure(exit_code))
        );
    }

    #[test]
    fn default_decider_termination() {
        // Arrange
        let exit_code = ExitStatus::from_raw(1);
        let task_outcome = TaskOutcome::Terminated(exit_code);
        let decider = Decider::new(None);

        // Act
        let conclusion = decider.decide(task_outcome);

        // Assert
        assert_eq!(
            conclusion,
            Decision::Finished(FinishedReason::Terminated(exit_code))
        );
    }

    #[test]
    fn default_decider_timeout() {
        // Arrange
        let task_outcome = TaskOutcome::TimeoutExceeded;
        let decider = Decider::new(None);

        // Act
        let conclusion = decider.decide(task_outcome);

        // Assert
        assert_eq!(conclusion, Decision::Unfinished(UnfinishedReason::Timeout));
    }

    #[test]
    fn retry_on_exit_codes_decider_success() {
        // Arrange
        // explicit empty set means it will never retry
        let decider = Decider::new(Some(Vec::new()));

        // Act
        let conclusion = decider.decide(TaskOutcome::Success);

        // Assert
        assert_eq!(conclusion, Decision::Finished(FinishedReason::Success));
    }

    #[tokio::test]
    async fn retry_on_exit_codes_decider_unfinished_failure() {
        // Arrange
        let task = Task::new(["sh", "-c", "exit 2"], None);
        let task_outcome = task.run().await.unwrap();
        let task_outcome_exit_status = match task_outcome {
            TaskOutcome::Failure(exit_status) => exit_status,
            _ => panic!("Expected TaskOutcome::Failure"),
        };
        let retry_on_exit_codes = Some(vec![1, 2, 3]);
        let decider = Decider::new(retry_on_exit_codes);

        // Act
        let conclusion = decider.decide(task_outcome);

        // Assert
        assert_eq!(
            conclusion,
            Decision::Unfinished(UnfinishedReason::Failure(task_outcome_exit_status))
        );
    }

    #[tokio::test]
    async fn retry_on_exit_codes_decider_finished_failure() {
        // Arrange
        let task = Task::new(["sh", "-c", "exit 4"], None);
        let task_outcome = task.run().await.unwrap();
        let task_outcome_exit_status = match task_outcome {
            TaskOutcome::Failure(exit_status) => exit_status,
            _ => panic!("Expected TaskOutcome::Failure"),
        };
        let retry_on_exit_codes = Some(vec![1, 2, 3]);
        let decider = Decider::new(retry_on_exit_codes);

        // Act
        let conclusion = decider.decide(task_outcome);

        // Assert
        assert_eq!(
            conclusion,
            Decision::Finished(FinishedReason::Failure(task_outcome_exit_status))
        );
    }

    #[test]
    fn retry_on_exit_codes_decider_termination() {
        // Arrange
        // explicit empty set means it will never retry
        let decider = Decider::new(Some(Vec::new()));
        let exit_code = ExitStatus::from_raw(1);
        let task_outcome = TaskOutcome::Terminated(exit_code);
        let conclusion = decider.decide(task_outcome);
        assert_eq!(
            conclusion,
            Decision::Finished(FinishedReason::Terminated(exit_code))
        );
    }

    #[test]
    fn retry_on_exit_codes_decider_timeout() {
        // Arrange
        let retry_on_exit_codes = vec![]; // explicit empty set means it will never retry
        let decider = RetryOnExitCodesDecider::new(retry_on_exit_codes.into_iter().collect());
        let task_outcome = TaskOutcome::TimeoutExceeded;

        // Act
        let conclusion = decider.decide(task_outcome);

        // Assert
        assert_eq!(conclusion, Decision::Unfinished(UnfinishedReason::Timeout));
    }
}
