//! Owned process supervision with byte-preserving output capture.
//!
//! On Linux and macOS each child gets a new process group. Timeout, termination,
//! and guard drop kill that group, including descendants left by an exited leader.
//! Descendants which deliberately leave the group (e.g. `setsid`) are not covered.
//! Drop cannot run after abrupt supervisor death (SIGKILL, abort, power loss).
//! Linux is acceptance-tested; macOS uses the same POSIX implementation.
//!
//! Windows supports capture and direct-child cleanup only. Job objects and polite
//! console signals are not implemented: **Windows process-tree cleanup is not
//! guaranteed**. Capture never waits for inherited pipe handles to close.

use std::ffi::OsString;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
compile_error!("gdproject::process supports Linux, macOS, and Windows only");

const POLL_INTERVAL: Duration = Duration::from_millis(5);
/// Longest wait for the leader to exit after a hard kill. A leader stuck in
/// uninterruptible I/O can outlive SIGKILL; it is then left unreaped.
pub const KILL_REAP_LIMIT: Duration = Duration::from_secs(5);

/// Combined stdout/stderr payload limit, including pending unterminated lines.
/// Allocator capacity and record metadata are additional, bounded overhead.
pub const MAX_CAPTURE_BYTES: usize = 16 * 1024 * 1024;
/// Maximum output records, including a reserved record for each pending fragment.
/// This also bounds per-line allocations and metadata for tiny/empty lines.
pub const MAX_CAPTURE_LINES: usize = 65_536;

#[derive(Clone, Debug)]
pub struct Spawn {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(OsString, OsString)>,
}

impl Spawn {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: None,
            env: Vec::new(),
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
    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command
            .args(&self.args)
            .envs(self.env.iter().cloned())
            .stdin(Stdio::null());
        if let Some(cwd) = &self.cwd {
            command.current_dir(cwd);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        command
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

/// A complete line (including its newline), or a final unterminated fragment.
/// Sequence is zero-based observation order, not the unknowable write order
/// between two independent pipes. Bytes, including CRLF and invalid UTF-8, survive.
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
    /// `None` only when the leader was still alive [`KILL_REAP_LIMIT`] after
    /// a timeout or output-limit hard kill, so no exit status exists.
    pub status: Option<ExitStatus>,
    pub timed_out: bool,
    /// Capture exceeded the byte or record budget. Output is incomplete and the
    /// owned process tree was killed, even if the leader had already exited zero.
    pub output_limit_exceeded: bool,
    pub lines: Vec<OutputLine>,
    pub duration: Duration,
}

impl Captured {
    pub fn success(&self) -> bool {
        self.status.is_some_and(|status| status.success())
            && !self.timed_out
            && !self.output_limit_exceeded
    }
    pub fn stdout(&self) -> Vec<u8> {
        self.stream_bytes(OutputStream::Stdout)
    }
    pub fn stderr(&self) -> Vec<u8> {
        self.stream_bytes(OutputStream::Stderr)
    }
    fn stream_bytes(&self, stream: OutputStream) -> Vec<u8> {
        self.lines
            .iter()
            .filter(|line| line.stream == stream)
            .flat_map(|line| line.bytes.iter().copied())
            .collect()
    }
    pub fn lines_text(&self) -> impl Iterator<Item = std::borrow::Cow<'_, str>> {
        self.lines.iter().map(OutputLine::text)
    }
}

