use std::{str::FromStr, time::Duration};

use clap::Parser;
use duration_string::DurationString;

fn duration_from_str(
    s: &str,
) -> Result<Duration, Box<dyn std::error::Error + Send + Sync + 'static>> {
    match DurationString::from_str(s) {
        Ok(duration) => Ok(duration.into()),
        Err(err) => Err(err.into()),
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Retry {
    ForDuration(Duration),
    NumberOfTimes(u64),
}

fn remove_last_character(value: &str) -> &str {
    let mut chars = value.chars();
    chars.next_back();
    chars.as_str()
}

impl FromStr for Retry {
    type Err = Box<dyn std::error::Error + Send + Sync + 'static>;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.ends_with('x') {
            if let Ok(retries) = str::parse(remove_last_character(s)) {
                return Ok(Self::NumberOfTimes(retries));
            }
        }
        if let Ok(duration) = DurationString::from_str(s) {
            return Ok(Self::ForDuration(duration.into()));
        }
        Err(format!(
            r#"Unable to parse retry constraint from {:?}

The accepted format is: [0-9]+(x|ns|us|ms|[smhdwy])

Examples
Retry 10 times: retry --up-to 10x ./foo
Retry for 100s: retry --up-to 100s ./foo"#,
            s
        )
        .into())
    }
}

#[derive(Debug, Parser)]
pub(crate) struct Cli {
    // TODO: make this optional to indicate "retry forever"
    /// Retry constraint expressed in number of attempts or total duration.
    ///
    /// Accepted format is:
    /// [0-9]+(x|ns|us|ms|[smhdwy])
    ///
    /// This is the same as duration below, with the option of specifying
    /// "x" (read: "times").
    ///
    /// Examples:
    /// ```
    /// retry --up-to 3x npm install
    /// retry --up-to 10m -- sh -c './download-new-publication && sleep 10s'
    /// ```
    #[arg(long, value_parser = Retry::from_str, help = "Retry constraint expressed in attempts or duration")]
    pub up_to: Retry,

    /// Timeout to enforce on each attempt.
    ///
    /// Accepted format is:
    /// [0-9]+(ns|us|ms|[smhdwy])
    ///
    /// Examples:
    /// ```
    /// retry --task-timeout 30s -- ping -c 1 google.com
    /// retry --task-timeout 5m ./download-all-the-data
    /// retry --task-timeout 7500000y ./what-is-the-answer
    /// ```
    #[arg(long, value_parser = duration_from_str, help = "Timeout to enforce on each attempt")]
    pub task_timeout: Option<Duration>,

    // TODO: enforce this is a non-empty array
    /// The command to run.
    ///
    /// Use double-dash (--) to pass flags or options to this command instead
    /// of to retry,
    ///
    /// Examples:
    /// ```
    /// retry ./simple-script
    /// retry -- ping -c 1 google.com
    /// retry -- sh -c 'do-work | head -n 20'
    /// ```
    #[arg(help = "The command to run")]
    pub command: Vec<String>,
}
