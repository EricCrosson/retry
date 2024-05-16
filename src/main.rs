#![forbid(unsafe_code)]
#![deny(warnings, missing_docs)]
#![feature(exit_status_error)]

//! TODO: document me

use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;

use clap::Parser;

mod cli;
mod decider;
mod executor;
mod task;

use crate::cli::Cli;
use crate::decider::{Decider, UnfinishedReason};
use crate::executor::{Executable, ExecutionOutcome, Executor, ExhaustionReason};
use crate::task::Task;

// Notable: https://docs.rs/retry/latest/retry/

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Cli::parse();

    let task = Task::new(args.command, args.task_timeout);
    let decider = Decider::new(args.on_exit_code);
    let mut executor = Executor::new(task, decider, args.up_to.into(), args.every);
    let retry_outcome = executor.execute().await?;
    Ok(match retry_outcome {
        ExecutionOutcome::Success => Ok(()),
        ExecutionOutcome::Failure(exit_status) => exit_status.exit_ok().map_err(Box::new),
        ExecutionOutcome::Terminated(exit_status) => exit_status.exit_ok().map_err(Box::new),
        ExecutionOutcome::Exhausted(exhaustion_reason) => match exhaustion_reason {
            ExhaustionReason::RetryTimesExceeded(unfinished_reason)
            | ExhaustionReason::RetryTimeoutExceeded(unfinished_reason) => {
                match unfinished_reason {
                    UnfinishedReason::Failure(exit_code) => exit_code.exit_ok().map_err(Box::new),
                    UnfinishedReason::Timeout => {
                        ExitStatus::from_raw(1).exit_ok().map_err(Box::new)
                    }
                }
            }
        },
    }?)
}