/// Runs to completion or `deadline` (measured from before spawn).
/// Both streams are drained without blocking. Timeout hard-kills the owned tree
/// and retains output already written, including unterminated lines. Successful
/// leader exit also cleans up remaining group members. Capture retains at most
/// [`MAX_CAPTURE_BYTES`] payload bytes and [`MAX_CAPTURE_LINES`] records across
/// both streams, including pending fragments. Exceeding either limit hard-kills
/// the group, preserves the bounded prefix, and sets `output_limit_exceeded`;
/// it is not a timeout or a successful run. Exactly filling a budget is allowed.
/// Vec capacity growth and per-record allocations add bounded memory overhead.
/// Group cleanup after the leader exits is best effort and never discards
/// capture. After a hard kill the leader is awaited for at most
/// [`KILL_REAP_LIMIT`]; if it survives, `status` is `None`.
pub fn run(spawn: &Spawn, deadline: Duration) -> io::Result<Captured> {
    let started = Instant::now();
    let child = spawn
        .command()
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut guard = ChildGuard::new(child);
    let mut stdout = guard.child.stdout.take().expect("piped stdout");
    let mut stderr = guard.child.stderr.take().expect("piped stderr");
    platform::prepare(&stdout)?;
    platform::prepare(&stderr)?;
    let mut out = CaptureBuffer::new(OutputStream::Stdout);
    let mut err = CaptureBuffer::new(OutputStream::Stderr);
    let mut lines = Vec::new();
    let mut budget = CaptureBudget::default();
    let (status, timed_out) = loop {
        let a = out.drain(&mut stdout, &mut lines, &mut budget)?;
        let b = err.drain(&mut stderr, &mut lines, &mut budget)?;
        if budget.exceeded {
            break (guard.kill_and_reap()?, false);
        }
        if let Some(status) = guard.try_wait()? {
            break (Some(status), false);
        }
        if started.elapsed() >= deadline {
            break (guard.kill_and_reap()?, true);
        }
        if !a && !b {
            std::thread::sleep(POLL_INTERVAL.min(deadline.saturating_sub(started.elapsed())));
        }
    };
    // Never wait for EOF: a process outside the group may hold a pipe open.
    // Bound this phase too, in case such a process is continuously writing.
    let draining = Instant::now();
    loop {
        let a = out.drain(&mut stdout, &mut lines, &mut budget)?;
        let b = err.drain(&mut stderr, &mut lines, &mut budget)?;
        if budget.exceeded || (!a && !b) || draining.elapsed() >= Duration::from_millis(100) {
            break;
        }
    }
    out.finish(&mut lines);
    err.finish(&mut lines);
    Ok(Captured {
        pid: guard.pid,
        status,
        timed_out,
        output_limit_exceeded: budget.exceeded,
        lines,
        duration: started.elapsed(),
    })
}

#[derive(Default)]
struct CaptureBudget {
    bytes: usize,
    records: usize,
    exceeded: bool,
}

struct CaptureBuffer {
    stream: OutputStream,
    pending: Vec<u8>,
}

impl CaptureBuffer {
    fn new(stream: OutputStream) -> Self {
        Self {
            stream,
            pending: Vec::new(),
        }
    }

    fn emit(&self, bytes: Vec<u8>, lines: &mut Vec<OutputLine>) {
        lines.push(OutputLine {
            sequence: lines.len(),
            stream: self.stream,
            bytes,
            observed_at_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .min(u64::MAX as u128) as u64,
        });
    }

    fn drain(
        &mut self,
        pipe: &mut impl platform::Pipe,
        lines: &mut Vec<OutputLine>,
        budget: &mut CaptureBudget,
    ) -> io::Result<bool> {
        if budget.exceeded {
            return Ok(false);
        }
        let mut read_any = false;
        let mut buffer = [0; 8192];
        // Fairness: a busy stream must not starve stderr or deadline checks.
        for _ in 0..4 {
            let count = match platform::read(pipe, &mut buffer) {
                Ok(0) => break,
                Ok(count) => count,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            };
            read_any = true;
            for segment in buffer[..count].split_inclusive(|byte| *byte == b'\n') {
                if budget.bytes == MAX_CAPTURE_BYTES
                    || (self.pending.is_empty() && budget.records == MAX_CAPTURE_LINES)
                {
                    budget.exceeded = true;
                    return Ok(true);
                }
                // Reserve a record on its first byte, not on newline/EOF: both
                // streams' final fragments must fit even when the budget trips.
                if self.pending.is_empty() {
                    budget.records += 1;
                }
                let accepted = segment.len().min(MAX_CAPTURE_BYTES - budget.bytes);
                self.pending.extend_from_slice(&segment[..accepted]);
                budget.bytes += accepted;
                if self.pending.last() == Some(&b'\n') {
                    let bytes = std::mem::take(&mut self.pending);
                    self.emit(bytes, lines);
                }
                if accepted < segment.len() {
                    budget.exceeded = true;
                    return Ok(true);
                }
            }
        }
        Ok(read_any)
    }

