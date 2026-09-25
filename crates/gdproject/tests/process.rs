//! Offline acceptance tests. Unix shell fixtures; tree liveness checks use Linux
//! /proc and treat zombies as dead (only their adopting parent can reap them).
#![cfg(unix)]

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use gdproject::process::{self, OutputStream, Spawn, TerminateOutcome};

// Generous: only failures wait this long, so a loaded machine cannot flake.
const LIMIT: Duration = Duration::from_secs(20);
/// Upper bound for "returned promptly" checks; still far below every 30s sleep.
const PROMPT: Duration = Duration::from_secs(10);

fn shell(script: &str) -> Spawn {
    Spawn::new("/bin/sh").args(["-c", script])
}

fn eventually(mut predicate: impl FnMut() -> bool) {
    let start = Instant::now();
    while !predicate() {
        assert!(
            start.elapsed() < LIMIT,
            "condition did not become true in {LIMIT:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn log_contains(path: &Path, text: &str) -> bool {
    fs::read_to_string(path).is_ok_and(|s| s.contains(text))
}

#[test]
fn run_captures_interleaved_stdout_stderr_in_observation_order() {
    let result = process::run(
        &shell("printf 'one\n'; sleep 0.1; printf 'two\n' >&2; sleep 0.1; printf 'three\n'"),
        LIMIT,
    )
    .unwrap();
    assert!(result.success());
    assert!(result.pid > 0);
    assert_eq!(result.stdout(), b"one\nthree\n");
    assert_eq!(result.stderr(), b"two\n");
    assert_eq!(
        result.lines_text().collect::<Vec<_>>(),
        ["one\n", "two\n", "three\n"]
    );
    assert_eq!(
        result.lines.iter().map(|l| l.stream).collect::<Vec<_>>(),
        [
            OutputStream::Stdout,
            OutputStream::Stderr,
            OutputStream::Stdout
        ]
    );
    for (index, line) in result.lines.iter().enumerate() {
        assert_eq!(line.sequence, index);
        assert!(line.observed_at_unix_ms > 0);
    }
}

#[test]
fn run_preserves_binary_crlf_blank_lines_and_final_fragments() {
    let result = process::run(
        &shell(r"printf '\377\000\r\n\nfragment'; printf 'error\377' >&2"),
        LIMIT,
    )
    .unwrap();
    assert_eq!(result.stdout(), b"\xff\0\r\n\nfragment");
    assert_eq!(result.stderr(), b"error\xff");
    assert!(result.lines_text().any(|line| line.contains('\u{fffd}')));
    assert_eq!(result.lines.len(), 4);
}

#[test]
fn run_drains_both_streams_beyond_pipe_capacity_without_deadlock() {
    let result = process::run(&shell("(i=0; while [ $i -lt 12000 ]; do printf 'stdout payload\n'; i=$((i+1)); done) & i=0; while [ $i -lt 12000 ]; do printf 'stderr payload\n' >&2; i=$((i+1)); done; wait"), LIMIT).unwrap();
    assert!(result.success());
    assert_eq!(result.stdout(), b"stdout payload\n".repeat(12000));
    assert_eq!(result.stderr(), b"stderr payload\n".repeat(12000));
    assert_eq!(result.lines.len(), 24000);
    assert!(
        result
            .lines
            .iter()
            .enumerate()
            .all(|(i, line)| i == line.sequence)
    );
}

#[test]
fn capture_byte_limit_counts_both_streams_and_unterminated_fragments() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input");
    let prefix = b"error fragment";
    fs::write(
        &input,
        vec![b'x'; process::MAX_CAPTURE_BYTES - prefix.len()],
    )
    .unwrap();
    let spec = shell("printf 'error fragment' >&2; cat \"$1\"")
        .arg("fixture")
        .arg(input.as_os_str());
    let mut exact = process::run(&spec, Duration::from_secs(30)).unwrap();
    assert!(exact.success());
    assert!(!exact.output_limit_exceeded);
    assert_eq!(exact.stderr(), prefix);
    assert_eq!(
        exact
            .lines
            .iter()
            .map(|line| line.bytes.len())
            .sum::<usize>(),
        process::MAX_CAPTURE_BYTES
    );
    assert_eq!(exact.lines.len(), 2);
    // Even a zero exit status must not turn incomplete output into success.
    exact.output_limit_exceeded = true;
    assert!(!exact.success());
    drop(exact);

    fs::OpenOptions::new()
        .append(true)
        .open(&input)
        .unwrap()
        .write_all(b"x")
        .unwrap();
    let exceeded = process::run(&spec, Duration::from_secs(30)).unwrap();
    assert!(exceeded.output_limit_exceeded);
    assert!(!exceeded.success());
    assert!(!exceeded.timed_out);
    assert_eq!(exceeded.stderr(), prefix);
    assert_eq!(exceeded.lines.len(), 2);
    assert_eq!(
        exceeded
            .lines
            .iter()
            .map(|line| line.bytes.len())
            .sum::<usize>(),
        process::MAX_CAPTURE_BYTES
    );
    assert!(exceeded.stdout().iter().all(|byte| *byte == b'x'));
}

#[test]
fn capture_record_limit_bounds_empty_lines_and_reserves_pending_fragments() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input");
    fs::write(&input, vec![b'\n'; process::MAX_CAPTURE_LINES - 1]).unwrap();
    let spec = shell("printf 'partial' >&2; cat \"$1\"")
        .arg("fixture")
        .arg(input.as_os_str());
    let exact = process::run(&spec, LIMIT).unwrap();
    assert!(exact.success());
    assert!(!exact.output_limit_exceeded);
    assert_eq!(exact.lines.len(), process::MAX_CAPTURE_LINES);
    assert_eq!(exact.stderr(), b"partial");
    drop(exact);

    fs::OpenOptions::new()
        .append(true)
        .open(&input)
        .unwrap()
        .write_all(b"\n")
        .unwrap();
    let exceeded = process::run(&spec, LIMIT).unwrap();
    assert!(exceeded.output_limit_exceeded);
    assert!(!exceeded.success());
    assert!(!exceeded.timed_out);
    assert_eq!(exceeded.lines.len(), process::MAX_CAPTURE_LINES);
    assert_eq!(exceeded.stderr(), b"partial");
    assert_eq!(
        exceeded.stdout(),
        vec![b'\n'; process::MAX_CAPTURE_LINES - 1]
    );
    assert!(
        exceeded
            .lines
            .iter()
            .enumerate()
            .all(|(i, line)| line.sequence == i)
    );
}

