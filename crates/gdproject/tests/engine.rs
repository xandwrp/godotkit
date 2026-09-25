//! Offline acceptance tests; the real engine test is explicitly opt-in.
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use gdproject::config::{EngineSelection, SelectionSource};
use gdproject::engine::{Engine, ProbeCacheHealth, probe, probe_cache_health, probe_key};
use gdproject::workspace::Workspace;
use serde_json::{Value, json};

const DEADLINE: Duration = Duration::from_secs(5);
const FLAGS: &str = "--headless --no-header --editor --path --script --import --quit";

struct Fixture {
    dir: tempfile::TempDir,
    selection: EngineSelection,
    workspace: Workspace,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let executable = dir
            .path()
            .join(if cfg!(windows) { "fake.exe" } else { "fake" });
        copy_engine(Path::new(env!("CARGO_BIN_EXE_fake-godot")), &executable);
        fs::write(dir.path().join("project.godot"), "config_version=5\n").unwrap();
        let workspace = Workspace::open(dir.path()).unwrap();
        Self {
            selection: EngineSelection {
                executable,
                source: SelectionSource::CommandLine,
            },
            workspace,
            dir,
        }
    }
    fn sidecar(&self, suffix: &str) -> PathBuf {
        let mut path = self.selection.executable.as_os_str().to_owned();
        path.push(suffix);
        path.into()
    }
    fn scenario(&self, scenario: Value) {
        fs::write(
            self.sidecar(".scenario.json"),
            serde_json::to_vec(&scenario).unwrap(),
        )
        .unwrap();
    }
    fn calls(&self) -> Vec<Vec<String>> {
        fs::read_to_string(self.sidecar(".log"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
    fn attach(&self) -> gdproject::Result<(Engine, bool)> {
        Engine::attach(&self.selection, &self.workspace, DEADLINE)
    }
    fn cache(&self) -> Value {
        serde_json::from_slice(&fs::read(self.workspace.probe_cache_path()).unwrap()).unwrap()
    }
    fn write_cache(&self, value: Value) {
        fs::write(
            self.workspace.probe_cache_path(),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
    }
}

fn copy_engine(source: &Path, destination: &Path) {
    // Another test's fork can inherit even a CLOEXEC writer until exec, causing
    // ETXTBSY. Only this waited child opens the executable for writing, keeping
    // writable fixture descriptors out of the multithreaded test process.
    #[cfg(unix)]
    {
        let status = std::process::Command::new("cp")
            .arg(source)
            .arg(destination)
            .status()
            .unwrap();
        assert!(status.success(), "fixture executable copy failed: {status}");
    }
    #[cfg(windows)]
    fs::copy(source, destination).unwrap();
}

fn assert_probe_error(error: gdproject::Error, expected: &str) -> String {
    match error {
        gdproject::Error::Probe { message, output } => {
            assert!(
                message.contains(expected),
                "{message:?} does not contain {expected:?}"
            );
            output
        }
        other => panic!("expected probe error, got {other:?}"),
    }
}

#[test]
fn attach_probes_once_then_hits_cache() {
    let mut f = Fixture::new();
    assert_eq!(
        probe_cache_health(&f.selection.executable, &f.workspace),
        ProbeCacheHealth::Missing
    );
    let (first, hit) = f.attach().unwrap();
    assert!(!hit);
    assert_eq!(first.version, "4.7.2.stable.fake");
    assert_eq!(first.fingerprint.len(), 64);
    assert_eq!(
        probe_cache_health(&f.selection.executable, &f.workspace),
        ProbeCacheHealth::Current
    );
    f.selection.source = SelectionSource::Environment;
    let (second, hit) = f.attach().unwrap();
    assert!(hit);
    assert_eq!(second.source, SelectionSource::Environment);
    assert_eq!(first.fingerprint, second.fingerprint);
    let calls = f.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0], ["--help"]);
    assert!(calls[1].iter().any(|a| a == "--headless"));
    let project = Path::new(&calls[1][calls[1].iter().position(|a| a == "--path").unwrap() + 1]);
    assert_ne!(project, f.workspace.root());
    assert!(!project.exists(), "scratch must be removed");
    let script = Path::new(&calls[1][calls[1].iter().position(|a| a == "--script").unwrap() + 1]);
    assert!(!script.exists());
}

#[test]
fn cache_misses_when_engine_size_or_mtime_or_harness_hash_changes() {
    let f = Fixture::new();
    let (original, _) = f.attach().unwrap();
    let file = fs::OpenOptions::new()
        .write(true)
        .open(&f.selection.executable)
        .unwrap();
    file.set_len(file.metadata().unwrap().len() + 1).unwrap();
    drop(file);
    assert_eq!(
        probe_cache_health(&f.selection.executable, &f.workspace),
        ProbeCacheHealth::Stale
    );
    let (resized, hit) = f.attach().unwrap();
    assert!(!hit);
    assert_ne!(original.fingerprint, resized.fingerprint);
    let file = fs::OpenOptions::new()
        .write(true)
        .open(&f.selection.executable)
        .unwrap();
    file.set_modified(SystemTime::now() + Duration::from_secs(10))
        .unwrap();
    drop(file);
    let (retimed, hit) = f.attach().unwrap();
    assert!(!hit);
    assert_ne!(resized.fingerprint, retimed.fingerprint);
    let mut cache = f.cache();
    cache["key"]["harness_hash"] = json!(0);
    f.write_cache(cache);
    assert_eq!(
        probe_cache_health(&f.selection.executable, &f.workspace),
        ProbeCacheHealth::Stale
    );
    assert!(!f.attach().unwrap().1);
    assert_eq!(f.calls().len(), 8);
}

#[test]
#[cfg(unix)]
fn cache_misses_when_engine_is_replaced_with_same_size_and_mtime() {
    let f = Fixture::new();
    let (original, _) = f.attach().unwrap();
    let before = fs::metadata(&f.selection.executable).unwrap();
    // `cp -p` / archive extraction: identical bytes, size and mtime, new file.
    let replacement = f.dir.path().join("replacement");
    copy_engine(&f.selection.executable, &replacement);
    fs::File::open(&replacement)
        .unwrap()
        .set_modified(before.modified().unwrap())
        .unwrap();
    fs::rename(&replacement, &f.selection.executable).unwrap();
    let after = fs::metadata(&f.selection.executable).unwrap();
    assert_eq!(
        (after.len(), after.modified().unwrap()),
        (before.len(), before.modified().unwrap())
    );
    assert_eq!(
        probe_cache_health(&f.selection.executable, &f.workspace),
        ProbeCacheHealth::Stale
    );
    let (replaced, hit) = f.attach().unwrap();
    assert!(!hit);
    assert_ne!(original.fingerprint, replaced.fingerprint);
}

#[test]
fn cache_is_not_written_when_probe_fails_or_engine_changes_mid_probe() {
    let f = Fixture::new();
    f.scenario(json!({"probe": {"mode": "error_envelope", "stderr": "resource failed\n"}}));
    assert!(f.attach().is_err());
    assert!(!f.workspace.probe_cache_path().exists());
    f.scenario(json!({"probe": {"delay_ms": 400}}));
    fs::remove_file(f.sidecar(".log")).unwrap();
    std::thread::scope(|scope| {
        let attach = scope.spawn(|| f.attach());
        let start = Instant::now();
        while f.calls().len() < 2 {
            assert!(start.elapsed() < DEADLINE);
            std::thread::sleep(Duration::from_millis(5));
        }
        // Metadata can change even while the executable is running (unlike its bytes).
        fs::File::open(&f.selection.executable)
            .unwrap()
            .set_modified(SystemTime::now() + Duration::from_secs(20))
            .unwrap();
        assert_probe_error(attach.join().unwrap().unwrap_err(), "changed");
    });
    assert!(!f.workspace.probe_cache_path().exists());
}

#[test]
fn probe_rejects_engines_missing_headless_editor_flags() {
    for missing in FLAGS.split_whitespace() {
        let f = Fixture::new();
        let help = FLAGS.replace(missing, &format!("{missing}-not-the-flag"));
        f.scenario(json!({"help": {"stdout": help}}));
        assert_probe_error(
            probe(&f.selection.executable, DEADLINE).unwrap_err(),
            missing,
        );
        assert_eq!(f.calls().len(), 1);
    }
}

#[test]
fn probe_accepts_colored_help_on_either_stream() {
    for stream in ["stdout", "stderr"] {
        let f = Fixture::new();
        let colored = FLAGS
            .split_whitespace()
            .map(|flag| format!("\u{1b}[92m{flag}\u{1b}[0m\n"))
            .collect::<String>();
        let mut help = json!({"stdout":""});
        help[stream] = json!(colored);
        f.scenario(json!({"help":help}));
        assert!(f.attach().is_ok());
    }
}

#[test]
fn probe_rejects_non_editor_or_non_4x_builds() {
    for payload in [
        json!({"version":"3.6", "major":3, "editor":true}),
        json!({"version":"4.7", "major":4, "editor":false}),
        json!({"version":"", "major":4, "editor":true}),
    ] {
        let f = Fixture::new();
        f.scenario(json!({"probe": {"payload": payload}}));
        assert_probe_error(f.attach().unwrap_err(), "Godot 4 editor");
        assert!(!f.workspace.probe_cache_path().exists());
    }
}

#[test]
fn probe_respects_deadline() {
    // Deadlines leave a loaded machine ample time to spawn the fake and flush its
    // output; a hanging stage never finishes, so any return proves the deadline.
    for stage in ["help", "probe"] {
        let f = Fixture::new();
        f.scenario(json!({stage: {"mode":"hang", "stdout":"partial stdout\n", "stderr":"partial stderr\n"}}));
        let deadline = Duration::from_millis(1500);
        let start = Instant::now();
        let output = assert_probe_error(
            probe(&f.selection.executable, deadline).unwrap_err(),
            "deadline",
        );
        let elapsed = start.elapsed();
        assert!(
            elapsed >= deadline && elapsed < deadline + Duration::from_secs(5),
            "{elapsed:?}"
        );
        assert!(output.contains("partial stdout"));
        assert!(output.contains("partial stderr"));
    }
    // One budget spans both stages. Help finishes well within it; the probe stage
    // gets only the remainder. A per-stage deadline would return after
    // help + deadline (3.5 s) and no deadline after help + probe (6 s), so the
    // bound keeps ~0.9 s of scheduling slack while still telling them apart.
    let f = Fixture::new();
    f.scenario(json!({"help":{"delay_ms":1000}, "probe":{"delay_ms":5000}}));
    let deadline = Duration::from_millis(2500);
    let start = Instant::now();
    assert_probe_error(
        probe(&f.selection.executable, deadline).unwrap_err(),
        "deadline",
    );
    let elapsed = start.elapsed();
    assert!(
        elapsed >= deadline && elapsed < Duration::from_millis(3400),
        "{elapsed:?}"
    );
    assert_eq!(f.calls().len(), 2);
    let f = Fixture::new();
    assert_probe_error(
        probe(&f.selection.executable, Duration::ZERO).unwrap_err(),
        "deadline",
    );
    assert!(f.calls().is_empty());
}

#[test]
fn console_launcher_tracks_companion_exe_on_windows() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["Godot.console.exe", "Godot_console.exe"] {
        let launcher = dir.path().join(name);
        let companion = dir.path().join("Godot.exe");
        fs::write(&launcher, "launcher").unwrap();
        fs::write(&companion, "engine").unwrap();
        let before = probe_key(&launcher).unwrap();
        fs::write(&companion, "new engine").unwrap();
        let after = probe_key(&launcher).unwrap();
        if cfg!(windows) {
            assert_eq!(before.files.len(), 2);
            assert_ne!(before, after);
            fs::remove_file(companion).unwrap();
            assert!(probe_key(&launcher).is_err());
        } else {
            assert_eq!(before.files.len(), 1);
            assert_eq!(before, after);
        }
    }
}

