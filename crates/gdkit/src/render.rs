//! One place that decides how a typed report reaches stdout.

use std::io::Write;

use serde::Serialize;

use crate::cli::Output;

/// Human rendering for a report. JSON rendering is `Serialize`.
pub trait Human {
    fn human(&self, out: &mut dyn Write) -> std::io::Result<()>;
}

/// Writes JSON (one document, trailing newline) or the human form.
pub fn emit<T: Serialize + Human>(output: Output, report: &T) -> std::io::Result<()> {
    let mut stdout = std::io::stdout().lock();
    match output {
        Output::Json => {
            serde_json::to_writer_pretty(&mut stdout, report)?;
            writeln!(stdout)?;
        }
        Output::Human => report.human(&mut stdout)?,
    }
    stdout.flush()
}

/// What a command returns. `main` maps it to the process exit code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    Ok,
    Failed,
}

impl From<Exit> for std::process::ExitCode {
    fn from(exit: Exit) -> Self {
        match exit {
            Exit::Ok => std::process::ExitCode::SUCCESS,
            Exit::Failed => std::process::ExitCode::from(1),
        }
    }
}
