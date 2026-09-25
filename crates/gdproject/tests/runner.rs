// Offline shell fixtures keep these contracts independent of fake-engine rollout.
#![cfg(unix)]

use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use gdproject::config::SelectionSource;
use gdproject::protocol::ProtocolError;
use gdproject::runner::{self, Harness, Invocation};
use gdproject::{Engine, Error};
use serde_json::{Value, json};

struct Fixture {
    dir: tempfile::TempDir,
    engine: Engine,
}

impl Fixture {
    fn new(body: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("engine with spaces");
        // Record every argument and check both embedded files before running.
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$ARGS\"\nprevious=''\nfor arg do\n  if [ \"$previous\" = '--script' ]; then\n    printf '%s' \"$arg\" > \"$SCRIPT_PATH\"\n    test -s \"$arg\" || exit 90\n    test -s \"${{arg%/*}}/protocol.gd\" || exit 91\n    cp \"$arg\" \"$HARNESS_COPY\"\n    cp \"${{arg%/*}}/protocol.gd\" \"$PROTOCOL_COPY\"\n  fi\n  previous=\"$arg\"\ndone\n{body}\n"
        );
        // Keep writable executable descriptors out of this multithreaded process:
        // another test's fork can inherit even a CLOEXEC descriptor until exec,
        // briefly causing ETXTBSY when this fixture is launched. Only this waited
        // child opens the executable for writing; the tests still run in parallel.
        let status = Command::new("/bin/sh")
            .args([
                "-c",
                "printf '%s' \"$1\" > \"$2\" && chmod 700 \"$2\"",
                "fixture-writer",
            ])
            .arg(script)
            .arg(&executable)
            .status()
            .unwrap();
        assert!(status.success());
        Self {
            engine: Engine {
                executable,
                version: "fixture".into(),
                fingerprint: "fixture".into(),
                source: SelectionSource::CommandLine,
            },
            dir,
        }
    }

    fn invocation(&self) -> Invocation<'_> {
        let mut invocation = Invocation::new(&self.engine, self.dir.path(), Duration::from_secs(2));
        for (key, file) in [
            ("ARGS", "args"),
            ("SCRIPT_PATH", "script-path"),
            ("HARNESS_COPY", "harness-copy"),
            ("PROTOCOL_COPY", "protocol-copy"),
        ] {
            invocation
                .env
                .push((key.into(), self.dir.path().join(file).into_os_string()));
        }
        invocation
    }

    fn args(&self) -> Vec<String> {
        fs::read_to_string(self.dir.path().join("args"))
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn script(&self) -> PathBuf {
        fs::read_to_string(self.dir.path().join("script-path"))
            .unwrap()
            .into()
    }
}

fn envelope(harness: Harness) -> String {
    format!(
        "printf '%s\\n' '{}'",
        format!(
            "GDKIT_RESULT:{}",
            json!({
                "protocol": 1, "harness": harness.name(), "ok": true, "payload": {"count": 3}
            })
        )
    )
}

#[test]
fn run_harness_writes_harness_and_protocol_to_a_temp_dir_and_passes_user_args_after_double_dash() {
    let fixture = Fixture::new(&envelope(Harness::Check));
    let mut invocation = fixture.invocation();
    invocation.user_args = vec!["res://with spaces.gd".into(), "--editor".into()];
    runner::run_harness::<Value>(&invocation, Harness::Check).unwrap();
    let args = fixture.args();
    let separator = args.iter().position(|arg| arg == "--").unwrap();
    assert_eq!(&args[separator + 1..], ["res://with spaces.gd", "--editor"]);
    assert_eq!(args[0], "--headless");
    assert!(args.contains(&"--no-header".into()));
    let path = args.iter().position(|arg| arg == "--path").unwrap();
    assert_eq!(PathBuf::from(&args[path + 1]), fixture.dir.path());
    assert_eq!(
        fs::read_to_string(fixture.dir.path().join("harness-copy")).unwrap(),
        Harness::Check.source()
    );
    assert_eq!(
        fs::read_to_string(fixture.dir.path().join("protocol-copy")).unwrap(),
        runner::PROTOCOL_SOURCE
    );
    assert!(!fixture.script().parent().unwrap().exists());
}

