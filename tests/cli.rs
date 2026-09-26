// Drives the built binary. Engine tests explicitly build gdproject's fake-godot
// once per test process into a dedicated target directory under
// CARGO_TARGET_TMPDIR (offline, with a three-minute deadline). That directory is
// reused across runs, so nothing leaks into /tmp and rebuilds are incremental.
// This works for both `cargo test -p gdkit` and `cargo test --workspace`, without
// relying on Cargo building dependency binaries or a pre-existing sibling binary.
// Every test copies the executable and its scenario; no process-global env changes.
#![allow(unused)]

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use clap::CommandFactory;

// The status table and the clap tree it describes, compiled into this test so
// the drift test can walk every command without a hand-kept list.
#[path = "../src/cli.rs"]
mod cli;
#[path = "../src/status.rs"]
mod status;

/// Runs the binary with an engine variable that points nowhere, so any
/// accidental engine use fails loudly.
fn gdkit(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .args(args)
        .env("GDKIT_GODOT", "/nonexistent/godot")
        .output()
        .unwrap()
}

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("project.godot"), "config_version=5\n").unwrap();
    for (path, contents) in files {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    dir
}

const MISSING_PRELOAD: (&str, &str) = (
    "scripts/a.gd",
    "extends Node\nconst X = preload(\"res://gone.tscn\")\n",
);

#[test]
#[ignore = "scaffold"]
fn no_arguments_prints_help_and_exits_zero() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn every_project_command_accepts_project_godot_and_output_flags_uniformly() {
    todo!()
}

#[test]
fn json_mode_writes_exactly_one_json_document_to_stdout_and_nothing_else() {
    let dir = project(&[MISSING_PRELOAD]);
    let root = dir.path().to_str().unwrap();
    let output = gdkit(&[
        "check",
        "--static-only",
        "--project",
        root,
        "--output",
        "json",
    ]);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let mut documents =
        serde_json::Deserializer::from_str(&stdout).into_iter::<serde_json::Value>();
    let report = documents.next().unwrap().unwrap();
    assert!(
        documents.next().is_none(),
        "more than one JSON document on stdout"
    );
    assert_eq!(report["outcome"], "failed");
    assert_eq!(
        report["phases"][0]["diagnostics"][0]["resource"],
        "res://scripts/a.gd"
    );
    assert!(
        output.stderr.is_empty(),
        "JSON mode printed progress: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn tool_errors_exit_2_with_error_prefix_on_stderr() {
    let outside = tempfile::tempdir().unwrap();
    let dir = project(&[]);
    let root = dir.path().to_str().unwrap();
    for args in [
        vec![
            "check",
            "--static-only",
            "--project",
            outside.path().to_str().unwrap(),
        ],
        vec![
            "check",
            "--static-only",
            "--project",
            root,
            "--slice",
            "../escape",
        ],
        vec![
            "check",
            "--static-only",
            "--project",
            root,
            "--baseline",
            "/nonexistent/report.json",
        ],
        vec!["check", "--project", root],
        vec![
            "check",
            "--static-only",
            "--project",
            root,
            "--output",
            "json",
            "--slice",
            "../escape",
        ],
    ] {
        let output = gdkit(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.starts_with("error: "), "{args:?}: {stderr}");
        assert!(
            output.stdout.is_empty(),
            "{args:?}: tool errors must not print a report"
        );
    }
}

#[test]
fn check_exit_code_follows_report_outcome() {
    let dir = project(&[MISSING_PRELOAD, ("scripts/ok.gd", "extends Node\n")]);
    let root = dir.path().to_str().unwrap();
    let failed = gdkit(&["check", "--static-only", "--project", root]);
    assert_eq!(failed.status.code(), Some(1));
    let stdout = String::from_utf8(failed.stdout).unwrap();
    assert!(
        stdout.contains(
            "res://scripts/a.gd:2: error: preload of res://gone.tscn, which does not exist"
        ),
        "{stdout}"
    );
    assert!(stdout.contains("check FAILED"), "{stdout}");
    // Discovery walks up from any path inside the project.
    let passed = gdkit(&[
        "check",
        "--static-only",
        "--project",
        &format!("{root}/scripts"),
        "--slice",
        "scripts/ok.gd",
    ]);
    assert_eq!(
        passed.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&passed.stdout)
    );
    assert!(
        String::from_utf8(passed.stdout)
            .unwrap()
            .contains("check passed")
    );
}

