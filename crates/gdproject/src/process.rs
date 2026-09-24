//! Process supervision. The only module allowed to spawn, poll, or kill.
//!
//! Identity is `(pid, start_time)`; a bare pid is never trusted across calls.
//! Every long-lived child is spawned in its own process group (Unix) or job
//! object (Windows) so `terminate` takes descendants with it.
//!
//! Platform support is explicit: Linux, macOS, Windows. Anything else fails to
//! compile rather than silently degrading.
//!
//! # Tests (tests/process.rs, all offline, spawn `sleep`/`cmd` as the subject)
//! - `run_captures_interleaved_stdout_stderr_in_observation_order`
//! - `run_enforces_deadline_and_reports_timed_out`
//! - `run_kills_descendants_on_timeout` (sh -c "sleep 30 & wait")
//! - `identity_detects_pid_reuse_via_start_time`
//! - `is_alive_is_false_for_zombies_after_reap`
//! - `terminate_waits_for_exit_and_escalates_to_kill_after_grace`
//! - `terminate_on_stale_identity_is_a_noop_that_returns_not_found`
//! - `spawn_detached_survives_parent_exit_and_ignores_sigint` (unix: setsid; windows: new console/job)
//! - `child_guard_kills_on_drop_unless_released`

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
compile_error!("gdproject::process supports Linux, macOS, and Windows only");

#[derive(Clone, Debug)]
pub struct Spawn {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(OsString, OsString)>,
    /// Isolate in a process group / job so termination is transitive.
    pub own_group: bool,
    /// Survive parent exit; no inherited console/tty.
    pub detached: bool,
}

impl Spawn {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: None,
            env: Vec::new(),
            own_group: true,
            detached: false,
        }
    }
    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }
    pub fn args<I: IntoIterator<Item = S>, S: Into<OsString>>(mut self, args: I) -> Self {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }
}

/// `(pid, start time)` so a reused pid is never mistaken for our process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessId {
    pub pid: u32,
    /// Platform-specific start stamp (jiffies on Linux, FILETIME on Windows, kinfo on macOS).
    pub started: u64,
}

impl ProcessId {
    pub fn of_running(pid: u32) -> std::io::Result<Self> {
        todo!()
    }
    pub fn is_alive(&self) -> bool {
        todo!()
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
    pub id: ProcessId,
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

/// Spawns without waiting. The returned guard kills the tree on drop unless released.
pub fn spawn(spawn: &Spawn, log: Option<&std::path::Path>) -> std::io::Result<ChildGuard> {
    todo!()
}

pub struct ChildGuard {
    id: ProcessId,
    released: bool,
    spawned_at: Instant,
    // platform handle / Child
}

impl ChildGuard {
    pub fn id(&self) -> ProcessId {
        self.id
    }
    /// Stop supervising: the process outlives this handle (durable sessions).
    pub fn release(mut self) -> ProcessId {
        self.released = true;
        self.id
    }
    pub fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        todo!()
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        todo!()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminateOutcome {
    /// Exited within grace after a polite signal.
    Exited,
    /// Needed the hard kill.
    Killed,
    /// Identity did not match a live process; nothing done.
    NotFound,
}

/// TERM (or CTRL_BREAK / job close) then KILL after `grace`. Always waits for exit.
pub fn terminate(id: &ProcessId, grace: Duration) -> std::io::Result<TerminateOutcome> {
    todo!()
}

/// Immediate hard kill of the tree. Used by `scenario crash`.
pub fn kill(id: &ProcessId) -> std::io::Result<TerminateOutcome> {
    todo!()
}