#[test]
fn run_captures_a_line_larger_than_pipe_capacity() {
    let result = process::run(
        &shell("i=0; while [ $i -lt 20000 ]; do printf 'abcdefgh'; i=$((i+1)); done"),
        LIMIT,
    )
    .unwrap();
    assert!(result.success());
    assert_eq!(result.stdout(), b"abcdefgh".repeat(20000));
    assert_eq!(result.lines.len(), 1);
}

#[test]
fn run_enforces_deadline_and_reports_timed_out_with_partial_output() {
    let result = process::run(
        &shell("printf 'ready\npartial'; printf 'error fragment' >&2; sleep 30"),
        Duration::from_secs(1),
    )
    .unwrap();
    assert!(result.timed_out);
    assert!(!result.success());
    assert!(result.status.is_some());
    assert!(result.duration >= Duration::from_secs(1));
    assert!(result.duration < PROMPT);
    assert_eq!(result.stdout(), b"ready\npartial");
    assert_eq!(result.stderr(), b"error fragment");
}

#[test]
fn run_checks_deadline_even_when_output_never_stops() {
    let result = process::run(
        &shell("while :; do printf 'busy\n'; printf 'err\n' >&2; done"),
        Duration::from_millis(100),
    )
    .unwrap();
    assert!(result.timed_out);
    assert!(result.duration < PROMPT);
    assert!(!result.stdout().is_empty());
    assert!(!result.stderr().is_empty());
}

#[test]
fn zero_deadline_and_closed_streams_do_not_hang() {
    let result = process::run(&shell("exec 1>&- 2>&-; sleep 30"), Duration::ZERO).unwrap();
    assert!(result.timed_out);
    assert!(result.duration < PROMPT);
    let result = process::run(
        &shell("exec 1>&- 2>&-; sleep 30"),
        Duration::from_millis(100),
    )
    .unwrap();
    assert!(result.timed_out);
    assert!(result.lines.is_empty());
}

#[test]
fn spawn_configuration_and_nonzero_exit_are_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let mut spec = shell("printf '%s\n' \"$VALUE\" \"$1\"; pwd; exit 7")
        .arg("fixture")
        .arg("argument with spaces")
        .env("VALUE", "environment value");
    spec.cwd = Some(dir.path().to_owned());
    let result = process::run(&spec, LIMIT).unwrap();
    assert!(!result.success());
    assert!(!result.timed_out);
    assert_eq!(result.status.unwrap().code(), Some(7));
    assert_eq!(
        result.stdout(),
        format!(
            "environment value\nargument with spaces\n{}\n",
            dir.path().display()
        )
        .as_bytes()
    );
}

