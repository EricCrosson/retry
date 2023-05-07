#![forbid(unsafe_code)]
#![deny(warnings, missing_docs)]

//! TODO: document me

use std::{process::ExitStatus, time::Duration};

use clap::Parser;
use cli::Cli;
use tokio::process::Command;

mod cli;

use crate::cli::Retry;

// Notable: https://docs.rs/retry/latest/retry/

type Error = Box<dyn std::error::Error + Send + Sync + 'static>;
type Result<T> = std::result::Result<T, Error>;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum TaskOutcome {
    Success,
    Timeout,
}

async fn eval(command: &[String]) -> Result<ExitStatus> {
    // FIXME: kill in a way that guarantees reaping the child:
    // https://docs.rs/tokio/latest/tokio/process/struct.Command.html#caveats
    let mut child = Command::new(command[0].clone());
    for argument in command.iter().skip(1) {
        child.arg(argument);
    }
    let mut child = child.kill_on_drop(true).spawn()?;

    Ok(child.wait().await?)
}

async fn run_task(command: &[String], task_timeout: Option<Duration>) -> Result<TaskOutcome> {
    let status_code = match task_timeout {
        None => eval(command).await?.code(),
        Some(task_timeout) => {
            let task = tokio::time::timeout(task_timeout, eval(command)).await;

            match task {
                // The command completed
                Ok(result_exit_status) => match result_exit_status {
                    Ok(exit_status) => exit_status.code(),
                    Err(_eval_err) => None,
                },
                // The command timed out
                Err(_timeout_err) => None,
            }
        }
    };

    if let Some(status_code) = status_code {
        if status_code == 0 {
            return Ok(TaskOutcome::Success);
        }
    }

    Ok(TaskOutcome::Timeout)
}

async fn loop_task(command: &[String], task_timeout: Option<Duration>) -> Result<TaskOutcome> {
    loop {
        let status_code = run_task(command, task_timeout).await?;
        if status_code == TaskOutcome::Success {
            return Ok(TaskOutcome::Success);
        }
    }
}

async fn run_tasks(
    command: Vec<String>,
    up_to: Retry,
    task_timeout: Option<Duration>,
) -> Result<()> {
    match up_to {
        Retry::ForDuration(duration) => {
            let task_outcome =
                tokio::time::timeout(duration, loop_task(&command, task_timeout)).await;
            if let Ok(Ok(TaskOutcome::Success)) = task_outcome {
                return Ok(());
            }
        }
        Retry::NumberOfTimes(num_times) => {
            for _ in 0..num_times {
                let task_outcome = run_task(&command, task_timeout).await?;
                if task_outcome == TaskOutcome::Success {
                    return Ok(());
                }
            }
        }
    };

    Err("Command did not succeed within designated bounds".into())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Cli = Cli::parse();
    run_tasks(args.command, args.up_to, args.task_timeout).await
}
