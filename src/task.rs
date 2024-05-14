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
    pub(crate) fn new<C>(command: C, task_timeout: Option<Duration>) -> Self
    where
        C: IntoIterator,
        C::Item: Into<String>,
    {
        Self {
            command: command.into_iter().map(Into::into).collect(),
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

impl TaskOutcome {
    #[cfg(test)]
    fn as_failure(self: &TaskOutcome) -> Option<ExitStatus> {
        match self {
            TaskOutcome::Failure(exit_status) => Some(exit_status.to_owned()),
            _ => None,
        }
    }
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
        let command = ["sh", "-c", "exit 0"];
        let task = Task::new(command, None);
        let outcome = task.run().await.unwrap();
        assert_eq!(outcome, TaskOutcome::Success);
    }

    #[tokio::test]
    async fn task_failure() {
        let command = ["sh", "-c", "exit 144"];
        let task = Task::new(command, None);
        let outcome = task.run().await;
        let outcome = outcome.expect("task outcome should be ok");
        let exit_status = outcome.as_failure().expect("task should have failed");
        let exit_code = exit_status.code().expect("exit code should be defined");
        assert_eq!(exit_code, 144);
    }

    #[tokio::test]
    async fn task_timeout_exceeded() {
        let command = ["sleep", "0.05"];
        let task = Task::new(command, Some(Duration::from_millis(1)));
        let outcome = task.run().await.unwrap();
        assert_eq!(outcome, TaskOutcome::TimeoutExceeded);
    }
}