    fn finish(&mut self, lines: &mut Vec<OutputLine>) {
        if !self.pending.is_empty() {
            let bytes = std::mem::take(&mut self.pending);
            self.emit(bytes, lines);
        }
    }
}

/// Spawns without waiting; both streams append directly to the same open log
/// file, with no buffering threads. The parent directory must already exist.
/// Stdin is closed. The guard owns cleanup even if the leader exits first.
pub fn spawn(spawn: &Spawn, log: &Path) -> io::Result<ChildGuard> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)?;
    let stderr = file.try_clone()?;
    let child = spawn.command().stdout(file).stderr(stderr).spawn()?;
    Ok(ChildGuard::new(child))
}

pub struct ChildGuard {
    pid: u32,
    child: Child,
    /// Set once the leader is reaped. The group is never signalled afterwards:
    /// only the unreaped leader pins its process-group ID against reuse.
    status: Option<ExitStatus>,
    /// The leader outlived [`KILL_REAP_LIMIT`] after SIGKILL; drop won't wait again.
    unresponsive: bool,
    // Fields drop after our Drop body has killed and reaped the child.
    retained: Vec<Box<dyn Send>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminateOutcome {
    /// Leader exited within grace after SIGTERM; remaining descendants killed.
    Exited,
    /// Needed the hard kill (always used on Windows).
    Killed,
    /// Leader had already exited; remaining descendants still cleaned up.
    AlreadyExited,
}

impl ChildGuard {
    fn new(child: Child) -> Self {
        Self {
            pid: child.id(),
            child,
            status: None,
            unresponsive: false,
            retained: Vec::new(),
        }
    }
    /// Keeps a resource (such as a harness directory) alive until this guard is
    /// dropped, after process cleanup and reaping. Polling or terminating the
    /// child does not release retained resources.
    pub fn retain(&mut self, resource: impl Send + 'static) {
        self.retained.push(Box::new(resource));
    }
    pub fn pid(&self) -> u32 {
        self.pid
    }
    /// Polls without waiting. Once the leader exits, also kills remaining group
    /// members (best effort) before reaping it, rather than retaining a group ID
    /// that could be reused by the time of a later drop.
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        if self.status.is_none() && platform::exited(&mut self.child)? {
            // The zombie leader still pins the group ID, so this cannot reach
            // an unrelated group. Failure must not lose the exit: a zombie-only
            // group may reject signals (EPERM on macOS), and ESRCH is benign.
            let _ = platform::kill_tree(&mut self.child);
            self.status = Some(self.child.wait()?);
        }
        Ok(self.status)
    }
    /// Hard-kills the tree, then waits at most [`KILL_REAP_LIMIT`] for the
    /// leader. `None` means it is still alive and deliberately left unreaped.
    fn kill_and_reap(&mut self) -> io::Result<Option<ExitStatus>> {
        if self.status.is_some() {
            return Ok(self.status);
        }
        if let Err(error) = platform::kill_tree(&mut self.child) {
            // Reaping only needs the leader gone; fall back to it alone.
            if !platform::exited(&mut self.child)? && self.child.kill().is_err() {
                return Err(error);
            }
        }
        let started = Instant::now();
        loop {
            if let Some(status) = self.try_wait()? {
                return Ok(Some(status));
            }
            if started.elapsed() >= KILL_REAP_LIMIT {
                self.unresponsive = true;
                return Ok(None);
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }
    /// SIGTERM then SIGKILL after `grace` on Unix; immediate direct-child kill
    /// on Windows. Reaps the direct child, waiting at most [`KILL_REAP_LIMIT`]
    /// after the hard kill. A leader that survives that (e.g. stuck in
    /// uninterruptible I/O) yields a `TimedOut` error and stays unreaped; drop
    /// then kills and polls once more without blocking. Idempotent after completion.
    pub fn terminate(&mut self, grace: Duration) -> io::Result<TerminateOutcome> {
        if self.try_wait()?.is_some() {
            return Ok(TerminateOutcome::AlreadyExited);
        }
        #[cfg(unix)]
        {
            if let Err(error) = platform::signal(self.pid, libc::SIGTERM) {
                // The leader may have exited since the poll above.
                if !platform::exited(&mut self.child)? {
                    return Err(error);
                }
            }
            let started = Instant::now();
            while started.elapsed() < grace {
                if self.try_wait()?.is_some() {
                    return Ok(TerminateOutcome::Exited);
                }
                std::thread::sleep(POLL_INTERVAL.min(grace.saturating_sub(started.elapsed())));
            }
        }
        match self.kill_and_reap()? {
            Some(_) => Ok(TerminateOutcome::Killed),
            None => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "process {} did not exit within {KILL_REAP_LIMIT:?} of being killed",
                    self.pid
                ),
            )),
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.status.is_some() {
            return;
        }
        if self.unresponsive {
            // Already waited once in vain; retry without blocking again.
            let _ = platform::kill_tree(&mut self.child);
            let _ = self.try_wait();
        } else {
            let _ = self.kill_and_reap();
        }
    }
}

