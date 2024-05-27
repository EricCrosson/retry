#![forbid(unsafe_code)]
#![deny(warnings, missing_docs)]

//! TODO: document me

use clap::Parser;

mod cli;
mod decider;
mod executor;
mod little_anyhow;
mod task;

use crate::cli::Cli;
use crate::decider::Decider;
use crate::executor::{Executable, Executor};
use crate::task::Task;

// Notable: https://docs.rs/retry/latest/retry/

#[tokio::main]
async fn main() -> Result<(), little_anyhow::Error> {
    let args = Cli::parse();

    let task = Task::new(args.command, args.task_timeout);
    let decider = Decider::new(args.on_exit_code);
    let mut executor = Executor::new(task, decider, args.up_to.into(), args.every);
    Ok(executor.execute().await?)
}