#[test]
fn fingerprint_is_stable_for_identical_key_and_differs_otherwise() {
    let f = Fixture::new();
    assert_eq!(
        probe_key(&f.selection.executable).unwrap(),
        probe_key(&f.selection.executable).unwrap()
    );
    let first = Engine::attach_standalone(&f.selection, DEADLINE).unwrap();
    let second = Engine::attach_standalone(&f.selection, DEADLINE).unwrap();
    assert_eq!(first, second);
    let key = probe_key(&f.selection.executable).unwrap();
    assert_eq!(
        first.fingerprint,
        blake3::hash(&serde_json::to_vec(&key).unwrap())
            .to_hex()
            .to_string()
    );
    let other = f.dir.path().join("other-engine");
    copy_engine(&f.selection.executable, &other);
    let other = EngineSelection {
        executable: other,
        source: SelectionSource::ProjectConfig,
    };
    assert_ne!(
        first.fingerprint,
        Engine::attach_standalone(&other, DEADLINE)
            .unwrap()
            .fingerprint
    );
    assert!(!f.workspace.state_dir().exists());
}

#[test]
fn cache_health_and_recovery() {
    let f = Fixture::new();
    f.attach().unwrap();
    fs::write(f.workspace.probe_cache_path(), b"not json").unwrap();
    assert_eq!(
        probe_cache_health(&f.selection.executable, &f.workspace),
        ProbeCacheHealth::Malformed
    );
    assert!(!f.attach().unwrap().1);
    let mut invalid = f.cache();
    invalid["report"]["editor"] = json!(false);
    f.write_cache(invalid);
    assert_eq!(
        probe_cache_health(&f.selection.executable, &f.workspace),
        ProbeCacheHealth::Malformed
    );
    assert!(!f.attach().unwrap().1);
    fs::remove_file(f.workspace.probe_cache_path()).unwrap();
    fs::create_dir(f.workspace.probe_cache_path()).unwrap();
    assert_eq!(
        probe_cache_health(&f.selection.executable, &f.workspace),
        ProbeCacheHealth::Unreadable
    );
}