#[cfg(unix)]
mod platform {
    use super::*;
    use std::os::fd::AsRawFd;

    pub trait Pipe: Read + AsRawFd {}
    impl<T: Read + AsRawFd> Pipe for T {}

    pub fn prepare(pipe: &impl Pipe) -> io::Result<()> {
        // SAFETY: the pipe owns a live descriptor; F_GETFL/F_SETFL take these args.
        let flags = unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_GETFL) };
        if flags == -1
            || unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) }
                == -1
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    pub fn read(pipe: &mut impl Pipe, buffer: &mut [u8]) -> io::Result<usize> {
        pipe.read(buffer)
    }
    /// Whether the unreaped leader has exited. WNOWAIT leaves it a zombie, so
    /// its pid (and therefore its group ID) stays reserved until `wait` reaps it.
    pub fn exited(child: &mut Child) -> io::Result<bool> {
        loop {
            // SAFETY: all-zero is a valid siginfo_t; waitid only writes into it.
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            // SAFETY: a valid out-pointer and a child pid this process has not reaped.
            let result = unsafe {
                libc::waitid(
                    libc::P_PID,
                    child.id() as libc::id_t,
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if result == 0 {
                // POSIX: with WNOHANG and nothing waitable, si_pid stays zero.
                // SAFETY: si_pid is valid for the zeroed or SIGCHLD-filled info.
                return Ok(unsafe { info.si_pid() } != 0);
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }
    /// Callers must not use this after the leader was reaped.
    pub fn kill_tree(child: &mut Child) -> io::Result<()> {
        signal(child.id(), libc::SIGKILL)
    }
    pub fn signal(pid: u32, signal: i32) -> io::Result<()> {
        // SAFETY: negative pid targets the process group created during spawn.
        if unsafe { libc::kill(-(pid as libc::pid_t), signal) } == -1 {
            let error = io::Error::last_os_error();
            // ESRCH: group already gone.
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(error);
            }
        }
        Ok(())
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn PeekNamedPipe(
            handle: *mut c_void,
            buffer: *mut c_void,
            size: u32,
            read: *mut u32,
            available: *mut u32,
            left: *mut u32,
        ) -> i32;
    }
    pub trait Pipe: Read + AsRawHandle {}
    impl<T: Read + AsRawHandle> Pipe for T {}
    pub fn prepare(_: &impl Pipe) -> io::Result<()> {
        Ok(())
    }
    /// The owned process handle keeps the pid reserved, so polling may reap.
    pub fn exited(child: &mut Child) -> io::Result<bool> {
        Ok(child.try_wait()?.is_some())
    }
    /// Direct child only; see the module docs.
    pub fn kill_tree(child: &mut Child) -> io::Result<()> {
        if child.try_wait()?.is_none() {
            child.kill()?;
        }
        Ok(())
    }
    pub fn read(pipe: &mut impl Pipe, buffer: &mut [u8]) -> io::Result<usize> {
        let mut available = 0;
        // SAFETY: live pipe handle and valid output pointer; optional args null.
        let result = unsafe {
            PeekNamedPipe(
                pipe.as_raw_handle(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        };
        if result == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(109) {
                return Ok(0);
            } // broken pipe
            return Err(error);
        }
        if available == 0 {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let count = buffer.len().min(available as usize);
        pipe.read(&mut buffer[..count])
    }
}