#[test]
fn stdin_is_closed_and_empty_success_has_no_lines() {
    let result = process::run(&shell("if read line; then exit 1; fi"), LIMIT).unwrap();
    assert!(result.success());
    assert!(result.lines.is_empty());
    assert!(result.stdout().is_empty());
    assert!(result.stderr().is_empty());

    let dir = tempfile::tempdir().unwrap();
    let mut guard = process::spawn(
        &shell("if read line; then exit 1; fi"),
        &dir.path().join("log"),
    )
    .unwrap();
    eventually(|| guard.try_wait().unwrap().is_some());
    assert!(guard.try_wait().unwrap().unwrap().success());
}

#[test]
fn spawn_and_run_report_setup_errors() {
    let dir = tempfile::tempdir().unwrap();
    let missing = Spawn::new(dir.path().join("missing-program"));
    assert!(process::run(&missing, LIMIT).is_err());
    assert!(process::spawn(&missing, &dir.path().join("log")).is_err());
    assert!(process::spawn(&shell("exit 0"), dir.path()).is_err());
    let mut invalid_cwd = shell("exit 0");
    invalid_cwd.cwd = Some(dir.path().join("missing-directory"));
    assert!(process::run(&invalid_cwd, LIMIT).is_err());
}

#[test]
fn spawn_streams_output_to_the_log_file_while_running() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("game.log");
    fs::write(&log, "previous\n").unwrap();
    let mut spec = shell("printf '%s\n' \"$VALUE\"; pwd; printf 'stderr\n' >&2; sleep 30")
        .env("VALUE", "stdout");
    spec.cwd = Some(dir.path().to_owned());
    let mut guard = process::spawn(&spec, &log).unwrap();
    eventually(|| log_contains(&log, "stderr\n"));
    assert!(guard.try_wait().unwrap().is_none());
    let text = fs::read_to_string(&log).unwrap();
    assert!(text.starts_with("previous\nstdout\n"));
    assert!(text.contains(&format!("{}\n", dir.path().display())));
    guard.terminate(Duration::ZERO).unwrap();
}

#[test]
fn guard_terminate_waits_for_exit_and_escalates_to_kill_after_grace() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("log");
    let mut guard = process::spawn(
        &shell("trap '' TERM; printf 'ready\n'; while :; do sleep 30; done"),
        &log,
    )
    .unwrap();
    eventually(|| log_contains(&log, "ready"));
    let start = Instant::now();
    assert_eq!(
        guard.terminate(Duration::from_millis(100)).unwrap(),
        TerminateOutcome::Killed
    );
    assert!(start.elapsed() >= Duration::from_millis(100));
    assert!(start.elapsed() < PROMPT);
    assert!(guard.try_wait().unwrap().is_some());
    assert_eq!(
        guard.terminate(LIMIT).unwrap(),
        TerminateOutcome::AlreadyExited
    );
}

#[test]
fn guard_terminate_allows_polite_exit() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("log");
    let mut guard = process::spawn(
        &shell("trap 'exit 0' TERM; printf 'ready\n'; while :; do :; done"),
        &log,
    )
    .unwrap();
    eventually(|| log_contains(&log, "ready"));
    assert_eq!(guard.terminate(LIMIT).unwrap(), TerminateOutcome::Exited);
    assert!(guard.try_wait().unwrap().unwrap().success());
}