#[test]
fn run_harness_adds_editor_flag_only_when_requested() {
    for harness in [Harness::Check, Harness::ImportScan] {
        let fixture = Fixture::new(&envelope(harness));
        runner::run_harness::<Value>(&fixture.invocation(), harness).unwrap();
        assert_eq!(
            fixture.args().contains(&"--editor".into()),
            harness.needs_editor()
        );
        let mut invocation = fixture.invocation();
        invocation.engine_args.push("--editor".into());
        runner::run_harness::<Value>(&invocation, harness).unwrap();
        assert_eq!(
            fixture
                .args()
                .iter()
                .filter(|arg| *arg == "--editor")
                .count(),
            1
        );
    }
}

#[test]
fn run_harness_decodes_envelope_and_attaches_diagnostics_and_captured_output() {
    let fixture = Fixture::new(&format!(
        "{}\nprintf 'ERROR: after completion\\n'\nprintf 'SCRIPT ERROR: runtime failure\\n' >&2",
        envelope(Harness::Check)
    ));
    let run = runner::run_harness::<Value>(&fixture.invocation(), Harness::Check).unwrap();
    assert_eq!(run.envelope.payload.unwrap()["count"], 3);
    assert!(run.captured.success());
    assert_eq!(run.diagnostics.len(), 2);
    assert!(String::from_utf8_lossy(&run.captured.stderr()).contains("runtime failure"));
}

#[test]
fn run_harness_maps_error_envelope_to_error_harness_with_stage() {
    let fixture = Fixture::new(
        "printf '%s\\n' 'GDKIT_RESULT:{\"protocol\":1,\"harness\":\"check\",\"ok\":false,\"error\":{\"stage\":\"load\",\"message\":\"broken\"}}'\nexit 1",
    );
    let run = runner::run_harness_captured::<Value>(&fixture.invocation(), Harness::Check).unwrap();
    assert!(
        matches!(run.envelope, Err(Error::Harness { stage, message, .. }) if stage == "load" && message == "broken")
    );
    assert!(!run.captured.stdout().is_empty());
    assert_eq!(run.captured.status.unwrap().code(), Some(1));
    assert!(matches!(
        runner::run_harness::<Value>(&fixture.invocation(), Harness::Check),
        Err(Error::Harness { .. })
    ));
}

#[test]
fn run_harness_maps_missing_envelope_to_error_protocol() {
    let fixture = Fixture::new("printf 'noise\\n'\nprintf 'ERROR: still retained\\n' >&2");
    let run = runner::run_harness_captured::<Value>(&fixture.invocation(), Harness::Check).unwrap();
    assert!(matches!(
        run.envelope,
        Err(Error::Protocol {
            source: ProtocolError::Missing,
            ..
        })
    ));
    assert_eq!(run.captured.stdout(), b"noise\n");
    assert_eq!(run.diagnostics.len(), 1);
}

#[test]
fn run_harness_enforces_deadline_and_reports_timeout_with_partial_output() {
    let fixture =
        Fixture::new("printf 'partial'; printf 'SCRIPT ERROR: before timeout\\n' >&2; sleep 30");
    let mut invocation = fixture.invocation();
    invocation.deadline = Duration::from_millis(150);
    let started = Instant::now();
    let run = runner::run_harness_captured::<Value>(&invocation, Harness::Check).unwrap();
    assert!(matches!(run.envelope, Err(Error::Timeout { .. })));
    assert!(run.captured.timed_out);
    assert_eq!(run.captured.stdout(), b"partial");
    assert_eq!(run.diagnostics.len(), 1);
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(!fixture.script().exists());
}

