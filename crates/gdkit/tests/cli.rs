// Drives the built binary. Engine tests explicitly build gdproject's fake-godot
// once into an isolated target directory (offline, with a three-minute deadline).
// This works for both `cargo test -p gdkit` and `cargo test --workspace`, without
// relying on Cargo building dependency binaries or a pre-existing sibling binary.
// Every test copies the executable and its scenario; no process-global env changes.
#![allow(unused)]

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

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

fn fake_binary() -> &'static Path {
    static BUILD: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    let dir = BUILD.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        let log = fs::File::create(dir.path().join("build.log")).unwrap();
        let mut child = Command::new(env!("CARGO"))
            .args(["build", "--offline", "--locked", "--manifest-path"])
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../gdproject/Cargo.toml"))
            .args([
                "--features",
                "test-engine",
                "--bin",
                "fake-godot",
                "--target-dir",
            ])
            .arg(dir.path())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap();
        let started = std::time::Instant::now();
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(
                    status.success(),
                    "fake engine build failed: {}",
                    fs::read_to_string(dir.path().join("build.log")).unwrap()
                );
                break;
            }
            if started.elapsed() > std::time::Duration::from_secs(180) {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("fake engine build exceeded 180 seconds");
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        dir
    });
    dir.path()
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
        copy_executable(
            &fake_binary()
                .join("debug")
                .join(format!("fake-godot{}", std::env::consts::EXE_SUFFIX)),
            &executable,
        );
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
            name.to_str().unwrap()
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
    assert_eq!(
        value["engine"]["executable"],
        fs::canonicalize(&local).unwrap().to_str().unwrap()
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
        value["engine"]["executable"],
        fake.executable.to_str().unwrap()
    );
    fs::write(
        dir.path().join("gdkit.toml"),
        "[check]\nstrict_methods = false\n",
    )
    .unwrap();
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
    assert!(fake.log().contains("strict-methods"));
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
    assert!(fake.log().contains("project-policy"));
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
        vec!["--script", "../escape.gd"],
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