#[test]
fn try_wait_reports_exit_status_without_blocking() {
    let dir = tempfile::tempdir().unwrap();
    let release = dir.path().join("release");
    let spec = shell("while [ ! -f \"$1\" ]; do sleep 0.01; done; exit 23")
        .arg("fixture")
        .arg(release.as_os_str());
    let mut guard = process::spawn(&spec, &dir.path().join("log")).unwrap();
    let start = Instant::now();
    assert!(guard.try_wait().unwrap().is_none());
    assert!(start.elapsed() < PROMPT);
    fs::write(release, "").unwrap();
    eventually(|| guard.try_wait().unwrap().is_some());
    assert_eq!(guard.try_wait().unwrap().unwrap().code(), Some(23));
    assert_eq!(
        guard.terminate(LIMIT).unwrap(),
        TerminateOutcome::AlreadyExited
    );
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;

    fn dead(pid: u32) -> bool {
        match fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat) => matches!(
                stat.rsplit_once(") ").unwrap().1.chars().next(),
                Some('Z' | 'X')
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
            Err(error) => panic!("cannot inspect process {pid}: {error}"),
        }
    }

    fn pids(bytes: &[u8]) -> Vec<u32> {
        String::from_utf8_lossy(bytes)
            .split_whitespace()
            .map(|pid| pid.parse().unwrap())
            .collect()
    }

    #[test]
    fn timeout_kills_grandchildren_and_preserves_signal_exit_status() {
        use std::os::unix::process::ExitStatusExt;
        let result = process::run(
            &shell("sh -c 'sleep 30 & printf \"%s\\n%s\\n\" $$ $!; wait' & wait"),
            Duration::from_secs(1),
        )
        .unwrap();
        assert!(result.timed_out);
        assert_eq!(result.status.unwrap().signal(), Some(9));
        let descendants = pids(&result.stdout());
        assert_eq!(descendants.len(), 2);
        for pid in descendants {
            eventually(|| dead(pid));
        }
    }

    #[test]
    fn capture_limit_kills_the_group_and_reaps_the_leader_before_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pids");
        let spec = shell("sleep 30 & printf '%s\\n%s\\n' $$ $! > \"$1\"; while :; do printf '\\n\\n\\n\\n\\n\\n\\n\\n'; done")
            .arg("fixture").arg(pid_file.as_os_str());
        let result = process::run(&spec, Duration::from_secs(30)).unwrap();
        assert!(result.output_limit_exceeded);
        assert!(!result.timed_out);
        assert!(!result.success());
        assert!(result.duration < PROMPT);
        assert_eq!(result.lines.len(), process::MAX_CAPTURE_LINES);
        let processes = pids(&fs::read(pid_file).unwrap());
        assert_eq!(processes.len(), 2);
        assert_eq!(processes[0], result.pid);
        assert!(!Path::new(&format!("/proc/{}", result.pid)).exists());
        for pid in processes {
            eventually(|| dead(pid));
        }
    }

    #[test]
    fn run_kills_descendants_on_timeout() {
        let result = process::run(
            &shell("sleep 30 & printf '%s\n%s\n' $$ $!; wait"),
            Duration::from_secs(1),
        )
        .unwrap();
        assert!(result.timed_out);
        let pids = pids(&result.stdout());
        assert_eq!(pids.len(), 2);
        for pid in pids {
            eventually(|| dead(pid));
        }
    }

    #[test]
    fn run_cleans_up_descendants_after_leader_exit_without_waiting_for_pipe_eof() {
        let result = process::run(&shell("sleep 30 & printf '%s\n' $!; exit 0"), LIMIT).unwrap();
        assert!(result.success());
        assert!(result.duration < PROMPT);
        for pid in pids(&result.stdout()) {
            eventually(|| dead(pid));
        }
    }

    #[test]
    fn run_bounds_the_drain_when_an_escaped_descendant_holds_the_pipes_open() {
        struct KillOnDrop(u32);
        impl Drop for KillOnDrop {
            fn drop(&mut self) {
                let _ = std::process::Command::new("/bin/sh")
                    .args(["-c", "kill -KILL \"$1\"", "kill"])
                    .arg(self.0.to_string())
                    .status();
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("escaped");
        // setsid leaves the group, so only a bounded drain (not EOF) ends capture.
        let spec = shell(
            r#"setsid sh -c 'echo $$ > "$1"; exec sleep 30' escaped "$1" &
while [ ! -s "$1" ]; do sleep 0.01; done; printf 'leader done\n'"#,
        )
        .arg("fixture")
        .arg(pid_file.as_os_str());
        let result = process::run(&spec, LIMIT);
        let escaped = KillOnDrop(pids(&fs::read(&pid_file).unwrap())[0]);
        let result = result.unwrap();
        assert!(result.success());
        assert_eq!(result.stdout(), b"leader done\n");
        assert!(result.duration < PROMPT);
        // It escaped cleanup and still holds the inherited stdout/stderr.
        assert!(!dead(escaped.0));
        let stat = fs::read_to_string(format!("/proc/{}/stat", escaped.0)).unwrap();
        let group = stat.rsplit_once(") ").unwrap().1.split(' ').nth(2).unwrap();
        assert_ne!(group, result.pid.to_string());
        drop(escaped);
    }

    #[test]
    fn guard_retains_resources_until_after_kill_and_reap() {
        use std::sync::mpsc;

        struct ReapProbe {
            pid: u32,
            dropped: mpsc::Sender<bool>,
        }

        impl Drop for ReapProbe {
            fn drop(&mut self) {
                // Unlike a zombie-state check, absence verifies that the direct
                // child was reaped before the retained resource was released.
                let reaped = fs::metadata(format!("/proc/{}", self.pid))
                    .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound);
                let _ = self.dropped.send(reaped);
            }
        }

        for mode in ["drop", "terminate", "exit"] {
            let log_dir = tempfile::tempdir().unwrap();
            let resource = tempfile::tempdir().unwrap();
            let resource_path = resource.path().to_owned();
            let release = log_dir.path().join("release");
            let spec = shell("while [ ! -f \"$1\" ]; do sleep 0.01; done")
                .arg("fixture")
                .arg(release.as_os_str());
            let mut guard = process::spawn(&spec, &log_dir.path().join("log")).unwrap();
            let (sender, receiver) = mpsc::channel();
            guard.retain(resource);
            guard.retain(ReapProbe {
                pid: guard.pid(),
                dropped: sender,
            });
            assert!(guard.try_wait().unwrap().is_none());
            assert!(resource_path.is_dir());
            assert!(matches!(
                receiver.try_recv(),
                Err(mpsc::TryRecvError::Empty)
            ));

            match mode {
                "terminate" => {
                    guard.terminate(Duration::ZERO).unwrap();
                }
                "exit" => {
                    fs::write(&release, "").unwrap();
                    eventually(|| guard.try_wait().unwrap().is_some());
                }
                _ => {}
            }
            assert!(resource_path.is_dir(), "resource released early in {mode}");
            assert!(matches!(
                receiver.try_recv(),
                Err(mpsc::TryRecvError::Empty)
            ));
            drop(guard);
            assert!(
                receiver.recv_timeout(LIMIT).unwrap(),
                "resource released before reap in {mode}"
            );
            assert!(!resource_path.exists());
        }
    }

    #[test]
    fn guard_drop_kills_the_tree() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("log");
        let guard = process::spawn(&shell("sleep 30 & printf '%s\n' $!; wait"), &log).unwrap();
        eventually(|| fs::metadata(&log).is_ok_and(|m| m.len() > 0));
        let mut children = pids(&fs::read(&log).unwrap());
        children.push(guard.pid());
        drop(guard);
        for pid in children {
            eventually(|| dead(pid));
        }
    }

    #[test]
    fn guard_poll_terminate_and_drop_clean_descendants_of_exited_leader() {
        let zombie = |pid: u32| {
            fs::read_to_string(format!("/proc/{pid}/stat"))
                .is_ok_and(|stat| stat.rsplit_once(") ").unwrap().1.starts_with('Z'))
        };
        for mode in ["try_wait", "terminate", "drop"] {
            let dir = tempfile::tempdir().unwrap();
            let log = dir.path().join("log");
            let mut guard =
                process::spawn(&shell("sleep 30 & printf '%s\n' $!; exit 0"), &log).unwrap();
            // Observe the exit without the guard, which would clean up by itself.
            eventually(|| zombie(guard.pid()));
            let children = pids(&fs::read(&log).unwrap());
            assert_eq!(children.len(), 1);
            assert!(!dead(children[0]), "{mode}: descendant died before cleanup");
            match mode {
                "try_wait" => assert!(guard.try_wait().unwrap().unwrap().success()),
                "terminate" => assert_eq!(
                    guard.terminate(LIMIT).unwrap(),
                    TerminateOutcome::AlreadyExited
                ),
                _ => drop(guard),
            }
            // Before any later drop: the operation itself must have cleaned up.
            for pid in children {
                eventually(|| dead(pid));
            }
        }
    }

    #[test]
    fn terminate_kills_descendants_even_if_they_ignore_term() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("log");
        let mut guard = process::spawn(
            &shell("trap '' TERM; sleep 30 & printf '%s\n' $!; wait"),
            &log,
        )
        .unwrap();
        eventually(|| fs::metadata(&log).is_ok_and(|m| m.len() > 0));
        let children = pids(&fs::read(&log).unwrap());
        assert_eq!(
            guard.terminate(Duration::from_millis(50)).unwrap(),
            TerminateOutcome::Killed
        );
        for pid in children {
            eventually(|| dead(pid));
        }
    }
}