#[test]
fn run_engine_is_the_raw_form_used_for_import_dump_and_run() {
    let fixture = Fixture::new("printf 'raw\\n'; printf 'ERROR: failure\\n' >&2; exit 7");
    let mut invocation = fixture.invocation();
    invocation.engine_args = vec!["--editor".into(), "--import".into()];
    invocation.user_args.push("user argument".into());
    let (captured, diagnostics) = runner::run_engine(&invocation).unwrap();
    assert_eq!(captured.status.unwrap().code(), Some(7));
    assert_eq!(captured.stdout(), b"raw\n");
    assert_eq!(diagnostics.len(), 1);
    assert!(!fixture.args().contains(&"--script".into()));
    assert!(fixture.args().contains(&"--import".into()));
}

#[derive(Debug)]
struct IntentionalUnwind;

fn assert_intentional_unwind(result: std::thread::Result<()>) {
    let panic = result.expect_err("expected deliberate unwind");
    if !panic.is::<IntentionalUnwind>() {
        std::panic::resume_unwind(panic);
    }
}

#[test]
fn temp_files_are_removed_after_every_outcome_including_panic() {
    for body in [
        envelope(Harness::Check),
        "exit 4".into(),
        "printf 'GDKIT_RESULT:bad\\n'".into(),
        "sleep 30".into(),
    ] {
        let fixture = Fixture::new(&body);
        let mut invocation = fixture.invocation();
        invocation.deadline = Duration::from_millis(150);
        let panic = std::panic::catch_unwind(|| {
            let _run = runner::run_harness_captured::<Value>(&invocation, Harness::Check).unwrap();
            std::panic::panic_any(IntentionalUnwind);
        });
        assert_intentional_unwind(panic);
        assert!(!fixture.script().parent().unwrap().exists());
    }
}

#[test]
fn protocol_failures_and_unsuccessful_exit_retain_capture() {
    for body in [
        "printf 'GDKIT_RESULT:bad\\n'".into(),
        format!("{}\n{}", envelope(Harness::Check), envelope(Harness::Check)),
        envelope(Harness::Probe),
        format!("{}\nexit 7", envelope(Harness::Check)),
        format!("{}\nkill -KILL $$", envelope(Harness::Check)),
    ] {
        let fixture = Fixture::new(&body);
        let run =
            runner::run_harness_captured::<Value>(&fixture.invocation(), Harness::Check).unwrap();
        assert!(run.envelope.is_err());
        assert!(!run.captured.stdout().is_empty());
    }
    let fixture = Fixture::new(&envelope(Harness::Check));
    let run =
        runner::run_harness_captured::<Vec<String>>(&fixture.invocation(), Harness::Check).unwrap();
    assert!(matches!(
        run.envelope,
        Err(Error::Protocol {
            source: ProtocolError::Payload(_),
            ..
        })
    ));
    assert!(!run.captured.stdout().is_empty());
}

#[test]
fn output_limit_is_rejected_before_protocol_decoding_and_retains_capture() {
    // Exercise both a valid success envelope and an undecodable prefix. Neither
    // can turn incomplete output into success or mask the explicit limit error.
    for prefix in [
        envelope(Harness::Check),
        "printf 'GDKIT_RESULT:bad\\n'".into(),
    ] {
        let fixture = Fixture::new(&format!(
            "{prefix}\nprintf 'ERROR: retained before limit\\n'\nawk 'BEGIN {{ for (i = 0; i <= {}; i++) print \"noise\" }}'",
            gdproject::process::MAX_CAPTURE_LINES,
        ));
        let run =
            runner::run_harness_captured::<Value>(&fixture.invocation(), Harness::Check).unwrap();
        assert!(run.captured.output_limit_exceeded);
        assert!(!run.captured.timed_out);
        assert!(matches!(run.envelope, Err(Error::Invalid(message))
            if message.contains("check") && message.contains("output capture limit") && message.contains("incomplete")));
        assert!(run.captured.stdout().starts_with(b"GDKIT_RESULT:"));
        assert_eq!(run.diagnostics.len(), 1);
        assert!(run.captured.lines.len() <= gdproject::process::MAX_CAPTURE_LINES);
        assert!(!fixture.script().parent().unwrap().exists());
        assert!(matches!(
            runner::run_harness::<Value>(&fixture.invocation(), Harness::Check),
            Err(Error::Invalid(_))
        ));

        let (captured, diagnostics) =
            runner::run_harness_raw(&fixture.invocation(), Harness::Check).unwrap();
        assert!(captured.output_limit_exceeded);
        assert_eq!(diagnostics.len(), 1);
    }
}

