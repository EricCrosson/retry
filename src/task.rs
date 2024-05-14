use std::process::ExitStatus;
use tokio::{process::Command, time::Duration};

pub(crate) trait Runnable {
    async fn eval(&self) -> std::io::Result<ExitStatus>;
    async fn run(&self) -> std::io::Result<TaskOutcome>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Task {
    command: Vec<String>,
    task_timeout: Option<Duration>,
}

impl Task {
    pub(crate) fn new(command: Vec<String>, task_timeout: Option<Duration>) -> Self {
        Self {
            command,
            task_timeout,
        }
    }
}

impl Runnable for Task {
    async fn eval(&self) -> std::io::Result<ExitStatus> {
        // FIXME: kill in a way that guarantees reaping the child:
        // https://docs.rs/tokio/latest/tokio/process/struct.Command.html#caveats
        let mut child = Command::new(&self.command[0]);
        for argument in self.command.iter().skip(1) {
            child.arg(argument);
        }
        let mut child = child.kill_on_drop(true).spawn()?;

        Ok(child.wait().await?)
    }

    async fn run(&self) -> std::io::Result<TaskOutcome> {
        Ok(match self.task_timeout {
            Some(task_timeout) => {
                match tokio::time::timeout(task_timeout, self.eval()).await {
                    // task completed
                    Ok(task_result) => Some(task_result?),
                    // task timed out
                    Err(_timeout) => None,
                }
            }
            None => Some(self.eval().await?),
        }
        .into())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskOutcome {
    Success,
    Failure(ExitStatus),
    Terminated(ExitStatus),
    TimeoutExceeded,
}

impl From<Option<ExitStatus>> for TaskOutcome {
    fn from(status: Option<ExitStatus>) -> Self {
        match status {
            Some(status) => match status.code() {
                Some(code) => match code {
                    // exited with code 0
                    0 => TaskOutcome::Success,
                    // exited with code
                    _ => TaskOutcome::Failure(status),
                },
                // terminated by signal
                None => TaskOutcome::Terminated(status),
            },
            // did not terminate
            None => TaskOutcome::TimeoutExceeded,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn task_success() {
        let command = vec!["sh".to_string(), "-c".to_string(), "exit 0".to_string()];
        let task = Task::new(command, None);
        let outcome = task.run().await.unwrap();
        assert_eq!(outcome, TaskOutcome::Success);
    }

    #[tokio::test]
    async fn task_failure() {
        let command = vec!["sh".to_string(), "-c".to_string(), "exit 144".to_string()];
        let task = Task::new(command, None);
        let outcome = task.run().await;
        let outcome_exit_status_code = match outcome {
            Ok(outcome) => match outcome {
                TaskOutcome::Failure(exit_status) => match exit_status.code() {
                    Some(code) => code,
                    None => panic!("Expected ExitStatus::code() to return Some(_)"),
                },
                _ => panic!("Expected TaskOutcome::Failure"),
            },
            _ => panic!("Expected Ok(_)"),
        };
        assert_eq!(outcome_exit_status_code, 144);
    }

    #[tokio::test]
    async fn task_timeout_exceeded() {
        let command = vec!["sleep".to_string(), "0.05".to_string()];
        let task = Task::new(command, Some(Duration::from_millis(1)));
        let outcome = task.run().await.unwrap();
        assert_eq!(outcome, TaskOutcome::TimeoutExceeded);
    }
}
