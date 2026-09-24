//! Process supervision. The only module allowed to spawn, poll, or kill.
//!
//! Nothing here outlives the calling gdkit process: every child is owned by a
//! [`ChildGuard`] that kills the whole tree on drop. That removes pid bookkeeping,
//! start-time identity, and cross-invocation liveness entirely.
//!
//! Every child is spawned in its own process group (Unix) or job object (Windows)
//! so termination is transitive. Platform support is explicit: Linux, macOS,
//! Windows. Anything else fails to compile rather than silently degrading.
//!
//! # Tests (tests/process.rs, all offline, spawn `sleep`/`sh`/`cmd` as the subject)
//! - `run_captures_interleaved_stdout_stderr_in_observation_order`
//! - `run_enforces_deadline_and_reports_timed_out`
//! - `run_kills_descendants_on_timeout` (sh -c "sleep 30 & wait")
//! - `spawn_streams_output_to_the_log_file_while_running`
//! - `guard_terminate_waits_for_exit_and_escalates_to_kill_after_grace`
//! - `guard_drop_kills_the_tree`
//! - `try_wait_reports_exit_status_without_blocking`

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
compile_error!("gdproject::process supports Linux, macOS, and Windows only");

#[derive(Clone, Debug)]
pub struct Spawn {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(OsString, OsString)>,
}

impl Spawn {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self { program: program.into(), args: Vec::new(), cwd: None, env: Vec::new() }
    }
    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }
    pub fn args<I: IntoIterator<Item = S>, S: Into<OsString>>(mut self, args: I) -> Self {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }
    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

#[derive(Clone, Debug)]
pub struct OutputLine {
    pub sequence: usize,
    pub stream: OutputStream,
    pub bytes: Vec<u8>,
    pub observed_at_unix_ms: u64,
}

impl OutputLine {
    pub fn text(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(&self.bytes)
    }
}

#[derive(Debug)]
pub struct Captured {
    pub pid: u32,
    pub status: Option<std::process::ExitStatus>,
    pub timed_out: bool,
    pub lines: Vec<OutputLine>,
    pub duration: Duration,
}

impl Captured {
    pub fn success(&self) -> bool {
        self.status.is_some_and(|status| status.success()) && !self.timed_out
    }
    pub fn stdout(&self) -> Vec<u8> {
        todo!()
    }
    pub fn stderr(&self) -> Vec<u8> {
        todo!()
    }
    pub fn lines_text(&self) -> impl Iterator<Item = std::borrow::Cow<'_, str>> {
        self.lines.iter().map(OutputLine::text)
    }
}

/// Runs to completion or `deadline`, capturing both streams line by line.
/// On deadline the process tree is terminated and `timed_out` is set.
pub fn run(spawn: &Spawn, deadline: Duration) -> std::io::Result<Captured> {
    todo!()
}

/// Spawns without waiting; both streams are appended to `log` as they arrive.
/// The guard kills the tree on drop.
pub fn spawn(spawn: &Spawn, log: &Path) -> std::io::Result<ChildGuard> {
    todo!()
}

pub struct ChildGuard {
    pid: u32,
    // platform handle / Child / job object
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminateOutcome {
    /// Exited within grace after a polite signal.
    Exited,
    /// Needed the hard kill.
    Killed,
    /// Had already exited.
    AlreadyExited,
}

impl ChildGuard {
    pub fn pid(&self) -> u32 {
        self.pid
    }
    pub fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        todo!()
    }
    /// TERM (or CTRL_BREAK / job close) then KILL after `grace`. Always waits for exit.
    pub fn terminate(&mut self, grace: Duration) -> std::io::Result<TerminateOutcome> {
        todo!()
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        todo!()
    }
}