#[test]
fn script_bootstrap_raw_capture_does_not_require_a_success_envelope() {
    for (body, exit, timeout) in [
        ("printf 'GDKIT_SCRIPT_STARTED\\n'", Some(0), false),
        ("printf 'GDKIT_SCRIPT_STARTED\\n'; exit 7", Some(7), false),
        (
            "printf 'GDKIT_SCRIPT_STARTED\\n'; printf 'SCRIPT ERROR: user failure\\n' >&2; sleep 30",
            None,
            true,
        ),
    ] {
        let fixture = Fixture::new(body);
        let mut invocation = fixture.invocation();
        invocation.deadline = Duration::from_millis(150);
        let (captured, diagnostics) =
            runner::run_harness_raw(&invocation, Harness::ScriptBootstrap).unwrap();
        assert_eq!(captured.stdout(), b"GDKIT_SCRIPT_STARTED\n");
        assert_eq!(captured.timed_out, timeout);
        assert_eq!(captured.status.unwrap().code(), exit);
        assert_eq!(diagnostics.len(), usize::from(timeout));
    }
}

#[test]
fn game_guard_keeps_embedded_files_until_drop_including_unwind() {
    for unwind in [false, true] {
        let fixture = Fixture::new("sleep 30");
        let result = std::panic::catch_unwind(|| {
            let mut invocation = fixture.invocation();
            invocation.user_args.push(OsString::from("game argument"));
            let guard =
                runner::spawn_game(&invocation, &fixture.dir.path().join("game.log")).unwrap();
            let started = Instant::now();
            while !fixture.dir.path().join("script-path").exists()
                || !fixture.dir.path().join("protocol-copy").exists()
            {
                assert!(started.elapsed() < Duration::from_secs(2));
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(fixture.script().exists());
            assert!(
                fixture
                    .script()
                    .parent()
                    .unwrap()
                    .join("protocol.gd")
                    .exists()
            );
            assert!(!fixture.args().contains(&"--headless".into()));
            if unwind {
                std::panic::panic_any(IntentionalUnwind);
            }
            drop(guard);
        });
        if unwind {
            assert_intentional_unwind(result);
        } else if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
        assert!(!fixture.script().parent().unwrap().exists());
    }
}

#[test]
fn harness_hash_is_repeatable_and_covers_every_embedded_source() {
    let mut hash = blake3::Hasher::new();
    hash.update(runner::PROTOCOL_SOURCE.as_bytes());
    for harness in [
        Harness::Probe,
        Harness::Check,
        Harness::ImportScan,
        Harness::ResourceSchema,
        Harness::ResourceCreate,
        Harness::RuntimeProbe,
        Harness::ScriptBootstrap,
    ] {
        assert!(!harness.source().is_empty());
        hash.update(harness.name().as_bytes());
        hash.update(&(harness.source().len() as u64).to_le_bytes());
        hash.update(harness.source().as_bytes());
    }
    let expected = u64::from_le_bytes(hash.finalize().as_bytes()[..8].try_into().unwrap());
    assert_eq!(runner::harness_hash(), expected);
    assert_eq!(runner::harness_hash(), runner::harness_hash());
}