#[test]
fn static_check_baseline_file_isolates_new_findings() {
    let dir = project(&[MISSING_PRELOAD]);
    let root = dir.path().to_str().unwrap();
    let before = gdkit(&[
        "check",
        "--static-only",
        "--project",
        root,
        "--output",
        "json",
    ]);
    let baseline = dir.path().join("baseline.json");
    fs::write(&baseline, &before.stdout).unwrap();
    fs::write(
        dir.path().join("scripts/b.gd"),
        "extends Node\nconst Y = load(\"res://also_gone.tres\")\n",
    )
    .unwrap();
    let after = gdkit(&[
        "check",
        "--static-only",
        "--project",
        root,
        "--output",
        "json",
        "--baseline",
        baseline.to_str().unwrap(),
    ]);
    assert_eq!(after.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&after.stdout).unwrap();
    let new = report["baseline"]["new"].as_array().unwrap();
    assert_eq!(new.len(), 1);
    assert_eq!(new[0]["resource"], "res://scripts/b.gd");
    assert_eq!(report["baseline"]["carried"].as_array().unwrap().len(), 1);
}

#[test]
#[ignore = "scaffold"]
fn scene_tree_autoloads_refs_settings_net_and_static_check_need_no_engine() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn init_writes_config_and_refuses_to_overwrite() {
    todo!()
}

/// Path of the fake engine executable. A failed build is cached, so every
/// later caller fails fast with the same message instead of rebuilding.
fn fake_binary() -> &'static Path {
    static BUILD: std::sync::OnceLock<Result<std::path::PathBuf, String>> =
        std::sync::OnceLock::new();
    match BUILD.get_or_init(build_fake_binary) {
        Ok(path) => path,
        Err(message) => panic!("{message}"),
    }
}

fn build_fake_binary() -> Result<std::path::PathBuf, String> {
    let target = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fake-godot-target");
    fs::create_dir_all(&target).map_err(|e| format!("{}: {e}", target.display()))?;
    let messages = target.join("build.json");
    let log = target.join("build.log");
    let file = |path: &Path| fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()));
    let mut child = Command::new(env!("CARGO"))
        .args(["build", "--offline", "--locked", "--manifest-path"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/gdproject/Cargo.toml"))
        .args([
            "--features",
            "test-engine",
            "--bin",
            "fake-godot",
            "--message-format",
            "json",
            "--target-dir",
        ])
        .arg(&target)
        // An inherited cross target would move the output and build for the wrong host.
        .env_remove("CARGO_BUILD_TARGET")
        .stdout(file(&messages)?)
        .stderr(file(&log)?)
        .spawn()
        .map_err(|e| format!("spawn fake engine build: {e}"))?;
    let started = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if started.elapsed() > std::time::Duration::from_secs(180) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("fake engine build exceeded 180 seconds".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    let log = fs::read_to_string(&log).unwrap_or_default();
    if !status.success() {
        return Err(format!("fake engine build failed ({status}): {log}"));
    }
    // Ask Cargo where it put the binary rather than assuming a profile layout.
    fs::read_to_string(&messages)
        .map_err(|e| e.to_string())?
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|m| m["reason"] == "compiler-artifact" && m["target"]["name"] == "fake-godot")
        .find_map(|m| m["executable"].as_str().map(std::path::PathBuf::from))
        .ok_or_else(|| format!("fake engine build reported no executable: {log}"))
}

fn copy_executable(source: &Path, destination: &Path) {
    // A concurrent test's fork can inherit a writable descriptor even with
    // CLOEXEC, causing ETXTBSY until exec. Only this waited child opens the
    // executable for writing, keeping that descriptor out of the test process.
    #[cfg(unix)]
    {
        let status = Command::new("cp")
            .arg(source)
            .arg(destination)
            .status()
            .expect("spawn executable fixture copy");
        assert!(status.success(), "executable fixture copy failed: {status}");
    }
    #[cfg(not(unix))]
    fs::copy(source, destination).expect("copy executable fixture");
}

struct Fake {
    dir: tempfile::TempDir,
    executable: std::path::PathBuf,
}

impl Fake {
    fn new(scenario: serde_json::Value) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let executable = dir
            .path()
            .join(format!("godot{}", std::env::consts::EXE_SUFFIX));
        copy_executable(fake_binary(), &executable);
        fs::write(
            format!("{}.scenario.json", executable.display()),
            scenario.to_string(),
        )
        .unwrap();
        Self { dir, executable }
    }

    fn command(&self, root: &Path) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_gdkit"));
        command
            .args(["check", "--project"])
            .arg(root)
            .env("GDKIT_GODOT", &self.executable)
            .env_remove("FAKE_GODOT_SCENARIO")
            .env_remove("FAKE_GODOT_LOG");
        command
    }

    fn log(&self) -> String {
        fs::read_to_string(format!("{}.log", self.executable.display())).unwrap()
    }

    /// The log since the last call, so assertions see only the latest runs.
    fn take_log(&self) -> String {
        let path = format!("{}.log", self.executable.display());
        let log = fs::read_to_string(&path).unwrap_or_default();
        let _ = fs::remove_file(&path);
        log
    }
}

