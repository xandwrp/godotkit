// Drives the built binary. Engine tests explicitly build gdproject's fake-godot
// once per test process into a dedicated target directory under
// CARGO_TARGET_TMPDIR (offline, with a three-minute deadline). That directory is
// reused across runs, so nothing leaks into /tmp and rebuilds are incremental.
// This works for both `cargo test -p gdkit` and `cargo test --workspace`, without
// relying on Cargo building dependency binaries or a pre-existing sibling binary.
// Every test copies the executable and its scenario; no process-global env changes.
// Every run gets its own GDKIT_CONFIG_DIR, so the developer's global config is
// never read or written.
#![allow(unused)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use clap::CommandFactory;

// The status table and the clap tree it describes, compiled into this test so
// the drift test can walk every command without a hand-kept list.
#[path = "../src/cli.rs"]
mod cli;
#[path = "../src/status.rs"]
mod status;

/// The binary with `config_dir` as its global config and an engine variable
/// that points nowhere, so any accidental engine use fails loudly.
fn gdkit_command(config_dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_gdkit"));
    command
        .env("GDKIT_CONFIG_DIR", config_dir)
        .env("GDKIT_GODOT", "/nonexistent/godot");
    command
}

/// Runs the binary with an empty global config.
fn gdkit(args: &[&str]) -> Output {
    let config = tempfile::tempdir().unwrap();
    gdkit_command(config.path()).args(args).output().unwrap()
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
fn scene_tree_needs_no_engine() {
    todo!()
}

const OFFLINE_PROJECT: &[(&str, &str)] = &[
    (
        "project.godot",
        "config_version=5\n\n[application]\n\nconfig/name=\"Offline\"\nrun/main_scene=\"uid://m41n\"\n\n[autoload]\n\nGame=\"*uid://g4m3\"\nHud=\"res://ui/hud.tscn\"\n\n[input]\n\njump={\n\"deadzone\": 0.2,\n\"events\": [Object(InputEventKey,\"keycode\":0,\"physical_keycode\":32,\"key_label\":0,\"unicode\":32)\n]\n}\n\n[layer_names]\n\n2d_physics/layer_3=\"Enemies\"\n",
    ),
    (
        "main.tscn",
        "[gd_scene format=3 uid=\"uid://m41n\"]\n\n[node name=\"Main\" type=\"Node\"]\n",
    ),
    (
        "game.gd",
        "extends Node\nconst MAIN := \"res://main.tscn\"\n",
    ),
    ("game.gd.uid", "uid://g4m3\n"),
];

/// Runs with `GDKIT_GODOT` pointing nowhere, so any engine use would fail.
fn offline(dir: &Path, args: &[&str]) -> Output {
    let mut all = args.to_vec();
    all.extend(["--project", dir.to_str().unwrap()]);
    gdkit(&all)
}

fn json(dir: &Path, args: &[&str], code: i32) -> serde_json::Value {
    let mut all = args.to_vec();
    all.extend(["--output", "json"]);
    let output = offline(dir, &all);
    assert_eq!(
        output.status.code(),
        Some(code),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn refs_settings_and_autoloads_need_no_engine() {
    let dir = project(OFFLINE_PROJECT);
    let dir = dir.path();

    let refs = json(dir, &["refs", "main.tscn"], 0);
    assert_eq!(refs["path"], "res://main.tscn");
    assert_eq!(refs["uid"], "uid://m41n");
    let references: Vec<_> = refs["references"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["resource"].as_str().unwrap(),
                r["line"].as_u64().unwrap(),
                r["kind"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        references,
        [
            ("res://game.gd", 2, "string"),
            ("res://project.godot", 6, "main_scene")
        ]
    );
    assert_eq!(
        json(dir, &["refs", "uid://g4m3"], 0)["sidecars"][0],
        "res://game.gd.uid"
    );
    let missing = json(dir, &["refs", "res://mian.tscn"], 1);
    assert_eq!(missing["exists"], false);
    assert_eq!(missing["suggestions"][0], "res://main.tscn");
    let error = tool_error(&offline(dir, &["refs", "uid://nothing"]));
    assert!(
        error.contains("no project file claims uid://nothing"),
        "{error}"
    );

    let input = json(dir, &["settings", "input"], 0);
    let actions = input["actions"].as_array().unwrap();
    assert_eq!(actions[0]["name"], "jump");
    assert_eq!(actions[0]["events"][0]["physical_keycode"], "KEY_SPACE");
    assert!(
        actions
            .iter()
            .any(|a| a["name"] == "ui_accept" && a["builtin"] == true)
    );
    assert_eq!(
        json(dir, &["settings", "layers"], 0)["physics_2d"]["3"],
        "Enemies"
    );
    assert_eq!(json(dir, &["settings", "window"], 0)["width"], 1152);
    let main = json(dir, &["settings", "main-scene"], 0);
    assert_eq!(
        (&main["path"], &main["exists"]),
        (&"res://main.tscn".into(), &true.into())
    );
    let name = json(dir, &["settings", "get", "application", "config/name"], 0);
    assert_eq!(name["value"], "\"Offline\"");

    // Unset values exit 1; human mode keeps stdout empty and suggests on stderr.
    let unset = offline(dir, &["settings", "get", "application", "config/nme"]);
    assert_eq!(unset.status.code(), Some(1));
    assert!(unset.stdout.is_empty());
    assert!(String::from_utf8_lossy(&unset.stderr).contains("did you mean config/name?"));
    let bare = project(&[]);
    assert_eq!(
        json(bare.path(), &["settings", "main-scene"], 1)["written"],
        serde_json::Value::Null
    );

    let autoloads = json(dir, &["autoloads"], 0);
    let listed: Vec<_> = autoloads["autoloads"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            (
                a["name"].as_str().unwrap(),
                a["kind"].as_str().unwrap(),
                a["path"].as_str().unwrap(),
                a["exists"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        listed,
        [
            ("Game", "script", "res://game.gd", true),
            ("Hud", "scene", "res://ui/hud.tscn", false)
        ]
    );
    let human = String::from_utf8(offline(dir, &["autoloads"]).stdout).unwrap();
    assert!(
        human.contains("res://ui/hud.tscn  (not a global name; missing)"),
        "{human}"
    );
}

fn succeeded(output: &Output) -> serde_json::Value {
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "JSON mode printed to stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn tool_error(output: &Output) -> String {
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr.clone()).unwrap();
    assert!(stderr.starts_with("error: "), "{stderr}");
    stderr
}

/// Reports a Godot 3 build, which the compatibility probe rejects.
fn godot_3() -> Fake {
    Fake::new(
        serde_json::json!({"probe": {"payload": {"version": "3.6.stable", "major": 3, "editor": true}}}),
    )
}

#[test]
fn init_writes_config_and_refuses_to_overwrite() {
    let fake = Fake::new(serde_json::json!({}));
    let dir = project(&[("scripts/ok.gd", "extends Node\n")]);
    let config = dir.path().join("gdkit.toml");
    // Nothing selects an engine: the error says how to set a default, and nothing is written.
    let stderr = tool_error(
        &fake
            .gdkit()
            .args(["init", "--project"])
            .arg(dir.path())
            .output()
            .unwrap(),
    );
    assert!(stderr.contains("gdkit config set godot"), "{stderr}");
    // A wrong or incompatible engine is caught by the probe before anything is written.
    let old = godot_3();
    for godot in [Path::new("/nonexistent/godot"), old.executable.as_path()] {
        tool_error(
            &fake
                .gdkit()
                .args(["init", "--project"])
                .arg(dir.path())
                .arg("--godot")
                .arg(godot)
                .output()
                .unwrap(),
        );
        assert!(!config.exists());
    }
    // --godot pins, from anywhere inside the project.
    let value = succeeded(
        &fake
            .gdkit()
            .args(["init", "--output", "json", "--project"])
            .arg(dir.path().join("scripts"))
            .arg("--godot")
            .arg(&fake.executable)
            .output()
            .unwrap(),
    );
    assert_eq!(value["pinned"], true);
    assert_eq!(value["engine"]["source"], "command_line");
    assert_eq!(
        fs::canonicalize(value["config"].as_str().unwrap()).unwrap(),
        fs::canonicalize(&config).unwrap()
    );
    let written = fs::read_to_string(&config).unwrap();
    assert!(
        written.contains(&format!(
            "executable = {:?}",
            fake.executable.to_str().unwrap()
        )),
        "{written}"
    );
    // A second init refuses before probing and leaves the file alone.
    fake.take_log();
    let stderr = tool_error(
        &fake
            .gdkit()
            .args(["init", "--project"])
            .arg(dir.path())
            .arg("--godot")
            .arg(&fake.executable)
            .output()
            .unwrap(),
    );
    assert!(stderr.contains("already exists"), "{stderr}");
    assert!(fake.take_log().is_empty(), "init probed before refusing");
    assert_eq!(fs::read_to_string(&config).unwrap(), written);
    // The pin beats a global default.
    let other = Fake::new(serde_json::json!({}));
    succeeded(
        &other
            .gdkit()
            .args(["config", "set", "godot", "--output", "json"])
            .arg(&other.executable)
            .output()
            .unwrap(),
    );
    let value = report(
        &fake
            .command(dir.path())
            .env_remove("GDKIT_GODOT")
            .env("GDKIT_CONFIG_DIR", other.config_dir())
            .args(["--output", "json"])
            .output()
            .unwrap(),
        0,
        "passed",
    );
    assert_eq!(
        fs::canonicalize(value["engine"]["executable"].as_str().unwrap()).unwrap(),
        fs::canonicalize(&fake.executable).unwrap()
    );
}

#[test]
fn init_without_godot_follows_the_global_default() {
    let fake = Fake::new(serde_json::json!({}));
    let dir = project(&[]);
    succeeded(
        &fake
            .gdkit()
            .args(["config", "set", "godot", "--output", "json"])
            .arg(&fake.executable)
            .output()
            .unwrap(),
    );
    let output = fake
        .gdkit()
        .args(["init", "--project"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("follows the global default"), "{stdout}");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.starts_with("engine: "), "{stderr}");
    // No [engine] table, only the commented example of how to pin one.
    let written = fs::read_to_string(dir.path().join("gdkit.toml")).unwrap();
    assert!(
        !written.lines().any(|line| line.starts_with("[engine]")),
        "{written}"
    );
    assert!(written.contains("# [engine]"), "{written}");
    let check = || {
        fake.command(dir.path())
            .env_remove("GDKIT_GODOT")
            .args(["--output", "json"])
            .output()
            .unwrap()
    };
    let engine = |output: &Output| {
        let value = report(output, 0, "passed");
        fs::canonicalize(value["engine"]["executable"].as_str().unwrap()).unwrap()
    };
    assert_eq!(
        engine(&check()),
        fs::canonicalize(&fake.executable).unwrap()
    );
    // Changing the global default moves the project with it.
    let other = Fake::new(serde_json::json!({}));
    succeeded(
        &fake
            .gdkit()
            .args(["config", "set", "godot", "--output", "json"])
            .arg(&other.executable)
            .output()
            .unwrap(),
    );
    assert_eq!(
        engine(&check()),
        fs::canonicalize(&other.executable).unwrap()
    );
    // GDKIT_GODOT still overrides it.
    let overridden = fake
        .command(dir.path())
        .args(["--output", "json"])
        .output()
        .unwrap();
    assert_eq!(
        engine(&overridden),
        fs::canonicalize(&fake.executable).unwrap()
    );
    // Without a global default the project has no engine.
    succeeded(
        &fake
            .gdkit()
            .args(["config", "unset", "godot", "--output", "json"])
            .output()
            .unwrap(),
    );
    assert!(tool_error(&check()).contains("gdkit config set godot"));
}

#[test]
fn config_set_get_unset_list_round_trip() {
    let fake = Fake::new(serde_json::json!({}));
    let file = fake.config_dir().join("config.toml");
    let exe = fake.executable.to_str().unwrap();
    let config = |args: &[&str]| fake.gdkit().arg("config").args(args).output().unwrap();
    // Unset: `get` exits 1 with nothing on stdout; `list` shows the file and the gap.
    let get = config(&["get", "godot"]);
    assert_eq!(get.status.code(), Some(1));
    assert!(get.stdout.is_empty() && get.stderr.is_empty());
    let get = config(&["get", "godot", "--output", "json"]);
    assert_eq!(get.status.code(), Some(1));
    let value: serde_json::Value = serde_json::from_slice(&get.stdout).unwrap();
    assert_eq!(value["value"], serde_json::Value::Null);
    let list = succeeded(&config(&["list", "--output", "json"]));
    assert_eq!(list["path"], file.to_str().unwrap());
    assert_eq!(list["values"]["godot"], serde_json::Value::Null);
    // `set` probes first: a missing or incompatible engine is never saved.
    let old = godot_3();
    for godot in ["/nonexistent/godot", old.executable.to_str().unwrap()] {
        tool_error(&config(&["set", "godot", godot]));
        assert!(!file.exists());
    }
    tool_error(&config(&["get", "bogus"]));
    // Human `set` names the engine on stderr and the saved value on stdout.
    let set = config(&["set", "godot", exe]);
    assert_eq!(set.status.code(), Some(0));
    let stdout = String::from_utf8(set.stdout).unwrap();
    assert!(stdout.starts_with(&format!("godot = {exe}\n")), "{stdout}");
    assert!(
        String::from_utf8(set.stderr)
            .unwrap()
            .starts_with("engine: ")
    );
    let get = config(&["get", "godot"]);
    assert_eq!(get.status.code(), Some(0));
    assert_eq!(String::from_utf8(get.stdout).unwrap(), format!("{exe}\n"));
    assert_eq!(
        succeeded(&config(&["list", "--output", "json"]))["values"]["godot"],
        exe
    );
    // Hand edits survive later `set`s.
    let text = fs::read_to_string(&file).unwrap();
    fs::write(&file, format!("# my defaults\n{text}")).unwrap();
    let value = succeeded(&config(&["set", "godot", exe, "--output", "json"]));
    assert_eq!(value["value"], exe);
    assert_eq!(value["engine"]["source"], "command_line");
    assert!(
        fs::read_to_string(&file)
            .unwrap()
            .starts_with("# my defaults\n")
    );
    // `unset` is idempotent.
    assert_eq!(
        succeeded(&config(&["unset", "godot", "--output", "json"]))["removed"],
        true
    );
    assert_eq!(config(&["get", "godot"]).status.code(), Some(1));
    assert_eq!(
        succeeded(&config(&["unset", "godot", "--output", "json"]))["removed"],
        false
    );
    // A broken file is a tool error for every subcommand and is left as it is.
    fs::write(&file, "godot = 1\n").unwrap();
    for args in [
        vec!["get", "godot"],
        vec!["list"],
        vec!["unset", "godot"],
        vec!["set", "godot", exe],
    ] {
        let stderr = tool_error(&config(&args));
        assert!(stderr.contains("config.toml"), "{args:?}: {stderr}");
        assert_eq!(fs::read_to_string(&file).unwrap(), "godot = 1\n");
    }
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

    /// This fake's global config directory; empty until a test writes to it.
    fn config_dir(&self) -> PathBuf {
        self.dir.path().join("config")
    }

    /// gdkit with this fake's global config and no GDKIT_GODOT.
    fn gdkit(&self) -> Command {
        let mut command = gdkit_command(&self.config_dir());
        command
            .env_remove("GDKIT_GODOT")
            .env_remove("FAKE_GODOT_SCENARIO")
            .env_remove("FAKE_GODOT_LOG");
        command
    }

    fn command(&self, root: &Path) -> Command {
        let mut command = self.gdkit();
        command
            .args(["check", "--project"])
            .arg(root)
            .env("GDKIT_GODOT", &self.executable);
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
        "config get" | "config unset" => vec!["config", &path[7..], "godot"],
        "config set" => vec!["config", "set", "godot", "/nonexistent/godot"],
        "config list" => vec!["config", "list"],
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
                    let config = tempfile::tempdir().unwrap();
                    let output = gdkit_command(config.path())
                        .args(&args)
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

/// The fake engine's outputs for `api`: the trimmed 4.7.2 dump and doctool
/// fixtures from gdview, plus docs for one project script.
fn api_fake() -> Fake {
    let fixture = |name: &str| {
        fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("crates/gdview/tests/fixtures/api")
                .join(name),
        )
        .unwrap()
    };
    Fake::new(serde_json::json!({
        "--dump-extension-api-with-docs": {
            "files": {"extension_api.json": fixture("extension_api.json")}
        },
        "--doctool": {"files": {
            "modules/gdscript/doc_classes/@GDScript.xml": fixture("doctool/@GDScript.xml"),
            "doc/classes/Node.xml": fixture("doctool/Node.xml"),
        }},
        "--gdscript-docs": {"files": {
            "Player.xml": "<class name=\"Player\" inherits=\"CharacterBody3D\"><brief_description>The hero.</brief_description></class>"
        }},
    }))
}

#[test]
fn api_exit_codes_follow_the_answer_and_json_is_one_document() {
    let fake = api_fake();
    let dir = project(&[("player.gd", "class_name Player\nextends CharacterBody3D\n")]);
    let api = |args: &[&str]| {
        fake.gdkit()
            .arg("api")
            .arg("--project")
            .arg(dir.path())
            .args(args)
            .env("GDKIT_GODOT", &fake.executable)
            .output()
            .unwrap()
    };
    let json = |output: &Output, code: i32| -> serde_json::Value {
        assert_eq!(
            output.status.code(),
            Some(code),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    };

    let found = json(
        &api(&["--output", "json", "CharacterBody3D", "move_and_slide"]),
        0,
    );
    assert_eq!(found["kind"], "member");
    assert_eq!(
        found["signature"],
        "CharacterBody3D.move_and_slide() -> bool"
    );
    assert_eq!(found["engine_version"], "4.7.2.stable.arch_linux");
    assert!(
        found.get("project_scripts").is_none(),
        "engine answers skip the scripts"
    );

    let miss = json(
        &api(&["--output", "json", "CharacterBody3D", "move_and_slid"]),
        1,
    );
    assert_eq!(miss["kind"], "miss");
    assert_eq!(miss["suggestions"][0], "move_and_slide");

    let player = json(&api(&["--output", "json", "Player", "move_and_slide"]), 0);
    assert_eq!(player["declaring_class"], "CharacterBody3D");
    assert_eq!(player["project_scripts"]["source"], "script_copy");

    let search = json(
        &api(&["--output", "json", "search", "player", "--limit", "1"]),
        0,
    );
    assert_eq!(search["results"][0]["name"], "Player");

    let human = api(&["lerp"]);
    assert_eq!(human.status.code(), Some(0));
    assert!(
        String::from_utf8_lossy(&human.stdout)
            .starts_with("lerp(from: Variant, to: Variant, weight: Variant) -> Variant")
    );
    assert!(String::from_utf8_lossy(&human.stderr).starts_with("engine: "));

    let error = api(&["search"]);
    assert_eq!(error.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&error.stderr)
            .starts_with("error: `gdkit api search` needs a term")
    );

    let outside = tempfile::tempdir().unwrap();
    let dump = fake
        .gdkit()
        .args(["--output", "json", "api", "--dump"])
        .current_dir(outside.path())
        .env("GDKIT_GODOT", &fake.executable)
        .output()
        .unwrap();
    let index = json(&dump, 0);
    assert!(index["classes"]["CharacterBody3D"].is_object());
    assert!(!outside.path().join(".godot").exists());
}

#[test]
fn doctor_exits_1_on_problems_and_json_is_one_document() {
    let fake = Fake::new(serde_json::json!({}));
    let dir = project(&[]);
    let doctor = |args: &[&str]| {
        fake.gdkit()
            .args(["doctor", "--project"])
            .arg(dir.path())
            .args(args)
            .env("GDKIT_GODOT", &fake.executable)
            .output()
            .unwrap()
    };

    let healthy = succeeded(&doctor(&["--output", "json"]));
    assert_eq!(healthy["problems"], serde_json::json!([]));
    assert_eq!(healthy["engine"]["candidates"][0]["source"], "environment");
    assert_eq!(
        healthy["engine"]["attached"]["version"],
        "4.7.2.stable.fake"
    );
    assert_eq!(healthy["engine"]["cache_hit"], false);

    // Human mode: the report is on stdout, and no `engine:` line on stderr.
    let human = doctor(&[]);
    assert_eq!(human.status.code(), Some(0));
    assert!(
        human.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&human.stderr)
    );
    let stdout = String::from_utf8(human.stdout).unwrap();
    assert!(stdout.contains("probe cache: current, used"), "{stdout}");
    assert!(stdout.ends_with("problems  none\n"), "{stdout}");

    // A broken gdkit.toml is reported, not fatal: exit 1, one JSON document.
    fs::write(dir.path().join("gdkit.toml"), "[engine]\nexe = 1\n").unwrap();
    let broken = doctor(&["--output", "json"]);
    assert_eq!(broken.status.code(), Some(1));
    assert!(
        broken.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&broken.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&broken.stdout).unwrap();
    assert_eq!(report["problems"][0]["code"], "project_config_invalid");
    assert_eq!(report["engine"]["attached"]["version"], "4.7.2.stable.fake");
    let human = String::from_utf8(doctor(&[]).stdout).unwrap();
    assert!(
        human.contains("unknown configuration key `engine.exe`"),
        "{human}"
    );

    // Outside a project there is nothing to diagnose.
    let outside = tempfile::tempdir().unwrap();
    let output = fake
        .gdkit()
        .args(["doctor", "--project"])
        .arg(outside.path())
        .output()
        .unwrap();
    assert!(tool_error(&output).contains("not inside a Godot project"));
}

#[test]
fn resource_schema_json_is_one_document_and_target_errors_exit_2() {
    let fake = Fake::new(serde_json::json!({"resource_schema": {
        "payload": {
            "class": "Resource",
            "script_class": "WeaponDefinition",
            "properties": [
                {"name": "kind", "type": 2, "class_name": "WeaponDefinition.Kind", "hint": 2, "hint_string": "Melee:0,Ranged:5", "usage": 69638, "default": 5},
                {"name": "tags", "type": 28, "class_name": "", "hint": 23, "hint_string": "4:", "usage": 4102, "default": []},
                {"name": "spread", "type": 3, "class_name": "", "hint": 1, "hint_string": "0.0,10.0,0.5", "usage": 4102, "default": 1.5},
            ],
        },
        "stderr": "WARNING: an autoload warned\n",
    }}));
    let dir = project(&[("res/weapon.gd", "extends Resource\n")]);
    let schema = |args: &[&str]| {
        fake.gdkit()
            .args(["resource", "schema", "--project"])
            .arg(dir.path())
            .args(args)
            .env("GDKIT_GODOT", &fake.executable)
            .output()
            .unwrap()
    };

    let report = succeeded(&schema(&["--script", "res/weapon.gd", "--output", "json"]));
    assert_eq!(
        report["target"],
        serde_json::json!({"script": "res://res/weapon.gd"})
    );
    assert_eq!(report["script_class"], "WeaponDefinition");
    assert_eq!(report["fields"][0]["enum_choices"][1]["value"], 5);
    assert_eq!(report["fields"][1]["element"]["variant_type"], "String");
    assert_eq!(
        report["engine_diagnostics"][0]["message"],
        "an autoload warned"
    );
    let log = fake.take_log();
    assert!(
        log.contains(r#""--","script","res://res/weapon.gd"]"#),
        "{log}"
    );

    let human = schema(&["--script", "res://res/weapon.gd"]);
    assert_eq!(human.status.code(), Some(0));
    let stdout = String::from_utf8(human.stdout).unwrap();
    assert!(
        stdout.starts_with("WeaponDefinition (res://res/weapon.gd, extends Resource)\n"),
        "{stdout}"
    );
    assert!(
        stdout.contains("  kind    WeaponDefinition.Kind  = 5  {Melee=0, Ranged=5}\n"),
        "{stdout}"
    );
    assert!(stdout.contains("  tags    Array[String]"), "{stdout}");
    assert!(stdout.contains("range(0.0,10.0,0.5)"), "{stdout}");
    assert!(
        stdout.ends_with("engine: 0 error(s), 1 warning(s) while loading (see --output json)\n")
    );

    // Exactly one target; both or neither is a usage error before any engine run.
    fake.take_log();
    for args in [
        &[][..],
        &["--class", "Curve", "--script", "res://res/weapon.gd"],
    ] {
        assert_eq!(schema(args).status.code(), Some(2));
    }
    assert!(schema(&["--script", "/elsewhere/weapon.gd"]).status.code() == Some(2));
    assert_eq!(fake.take_log(), "");

    let fake = Fake::new(
        serde_json::json!({"resource_schema": {"mode": "error_envelope", "payload": {
            "stage": "target", "message": "Node is not a Resource", "field": "class",
        }}}),
    );
    let output = fake
        .gdkit()
        .args([
            "resource",
            "schema",
            "--class",
            "Node",
            "--output",
            "json",
            "--project",
        ])
        .arg(dir.path())
        .env("GDKIT_GODOT", &fake.executable)
        .output()
        .unwrap();
    assert_eq!(
        tool_error(&output),
        "error: harness resource_schema failed at target (class): Node is not a Resource\n"
    );
}

#[test]
fn resource_create_publishes_a_verified_file_and_every_failure_exits_2() {
    let fake = Fake::new(serde_json::json!({}));
    let dir = project(&[("weapon.gd", "extends Resource\n"), ("weapons/.keep", "")]);
    let spec = dir.path().join("shotgun.json");
    fs::write(
        &spec,
        r#"{"script": "res://weapon.gd", "properties": {"damage": 3, "offset": {"$variant": {"type": "Vector2", "value": [1, 2.5]}}}}"#,
    )
    .unwrap();
    let create = |fake: &Fake, spec: &Path, out: &str, json: bool| {
        let mut command = fake.gdkit();
        command
            .args(["resource", "create", "--project"])
            .arg(dir.path())
            .arg("--spec")
            .arg(spec)
            .args(["--out", out])
            .env("GDKIT_GODOT", &fake.executable);
        if json {
            command.args(["--output", "json"]);
        }
        command.output().unwrap()
    };

    let report = succeeded(&create(&fake, &spec, "res://weapons/shotgun.tres", true));
    assert_eq!(report["path"], "res://weapons/shotgun.tres");
    assert_eq!(
        report["target"],
        serde_json::json!({"script": "res://weapon.gd"})
    );
    assert_eq!(report["properties_written"], 2);
    assert_eq!(report["properties"]["damage"], 3);
    assert!(dir.path().join("weapons/shotgun.tres").is_file());

    let human = create(&fake, &spec, "weapons/rifle.tres", false);
    assert_eq!(human.status.code(), Some(0));
    let stdout = String::from_utf8(human.stdout).unwrap();
    assert!(
        stdout.starts_with(
            "created res://weapons/rifle.tres (res://weapon.gd, 2 properties)\n  damage  = 3\n"
        ),
        "{stdout}"
    );

    // Offline failures exit 2 before any engine run and write nothing.
    fake.take_log();
    let missing = dir.path().join("missing.json");
    let not_json = dir.path().join("not.json");
    fs::write(&not_json, "{").unwrap();
    let bad_spec = dir.path().join("bad.json");
    fs::write(&bad_spec, r#"{"script": "res://weapon.gd", "properties": {"offset": {"$variant": {"type": "Vector2", "value": [1]}}}}"#).unwrap();
    let gone = dir.path().join("gone.json");
    fs::write(&gone, r#"{"script": "res://gone.gd"}"#).unwrap();
    for (spec, out, expected) in [
        (&missing, "res://weapons/a.tres", "missing.json"),
        (&not_json, "res://weapons/a.tres", "not JSON"),
        (&bad_spec, "res://weapons/a.tres", "at /properties/offset"),
        (
            &spec,
            "res://weapons/shotgun.tres",
            "already exists; gdkit never overwrites",
        ),
        (&spec, "res://armor/a.tres", "res://armor does not exist"),
        (&spec, "res://weapons/a.res", "must name a .tres file"),
        (&gone, "res://weapons/a.tres", "do not exist: res://gone.gd"),
    ] {
        let stderr = tool_error(&create(&fake, spec, out, true));
        assert!(stderr.contains(expected), "{stderr}");
    }
    assert_eq!(fake.take_log(), "", "no engine run");

    // A verify failure names the field; nothing is published or left staged.
    let lossy = Fake::new(serde_json::json!({"resource_create": {"payload": {"echo": {
        "damage": 3, "offset": {"$variant": {"type": "Vector2", "value": [1.0, 2.0]}},
    }}}}));
    let entries = || fs::read_dir(dir.path().join("weapons")).unwrap().count();
    let before = entries();
    let stderr = tool_error(&create(&lossy, &spec, "res://weapons/lossy.tres", true));
    assert_eq!(
        stderr,
        "error: harness resource_create failed at verify (properties.offset[1]): the engine stored 2.0 where the spec has 2.5\n"
    );
    assert_eq!(entries(), before);
}