#[test]
fn failures_retain_captured_output_and_never_cache() {
    for scenario in [
        json!({"mode":"no_envelope"}),
        json!({"mode":"error_envelope"}),
        json!({"exit":7}),
        json!({"payload":null}),
        json!({"payload":{"version":42}}),
        json!({"mode":"crash"}),
    ] {
        let f = Fixture::new();
        let mut scenario = scenario;
        scenario["stdout"] = json!("probe stdout\n");
        scenario["stderr"] = json!("probe stderr\n");
        f.scenario(json!({"probe":scenario}));
        let output = assert_probe_error(f.attach().unwrap_err(), "");
        assert!(output.contains("probe stdout"));
        assert!(output.contains("probe stderr"));
        assert!(!f.workspace.probe_cache_path().exists());
    }
    let f = Fixture::new();
    f.scenario(json!({"help":{"exit":1, "stdout":FLAGS, "stderr":"bad help"}}));
    assert!(assert_probe_error(f.attach().unwrap_err(), "--help").contains("bad help"));
    assert_eq!(f.calls().len(), 1);
}

#[test]
#[ignore = "opt-in: runs GDKIT_TEST_GODOT or /usr/bin/godot"]
fn real_engine_standalone_probe_loads_resource() {
    let executable = std::env::var_os("GDKIT_TEST_GODOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/usr/bin/godot"));
    let selection = EngineSelection {
        executable: fs::canonicalize(executable).unwrap(),
        source: SelectionSource::CommandLine,
    };
    let engine = Engine::attach_standalone(&selection, Duration::from_secs(60)).unwrap();
    assert!(engine.version.starts_with("4."));
    assert_eq!(engine.fingerprint.len(), 64);
}