fn report(output: &Output, code: i32, outcome: &str) -> serde_json::Value {
    assert_eq!(
        output.status.code(),
        Some(code),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // from_slice also rejects trailing non-whitespace / a second JSON document.
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["outcome"], outcome);
    report
}

#[test]
fn engine_check_passes_with_repeated_scripts_and_preserves_artifacts() {
    let fake = Fake::new(serde_json::json!({}));
    let dir = project(&[
        ("one.gd", "extends SceneTree\n"),
        ("two.gd", "extends SceneTree\n"),
    ]);
    let output = fake
        .command(dir.path())
        .args([
            "--output",
            "json",
            "--verbose",
            "--script",
            "res://two.gd",
            "--script",
            "res://one.gd",
            "--script",
            "res://two.gd",
            "--script-timeout",
            "2",
            "--phase-timeout",
            "3",
        ])
        .output()
        .unwrap();
    let value = report(&output, 0, "passed");
    assert_eq!(value["counts"]["project_scripts_run"], 3);
    let phases: Vec<_> = value["phases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["id"]["kind"] == "project_script")
        .collect();
    assert_eq!(phases.len(), 3);
    assert!(
        phases[0]["id"]["id"]
            .as_str()
            .unwrap()
            .ends_with("res://two.gd")
    );
    assert!(
        phases[1]["id"]["id"]
            .as_str()
            .unwrap()
            .ends_with("res://one.gd")
    );
    assert!(
        phases[2]["id"]["id"]
            .as_str()
            .unwrap()
            .ends_with("res://two.gd")
    );
    assert!(Path::new(value["artifact_dir"].as_str().unwrap()).is_dir());
    for phase in value["phases"].as_array().unwrap() {
        for path in phase["artifacts"].as_array().unwrap() {
            assert!(Path::new(path.as_str().unwrap()).is_file());
        }
    }
}

#[test]
fn engine_selection_and_strict_policy_follow_flag_env_config_precedence() {
    let fake = Fake::new(serde_json::json!({}));
    let dir = project(&[]);
    // A config-relative executable must resolve against the project, not cwd.
    let name = fake.executable.file_name().unwrap();
    let local = dir.path().join(name);
    copy_executable(&fake.executable, &local);
    fs::write(
        dir.path().join("gdkit.toml"),
        format!(
            "[engine]\nexecutable = {:?}\n[check]\nstrict_methods = true\n",
            format!("./{}", name.to_str().unwrap())
        ),
    )
    .unwrap();
    let mut command = fake.command(dir.path());
    command.env_remove("GDKIT_GODOT");
    let value = report(
        &command.args(["--output", "json"]).output().unwrap(),
        0,
        "passed",
    );
    let canonical = |path: &serde_json::Value| fs::canonicalize(path.as_str().unwrap()).unwrap();
    assert_eq!(
        canonical(&value["engine"]["executable"]),
        fs::canonicalize(&local).unwrap()
    );
    assert_eq!(value["policy"]["strict_methods"], true);
    let value = report(
        &fake
            .command(dir.path())
            .args(["--output", "json"])
            .output()
            .unwrap(),
        0,
        "passed",
    );
    assert_eq!(
        canonical(&value["engine"]["executable"]),
        fs::canonicalize(&fake.executable).unwrap()
    );
    fs::write(
        dir.path().join("gdkit.toml"),
        "[check]\nstrict_methods = false\n",
    )
    .unwrap();
    // Earlier runs already logged strict-methods; judge only the next run.
    fake.take_log();
    let value = report(
        &fake
            .command(dir.path())
            .env("GDKIT_GODOT", "/nonexistent/godot")
            .arg("--godot")
            .arg(&fake.executable)
            .args(["--output", "json", "--strict-methods"])
            .output()
            .unwrap(),
        0,
        "passed",
    );
    assert_eq!(value["policy"]["strict_methods"], true);
    let log = fake.take_log();
    assert!(log.contains("\"strict-methods\""), "{log}");
    assert!(!log.contains("\"project-policy\""), "{log}");
    let value = report(
        &fake
            .command(dir.path())
            .args(["--output", "json"])
            .output()
            .unwrap(),
        0,
        "passed",
    );
    assert_eq!(value["policy"]["strict_methods"], false);
    let log = fake.take_log();
    assert!(log.contains("\"project-policy\""), "{log}");
    assert!(!log.contains("\"strict-methods\""), "{log}");
}

#[test]
fn engine_diagnostics_and_baselines_remain_failures() {
    let fake = Fake::new(
        serde_json::json!({"check": {"stderr": "SCRIPT ERROR: Invalid call. Nonexistent function 'nope'.\n   at: res://one.gd:7\n"}}),
    );
    let dir = project(&[("one.gd", "extends Node\n")]);
    let before = fake
        .command(dir.path())
        .args(["--output", "json"])
        .output()
        .unwrap();
    let value = report(&before, 1, "failed");
    assert!(
        value["phases"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| !p["diagnostics"].as_array().unwrap().is_empty())
    );
    let baseline = dir.path().join("baseline.json");
    fs::write(&baseline, &before.stdout).unwrap();
    let after = fake
        .command(dir.path())
        .args(["--output", "json", "--baseline"])
        .arg(&baseline)
        .output()
        .unwrap();
    let value = report(&after, 1, "failed");
    assert_eq!(value["baseline"]["new"].as_array().unwrap().len(), 0);
    assert_eq!(value["baseline"]["carried"].as_array().unwrap().len(), 1);
}

#[test]
fn carried_static_findings_allow_engine_phases() {
    let fake = Fake::new(serde_json::json!({}));
    let dir = project(&[MISSING_PRELOAD]);
    let before = gdkit(&[
        "check",
        "--static-only",
        "--output",
        "json",
        "--project",
        dir.path().to_str().unwrap(),
    ]);
    let previous = report(&before, 1, "failed");
    let baseline = dir.path().join("baseline.json");
    fs::write(&baseline, before.stdout).unwrap();
    let value = report(
        &fake
            .command(dir.path())
            .args(["--output", "json", "--baseline"])
            .arg(baseline)
            .output()
            .unwrap(),
        1,
        "failed",
    );
    assert_eq!(value["baseline"]["carried"].as_array().unwrap().len(), 1);
    assert_eq!(value["baseline"]["new"].as_array().unwrap().len(), 0);
    assert_eq!(value["failures"], previous["failures"]);
    assert_eq!(value["failures"].as_array().unwrap().len(), 1);
    assert_eq!(value["failures"][0]["kind"], "static_finding");
    let phases = value["phases"].as_array().unwrap();
    assert_eq!(phases[0]["outcome"], "failed");
    // Baselines open the engine gate; they do not clear failures or the verdict.
    for id in [
        "import",
        "import_scan",
        "class_cache_audit",
        "resource_loading",
    ] {
        let phase = phases.iter().find(|phase| phase["id"]["id"] == id).unwrap();
        assert_eq!(phase["outcome"], "completed", "{id}: {phase}");
    }
    assert!(fake.log().contains("check.gd"));
}

#[test]
fn deadlines_produce_incomplete_reports_and_skip_later_scripts() {
    for (scenario, phase, args) in [
        (
            serde_json::json!({"--import": {"mode": "hang"}}),
            "import",
            vec!["--phase-timeout", "1"],
        ),
        (
            serde_json::json!({"script_bootstrap:res://one.gd": {"mode": "hang"}}),
            "project_script:0:res://one.gd",
            vec![
                "--script-timeout",
                "1",
                "--script",
                "res://one.gd",
                "--script",
                "res://two.gd",
            ],
        ),
    ] {
        let fake = Fake::new(scenario);
        let dir = project(&[
            ("one.gd", "extends SceneTree\n"),
            ("two.gd", "extends SceneTree\n"),
        ]);
        let value = report(
            &fake
                .command(dir.path())
                .args(["--output", "json"])
                .args(args)
                .output()
                .unwrap(),
            1,
            "incomplete",
        );
        assert!(
            value["phases"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["id"]["id"] == phase && p["outcome"] == "timed_out")
        );
        assert!(!fake.log().contains("\"res://two.gd\""));
    }
}

#[test]
fn human_reports_explain_failures_without_diagnostics_and_verbose_replays_streams() {
    let fake = Fake::new(
        serde_json::json!({"check": {"mode": "no_envelope", "stdout": "raw engine output\n"}}),
    );
    let dir = project(&[]);
    let output = fake.command(dir.path()).arg("--verbose").output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("resource_loading: failure:"), "{stdout}");
    assert!(stdout.contains("check INCOMPLETE"), "{stdout}");
    assert!(stdout.contains("artifacts:"), "{stdout}");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("engine:"), "{stderr}");
    assert!(stderr.contains("raw engine output"), "{stderr}");
    report(
        &fake
            .command(dir.path())
            .args(["--output", "json", "--verbose"])
            .output()
            .unwrap(),
        1,
        "incomplete",
    );
}

#[test]
fn static_scripts_and_invalid_setup_are_rejected() {
    let dir = project(&[]);
    let root = dir.path().to_str().unwrap();
    for extra in [
        vec!["--static-only", "--script", "res://test.gd"],
        vec!["--static-only", "--baseline", "bad.json"],
        vec!["--static-only", "--script-timeout", "0"],
        vec!["--static-only", "--phase-timeout", "0"],
        vec!["--phase-timeout", "0"],
        vec!["--phase-timeout", "86401"],
        vec!["--script", "../escape.gd"],
        // A slice that does not exist is a usage error, not a vacuous pass.
        vec!["--static-only", "--slice", "typo.gd"],
        vec!["--slice", "typo.gd"],
    ] {
        let mut args = vec!["check", "--project", root, "--output", "json"];
        args.extend(extra);
        let output = gdkit(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).starts_with("error:"));
    }
    fs::write(dir.path().join("bad.json"), "not json").unwrap();
    let output = gdkit(&[
        "check",
        "--static-only",
        "--project",
        root,
        "--baseline",
        dir.path().join("bad.json").to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(2));
    fs::write(dir.path().join("gdkit.toml"), "[check]\nunknown = true\n").unwrap();
    assert_eq!(
        gdkit(&["check", "--static-only", "--project", root])
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn baseline_schema_mismatch_is_a_tool_error_and_foreign_project_warns() {
    let dir = project(&[MISSING_PRELOAD]);
    let root = dir.path().to_str().unwrap();
    let before = gdkit(&[
        "check",
        "--static-only",
        "--project",
        root,
        "--output",
        "json",
    ]);
    let mut value: serde_json::Value = serde_json::from_slice(&before.stdout).unwrap();
    let baseline = dir.path().join("baseline.json");
    for version in [serde_json::json!(2), serde_json::Value::Null] {
        value["schema_version"] = version;
        fs::write(&baseline, value.to_string()).unwrap();
        let output = gdkit(&[
            "check",
            "--static-only",
            "--project",
            root,
            "--baseline",
            baseline.to_str().unwrap(),
        ]);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(
            stderr.starts_with("error: ") && stderr.contains("schema_version"),
            "{stderr}"
        );
    }
    // A report from another project is still usable, with a warning.
    let other = project(&[]);
    fs::write(&baseline, &before.stdout).unwrap();
    let output = gdkit(&[
        "check",
        "--static-only",
        "--project",
        other.path().to_str().unwrap(),
        "--output",
        "json",
        "--baseline",
        baseline.to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.starts_with("warning: ") && stderr.contains("recorded for project"),
        "{stderr}"
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["baseline"]["resolved"].as_array().unwrap().len(), 1);
    // The same project does not warn.
    let output = gdkit(&[
        "check",
        "--static-only",
        "--project",
        root,
        "--output",
        "json",
        "--baseline",
        baseline.to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn human_summary_counts_engine_failures() {
    let fake = Fake::new(serde_json::json!({"check": {"stderr": "ERROR: engine problem\n"}}));
    let dir = project(&[("one.gd", "extends Node\n")]);
    let output = fake.command(dir.path()).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    let summary = stdout.lines().last().unwrap();
    assert!(
        summary.starts_with("check FAILED: 1 failure(s) (0 static finding(s))"),
        "{stdout}"
    );
}

#[test]
fn missing_slice_is_rejected_before_the_engine_is_probed() {
    let fake = Fake::new(serde_json::json!({}));
    let dir = project(&[("one.gd", "extends Node\n")]);
    let output = fake
        .command(dir.path())
        .args(["--slice", "./typo.gd"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.starts_with("error: ") && stderr.contains("typo.gd"),
        "{stderr}"
    );
    assert!(fake.take_log().is_empty(), "engine ran before validation");
    // A `./` spelling of an existing path is normalized and checked in the engine.
    let value = report(
        &fake
            .command(dir.path())
            .args(["--output", "json", "--slice", "./one.gd"])
            .output()
            .unwrap(),
        0,
        "passed",
    );
    assert_eq!(value["project"]["sliced"], true);
}

/// Valid arguments that reach the command's handler, for every leaf command.
fn smoke_args(path: &str, dir: &Path) -> Vec<String> {
    let root = dir.to_str().unwrap();
    let args: Vec<&str> = match path {
        "init" | "doctor" | "check" | "autoloads" | "net" | "import" | "run" => {
            vec![path, "--project", root]
        }
        "api" => vec!["api", "--project", root, "Node"],
        "refs" => vec!["refs", "--project", root, "res://main.tscn"],
        "settings get" => vec![
            "settings",
            "--project",
            root,
            "get",
            "application",
            "config/name",
        ],
        "resource schema" => vec![
            "resource",
            "schema",
            "--project",
            root,
            "--class",
            "Resource",
        ],
        "resource create" => {
            let spec = dir.join("spec.json");
            return ["resource", "create", "--project", root, "--spec"]
                .into_iter()
                .map(str::to_owned)
                .chain([
                    spec.to_str().unwrap().to_owned(),
                    "--out".into(),
                    "res://x.tres".into(),
                ])
                .collect();
        }
        "scene-tree" => {
            let scene = dir.join("main.tscn");
            return vec!["scene-tree".into(), scene.to_str().unwrap().to_owned()];
        }
        _ => match path.strip_prefix("settings ") {
            Some(what) => vec!["settings", "--project", root, what],
            None => panic!("add smoke args for `{path}` to smoke_args in tests/cli.rs"),
        },
    };
    args.into_iter().map(str::to_owned).collect()
}

fn panicked(output: &Output) -> bool {
    output.status.code() == Some(101)
        && String::from_utf8_lossy(&output.stderr).contains("panicked")
}

#[test]
fn status_table_matches_what_each_command_does() {
    let dir = project(&[
        (
            "main.tscn",
            "[gd_scene format=3]\n\n[node name=\"Main\" type=\"Node\"]\n",
        ),
        ("spec.json", "{}"),
    ]);
    for path in status::leaves(&cli::Cli::command(), "") {
        let args = smoke_args(&path, dir.path());
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        match status::of(&path) {
            status::Status::Ready => {
                let output = gdkit(&args);
                assert!(
                    !panicked(&output),
                    "`{path}` is marked Ready in src/status.rs but panics:\n{}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            status::Status::Stub => {
                let output = gdkit(&args);
                assert_eq!(output.status.code(), Some(2), "`{path}`");
                assert!(output.stdout.is_empty(), "`{path}`");
                assert_eq!(
                    String::from_utf8_lossy(&output.stderr),
                    format!("error: `gdkit {path}` is not implemented yet\n")
                );
                // The unlock only exists in debug builds.
                if cfg!(debug_assertions) {
                    let output = Command::new(env!("CARGO_BIN_EXE_gdkit"))
                        .args(&args)
                        .env("GDKIT_GODOT", "/nonexistent/godot")
                        .env("GDKIT_ALLOW_STUBS", "1")
                        .output()
                        .unwrap();
                    assert!(
                        panicked(&output),
                        "`{path}` no longer panics; mark it Ready in src/status.rs"
                    );
                }
            }
        }
    }
}

#[test]
fn help_tags_every_command_that_is_not_ready() {
    let command = cli::Cli::command();
    let help = String::from_utf8(gdkit(&["--help"]).stdout).unwrap();
    assert!(
        !help.contains('\x1b'),
        "piped help must not carry color codes"
    );
    for sub in command.get_subcommands() {
        let name = sub.get_name();
        let line = help
            .lines()
            .find(|line| line.trim_start().starts_with(&format!("{name} ")))
            .unwrap_or_else(|| panic!("`{name}` missing from help:\n{help}"));
        match status::tag(sub, name) {
            Some(tag) => assert!(line.ends_with(&format!("({tag})")), "{line}"),
            None => assert!(!line.ends_with("implemented)"), "{line}"),
        }
    }
}
