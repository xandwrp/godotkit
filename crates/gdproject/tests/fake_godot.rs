//! Acceptance tests for the schema documented in src/bin/fake_godot.rs.
//! Every test owns a copied executable and sidecars; no process-global env edits.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use gdproject::process::{self, Captured, Spawn};
use gdproject::protocol::{Envelope, parse_envelope};
use serde_json::{Value, json};
use tempfile::TempDir;

struct Fake {
    dir: TempDir,
    executable: PathBuf,
    scenario: PathBuf,
    log: PathBuf,
}

impl Fake {
    fn new(scenario: Value) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let executable = dir
            .path()
            .join(format!("fake-godot{}", std::env::consts::EXE_SUFFIX));
        // Only the waited child opens the executable for writing, so another
        // test's fork cannot inherit a writable descriptor and cause ETXTBSY.
        #[cfg(unix)]
        {
            let status = std::process::Command::new("cp")
                .arg(env!("CARGO_BIN_EXE_fake-godot"))
                .arg(&executable)
                .status()
                .unwrap();
            assert!(status.success(), "fixture executable copy failed: {status}");
        }
        #[cfg(windows)]
        fs::copy(env!("CARGO_BIN_EXE_fake-godot"), &executable).unwrap();
        let sidecar = |suffix: &str| {
            let mut name = executable.as_os_str().to_owned();
            name.push(suffix);
            PathBuf::from(name)
        };
        let scenario_path = sidecar(".scenario.json");
        let log = sidecar(".log");
        fs::write(&scenario_path, scenario.to_string()).unwrap();
        // The probe workspace production writes: real Godot's probe needs both.
        fs::write(dir.path().join("project.godot"), "config_version=5\n").unwrap();
        fs::write(
            dir.path().join("probe.tres"),
            "[gd_resource type=\"Resource\" format=3]\n\n[resource]\n",
        )
        .unwrap();
        Self {
            dir,
            executable,
            scenario: scenario_path,
            log,
        }
    }

    fn spawn(&self, args: &[&str]) -> Spawn {
        Spawn::new(&self.executable).args(args.iter().copied())
    }

    fn run(&self, args: &[&str]) -> Captured {
        process::run(&self.spawn(args), Duration::from_secs(5)).unwrap()
    }

    /// Production's shape: `--path` is the fixture dir and ImportScan gets `--editor`.
    fn harness_spawn(&self, name: &str, user_args: &[&str]) -> Spawn {
        let mut args = vec!["--headless", "--path", self.dir.path().to_str().unwrap()];
        if name.ends_with("import_scan.gd") {
            args.push("--editor");
        }
        args.extend(["--script", name, "--"]);
        args.extend_from_slice(user_args);
        self.spawn(&args)
    }

    fn harness(&self, name: &str, user_args: &[&str]) -> Captured {
        process::run(&self.harness_spawn(name, user_args), Duration::from_secs(5)).unwrap()
    }

    fn cache(&self) -> PathBuf {
        self.dir.path().join(".godot/global_script_class_cache.cfg")
    }
}

fn stdout(captured: &Captured) -> String {
    String::from_utf8(captured.stdout()).unwrap()
}

fn stderr(captured: &Captured) -> String {
    String::from_utf8(captured.stderr()).unwrap()
}

fn envelope(captured: &Captured) -> Envelope<Value> {
    parse_envelope(stdout(captured).lines().map(str::to_owned)).unwrap()
}

/// Deadline for runs that must be killed after the fake flushes its output.
/// Generous so a loaded CI box still starts the fake and flushes before the
/// kill; the hanging fake never finishes on its own, so the property holds.
const KILL_DEADLINE: Duration = Duration::from_secs(3);

#[test]
fn defaults_help_probe_and_missing_sidecar() {
    let fake = Fake::new(json!({}));
    fs::remove_file(&fake.scenario).unwrap();
    let help = fake.run(&["--help"]);
    assert!(help.success());
    for flag in [
        "--headless",
        "--editor",
        "--path",
        "--script",
        "--import",
        "--no-header",
    ] {
        assert!(stdout(&help).contains(flag));
    }
    let result = fake.harness("/temporary harness/probe.gd", &[]);
    assert!(result.success());
    let result = envelope(&result);
    assert_eq!(result.protocol, 1);
    assert_eq!(result.harness, "probe");
    assert!(result.ok);
    assert_eq!(
        result.payload.unwrap(),
        json!({"version":"4.7.2.stable.fake","major":4,"editor":true})
    );
    assert!(result.error.is_none());
}

#[test]
fn fake_engine_honours_each_mode() {
    for mode in ["envelope", "error_envelope", "no_envelope", "hang", "crash"] {
        let fake = Fake::new(
            json!({"probe": {"mode": mode, "stdout":"noise\n", "stderr":"diagnostic\n"}}),
        );
        let result = process::run(&fake.harness_spawn("probe.gd", &[]), KILL_DEADLINE).unwrap();
        assert!(stdout(&result).starts_with("noise\n"), "{mode}");
        assert_eq!(stderr(&result), "diagnostic\n");
        match mode {
            "envelope" => {
                assert!(result.success());
                assert!(envelope(&result).ok);
            }
            "error_envelope" => {
                assert_eq!(result.status.unwrap().code(), Some(1));
                let result = envelope(&result);
                assert!(!result.ok);
                assert!(result.payload.is_none());
                let error = result.error.unwrap();
                assert_eq!(error.stage, "probe");
                assert_eq!(error.message, "fake engine error");
            }
            "no_envelope" => {
                assert!(result.success());
                assert_eq!(stdout(&result), "noise\n");
            }
            "hang" => {
                assert!(result.timed_out);
                assert_eq!(stdout(&result), "noise\n");
            }
            "crash" => {
                assert_eq!(result.status.unwrap().code(), Some(1));
                assert_eq!(stdout(&result), "noise\n");
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn payload_error_exit_and_verbatim_output_overrides() {
    let fake = Fake::new(json!({
        "help": {"stdout":"custom help", "stderr":"no newline", "exit":7},
        "probe": {"payload":{"major":3}, "exit":9},
        "check": {"payload":null},
        "script_bootstrap": {"mode":"error_envelope", "exit":0,
            "payload":{"stage":"load", "message":"bad script", "field":"target"}}
    }));
    let help = fake.run(&["--help"]);
    assert_eq!(stdout(&help), "custom help");
    assert_eq!(stderr(&help), "no newline");
    assert_eq!(help.status.unwrap().code(), Some(7));
    let probe = fake.harness("probe.gd", &[]);
    assert_eq!(probe.status.unwrap().code(), Some(9));
    assert_eq!(envelope(&probe).payload.unwrap(), json!({"major":3}));
    let check = fake.harness("check.gd", &[]);
    assert!(check.success());
    assert!(envelope(&check).payload.is_none());
    let script = fake.harness("script_bootstrap.gd", &["res://bad.gd"]);
    assert_eq!(script.status.unwrap().code(), Some(0));
    assert!(!stdout(&script).contains("GDKIT_SCRIPT_STARTED"));
    let error = envelope(&script).error.unwrap();
    assert_eq!(
        (
            error.stage.as_str(),
            error.message.as_str(),
            error.field.as_deref()
        ),
        ("load", "bad script", Some("target"))
    );
}

#[test]
fn bootstrap_specific_key_fallback_and_all_modes() {
    let fake = Fake::new(json!({
        "script_bootstrap": {"stdout":"fallback\n"},
        "script_bootstrap:res://normal.gd": {"stdout":"user init\n", "exit":7},
        "script_bootstrap:res://missing.gd": {"mode":"no_envelope"},
        "script_bootstrap:res://bad.gd": {"mode":"error_envelope"},
        "script_bootstrap:res://crash.gd": {"mode":"crash", "exit":17},
        "script_bootstrap:res://hang.gd": {"mode":"hang", "stderr":"SCRIPT ERROR: runtime\n"}
    }));
    let normal = fake.harness("script_bootstrap.gd", &["res://normal.gd"]);
    assert_eq!(stdout(&normal), "GDKIT_SCRIPT_STARTED\nuser init\n");
    assert_eq!(normal.status.unwrap().code(), Some(7));
    // A flag-like user argument after `--` is neither a flag nor a specific key.
    let fallback = fake.harness("script_bootstrap.gd", &["--import"]);
    assert!(fallback.success());
    assert_eq!(stdout(&fallback), "GDKIT_SCRIPT_STARTED\nfallback\n");
    let missing = fake.harness("script_bootstrap.gd", &["res://missing.gd"]);
    assert!(missing.success());
    assert!(stdout(&missing).is_empty());
    let bad = fake.harness("script_bootstrap.gd", &["res://bad.gd"]);
    assert!(!envelope(&bad).ok);
    assert!(!stdout(&bad).contains("GDKIT_SCRIPT_STARTED"));
    let crash = fake.harness("script_bootstrap.gd", &["res://crash.gd"]);
    assert_eq!(crash.status.unwrap().code(), Some(17));
    assert!(stdout(&crash).is_empty());
    let hung = process::run(
        &fake.harness_spawn("script_bootstrap.gd", &["res://hang.gd"]),
        KILL_DEADLINE,
    )
    .unwrap();
    assert!(hung.timed_out);
    assert_eq!(stdout(&hung), "GDKIT_SCRIPT_STARTED\n");
    assert_eq!(stderr(&hung), "SCRIPT ERROR: runtime\n");
}

#[test]
fn delays_flush_output_before_deadline_and_before_normal_exit() {
    let fake = Fake::new(json!({"probe":{"delay_ms":200, "stderr":"partial"}}));
    let completed = fake.harness("probe.gd", &[]);
    assert!(completed.success());
    assert!(completed.duration >= Duration::from_millis(200));
    fs::write(
        &fake.scenario,
        json!({"probe":{"delay_ms":60000,"stderr":"partial"}}).to_string(),
    )
    .unwrap();
    let timed = process::run(&fake.harness_spawn("probe.gd", &[]), KILL_DEADLINE).unwrap();
    assert!(timed.timed_out);
    assert_eq!(stderr(&timed), "partial");
    assert!(envelope(&timed).ok);
}

const RUNTIME_EXTENSIONS: &[&str] = &[
    "gd",
    "gdshader",
    "gdshaderinc",
    "json",
    "res",
    "scn",
    "tres",
    "tscn",
];
const EDITOR_EXTENSIONS: &[&str] = &[
    "bmp",
    "gd",
    "gdshader",
    "gdshaderinc",
    "json",
    "png",
    "res",
    "scn",
    "svg",
    "tres",
    "tscn",
];

#[test]
fn full_manifest_uses_editor_handoff_and_filters_case_insensitively() {
    let fake = Fake::new(json!({}));
    let manifest = fake.dir.path().join("manifest with spaces.json");
    let extensions = fake.dir.path().join("editor extensions.json");
    fs::write(
        &manifest,
        json!([
            "res://a.gd",
            "res://b.GD",
            "res://a.gd",
            "res://a.tscn",
            "res://b.SCN",
            "res://a.tres",
            "res://b.res",
            "res://image.svg",
            "res://missing.BMP",
            "res://shared.GDSHADERINC",
            "res://data.JSON",
            "res://texture.png",
            "res://a.gdshader",
            "res://notes.md",
            "res://project.godot",
            "res://script.gd.uid",
            "res://readme",
            "res://dir.gd/no_extension",
            "res://unknown.custom"
        ])
        .to_string(),
    )
    .unwrap();
    for (name, ext_file) in [
        (manifest.to_str().unwrap(), extensions.to_str().unwrap()),
        ("manifest with spaces.json", "editor extensions.json"),
        (
            "res://manifest with spaces.json",
            "res://editor extensions.json",
        ),
    ] {
        let scan = fake.harness("import_scan.gd", &[name]);
        assert!(scan.success(), "{}", stderr(&scan));
        let scanned = envelope(&scan).payload.unwrap();
        assert_eq!(
            scanned,
            json!({"scanned":3,"recognized_extensions":EDITOR_EXTENSIONS})
        );
        assert!(fake.cache().exists());
        fs::write(&extensions, scanned["recognized_extensions"].to_string()).unwrap();
        for policy in ["strict-methods", "project-policy"] {
            let check = fake.harness("check.gd", &[name, policy, ext_file]);
            assert!(check.success(), "{}", stderr(&check));
            assert_eq!(
                envelope(&check).payload.unwrap(),
                json!({
                    "counts":{"scripts":3,"scenes":2,"resources":8},
                    "failures":[], "recognized_extensions":EDITOR_EXTENSIONS
                })
            );
        }
    }
    // Eligibility depends on the declared registry, not whether resources exist.
    assert!(!fake.dir.path().join("missing.BMP").exists());
    fs::write(&manifest, "[]").unwrap();
    assert_eq!(
        envelope(&fake.harness("import_scan.gd", &[manifest.to_str().unwrap()]))
            .payload
            .unwrap(),
        json!({"scanned":0,"recognized_extensions":EDITOR_EXTENSIONS})
    );
    assert_eq!(
        envelope(&fake.harness(
            "check.gd",
            &[
                manifest.to_str().unwrap(),
                "project-policy",
                extensions.to_str().unwrap()
            ]
        ))
        .payload
        .unwrap()["counts"],
        json!({"scripts":0,"scenes":0,"resources":0})
    );
}

#[test]
fn check_unions_normalizes_and_deduplicates_extensions_with_explicit_empty_supported() {
    let fake = Fake::new(json!({}));
    fs::write(
        fake.dir.path().join("files.json"),
        json!([
            "res://a.GD",
            "res://a.JSON",
            "res://a.GDSHADERINC",
            "res://a.BMP",
            "res://a.CUSTOM",
            "res://a.CUSTOM",
            "res://a.svg",
            "res://notes.md"
        ])
        .to_string(),
    )
    .unwrap();
    let extensions = fake.dir.path().join("extensions.json");
    fs::write(&extensions, "[]").unwrap();
    let args = ["files.json", "project-policy", "extensions.json"];
    let runtime = fake.harness("check.gd", &args);
    assert!(runtime.success());
    assert_eq!(
        envelope(&runtime).payload.unwrap(),
        json!({
            "counts":{"scripts":1,"scenes":0,"resources":2}, "failures":[],
            "recognized_extensions":RUNTIME_EXTENSIONS
        })
    );
    fs::write(
        &extensions,
        json!(["CUSTOM", "bmp", "custom", "GD", "BMP"]).to_string(),
    )
    .unwrap();
    let union = fake.harness("check.gd", &args);
    assert!(union.success());
    assert_eq!(
        envelope(&union).payload.unwrap(),
        json!({
            "counts":{"scripts":1,"scenes":0,"resources":5}, "failures":[],
            "recognized_extensions":["bmp","custom","gd","gdshader","gdshaderinc","json","res","scn","tres","tscn"]
        })
    );
}

#[test]
fn payload_overrides_support_custom_registries_and_bypass_default_inputs() {
    let scan_payload = json!({"scanned":99,"recognized_extensions":["custom"]});
    let check_payload = json!({
        "counts":{"scripts":0,"scenes":0,"resources":1},
        "failures":["res://bad.custom"], "recognized_extensions":["custom"]
    });
    let fake = Fake::new(json!({
        "import_scan":{"payload":scan_payload}, "check":{"payload":check_payload}
    }));
    let scan = fake.harness("import_scan.gd", &[]);
    assert!(scan.success());
    assert_eq!(envelope(&scan).payload.unwrap(), scan_payload);
    let check = fake.harness("check.gd", &[]);
    assert!(check.success());
    assert_eq!(envelope(&check).payload.unwrap(), check_payload);
    fs::write(
        fake.dir.path().join("extensions.json"),
        scan_payload["recognized_extensions"].to_string(),
    )
    .unwrap();
    fs::write(
        fake.dir.path().join("files.json"),
        json!(["res://a.CUSTOM"]).to_string(),
    )
    .unwrap();
    fs::write(&fake.scenario, "{}").unwrap();
    let check = fake.harness(
        "check.gd",
        &["files.json", "project-policy", "extensions.json"],
    );
    assert!(check.success());
    assert_eq!(envelope(&check).payload.unwrap()["counts"]["resources"], 1);
}

fn assert_input_error(result: &Captured, stage: &str, field: Option<&str>) {
    assert_eq!(result.status.unwrap().code(), Some(2));
    let result = envelope(result);
    assert!(!result.ok);
    assert!(result.payload.is_none());
    let error = result.error.unwrap();
    assert_eq!(error.stage, stage);
    assert_eq!(error.field.as_deref(), field);
}

#[test]
fn default_harnesses_require_exact_arguments_and_valid_editor_extension_file() {
    let fake = Fake::new(json!({}));
    fs::write(fake.dir.path().join("files.json"), "[]").unwrap();
    for args in [
        vec![],
        vec!["files.json"],
        vec!["files.json", "project-policy"],
        vec!["files.json", "wrong-policy", "extensions.json"],
        vec!["files.json", "strict-methods", "extensions.json", "extra"],
    ] {
        assert_input_error(&fake.harness("check.gd", &args), "arguments", None);
    }
    for args in [vec![], vec!["files.json", "extra"]] {
        assert_input_error(&fake.harness("import_scan.gd", &args), "arguments", None);
    }
    for file in ["missing.json", "[]"] {
        assert_input_error(
            &fake.harness("check.gd", &["files.json", "strict-methods", file]),
            "extensions",
            Some(file),
        );
    }
    let extensions = fake.dir.path().join("extensions.json");
    for text in [
        "not json",
        "{}",
        "null",
        "[1]",
        r#"{"recognized_extensions":[]}"#,
    ] {
        fs::write(&extensions, text).unwrap();
        assert_input_error(
            &fake.harness(
                "check.gd",
                &["files.json", "strict-methods", "extensions.json"],
            ),
            "extensions",
            Some("extensions.json"),
        );
    }
    for ext in [
        "",
        ".bmp",
        "a.b",
        "a/b",
        "a\\b",
        "a:b",
        " bmp",
        "b mp",
        "bmp\t",
        "bmp\n",
        "bmp\u{0}",
        "bmp\u{7f}",
    ] {
        fs::write(&extensions, json!([ext]).to_string()).unwrap();
        assert_input_error(
            &fake.harness(
                "check.gd",
                &["files.json", "strict-methods", "extensions.json"],
            ),
            "extensions",
            Some("extensions.json"),
        );
    }
}

#[test]
fn normal_import_generates_deterministic_fixture_cache_only_in_path() {
    let fake = Fake::new(json!({}));
    let project = fake.dir.path().join("disposable copy");
    fs::create_dir_all(project.join("nested")).unwrap();
    fs::create_dir_all(project.join(".hidden")).unwrap();
    fs::write(project.join("base.gd"), "class_name Base\nextends Node\n").unwrap();
    fs::write(
        project.join("nested/child.gd"),
        "@tool\nclass_name Child extends Base # fixture\n",
    )
    .unwrap();
    fs::write(project.join("plain.gd"), "extends Node\n").unwrap();
    fs::write(project.join("default.gd"), "class_name DefaultBase\n").unwrap();
    fs::write(project.join(".hidden/ignored.gd"), "class_name Ignored\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(project.join("base.gd"), project.join("alias.gd")).unwrap();
    let args = ["--editor", "--import", "--path", project.to_str().unwrap()];
    let imported = fake.run(&args);
    assert!(imported.success(), "{}", stderr(&imported));
    assert!(stdout(&imported).is_empty());
    assert!(!fake.cache().exists());
    let cache_path = project.join(".godot/global_script_class_cache.cfg");
    let cache = fs::read_to_string(&cache_path).unwrap();
    assert!(cache.starts_with("list=Array[Dictionary](["));
    for expected in [
        "\"class\": &\"Base\"",
        "\"base\": &\"Node\"",
        "\"class\": &\"Child\"",
        "\"base\": &\"Base\"",
        "\"base\": &\"RefCounted\"",
        "\"is_tool\": true",
        "res://nested/child.gd",
    ] {
        assert!(cache.contains(expected), "missing {expected}: {cache}");
    }
    for excluded in ["Ignored", "plain.gd", "alias.gd"] {
        assert!(!cache.contains(excluded));
    }
    assert!(fake.run(&args).success());
    assert_eq!(fs::read_to_string(&cache_path).unwrap(), cache);
}

#[test]
fn configured_cache_is_verbatim_for_both_import_phases_and_not_written_on_failure() {
    let fake = Fake::new(json!({
        "--import":{"class_cache":"intentionally stale cache\n"},
        "import_scan":{"class_cache":"", "payload":{"scanned":0}}
    }));
    let args = ["--import", "--path", fake.dir.path().to_str().unwrap()];
    assert!(fake.run(&args).success());
    assert_eq!(
        fs::read_to_string(fake.cache()).unwrap(),
        "intentionally stale cache\n"
    );
    assert!(fake.harness("import_scan.gd", &[]).success());
    assert_eq!(fs::read_to_string(fake.cache()).unwrap(), "");
    for mode in ["error_envelope", "no_envelope", "crash"] {
        fs::remove_file(fake.cache()).unwrap();
        fs::write(
            &fake.scenario,
            json!({"--import":{"mode":mode,"class_cache":"must not write"}}).to_string(),
        )
        .unwrap();
        fake.run(&args);
        assert!(!fake.cache().exists());
        fs::write(fake.cache(), "sentinel").unwrap();
    }
}

#[test]
fn sidecars_are_per_executable_and_inherited_environment_is_ignored() {
    let first = Fake::new(json!({"probe":{"payload":{"owner":"first"}}}));
    let second = Fake::new(json!({"probe":{"payload":{"owner":"second"}}}));
    let args = |fake: &Fake| {
        [
            "--headless",
            "--path",
            fake.dir.path().to_str().unwrap(),
            "--script",
            "/some path/probe.gd",
            "--",
            "argument with spaces",
            "--help",
        ]
        .map(str::to_owned)
    };
    let (first_args, second_args) = (args(&first), args(&second));
    let a = first.spawn(&first_args.each_ref().map(String::as_str));
    let b = second.spawn(&second_args.each_ref().map(String::as_str));
    let (a, b) = std::thread::scope(|scope| {
        let a = scope.spawn(|| process::run(&a, Duration::from_secs(5)).unwrap());
        let b = scope.spawn(|| process::run(&b, Duration::from_secs(5)).unwrap());
        (a.join().unwrap(), b.join().unwrap())
    });
    assert_eq!(envelope(&a).payload.unwrap()["owner"], "first");
    assert_eq!(envelope(&b).payload.unwrap()["owner"], "second");
    // Variables a developer or CI job happens to export must not redirect a test.
    let env_log = first.dir.path().join("env log.jsonl");
    let inherited = first
        .spawn(&first_args.each_ref().map(String::as_str))
        .env("FAKE_GODOT_SCENARIO", r#"{"probe":{"mode":"crash"}}"#)
        .env("FAKE_GODOT_LOG", env_log.as_os_str());
    for _ in 0..2 {
        let result = process::run(&inherited, Duration::from_secs(5)).unwrap();
        assert!(result.success());
        assert_eq!(envelope(&result).payload.unwrap()["owner"], "first");
    }
    assert!(!env_log.exists());
    for (fake, args, count) in [(&first, &first_args, 3), (&second, &second_args, 1)] {
        let log = fs::read_to_string(&fake.log).unwrap();
        assert_eq!(log.lines().count(), count);
        for line in log.lines() {
            assert_eq!(serde_json::from_str::<Value>(line).unwrap(), json!(args));
        }
    }
}

#[test]
fn probe_requires_project_godot_and_probe_tres_like_godot() {
    for missing in ["project.godot", "probe.tres"] {
        // A payload override does not bypass the workspace requirement.
        for scenario in [
            json!({}),
            json!({"probe":{"payload":{"major":4}, "exit":0}}),
        ] {
            let fake = Fake::new(scenario);
            fs::remove_file(fake.dir.path().join(missing)).unwrap();
            let result = fake.harness("probe.gd", &[]);
            assert_eq!(result.status.unwrap().code(), Some(1), "{missing}");
            let result = envelope(&result);
            assert!(!result.ok);
            assert!(result.payload.is_none());
            assert_eq!(result.error.unwrap().stage, "resource");
        }
    }
    // A directory is not a loadable resource either.
    let fake = Fake::new(json!({}));
    fs::remove_file(fake.dir.path().join("probe.tres")).unwrap();
    fs::create_dir(fake.dir.path().join("probe.tres")).unwrap();
    assert_eq!(
        fake.harness("probe.gd", &[]).status.unwrap().code(),
        Some(1)
    );
    // Without --path the probe runs in the current directory, which is no probe workspace.
    let result = fake.run(&["--headless", "--script", "probe.gd"]);
    assert_eq!(envelope(&result).error.unwrap().stage, "resource");
}

#[test]
fn import_scan_without_editor_fails_like_godot_and_writes_no_cache() {
    let fake = Fake::new(json!({}));
    fs::write(fake.dir.path().join("files.json"), "[]").unwrap();
    let path = fake.dir.path().to_str().unwrap();
    let without_editor = |user_args: &[&str]| {
        let mut args = vec![
            "--headless",
            "--path",
            path,
            "--script",
            "import_scan.gd",
            "--",
        ];
        args.extend_from_slice(user_args);
        fake.run(&args)
    };
    assert_input_error(&without_editor(&["files.json"]), "editor", None);
    assert!(!fake.cache().exists());
    // Like the real harness, argument and manifest errors are reported first.
    assert_input_error(&without_editor(&[]), "arguments", None);
    assert_input_error(
        &without_editor(&["missing.json"]),
        "manifest",
        Some("missing.json"),
    );
    // A payload override does not bypass the --editor requirement.
    fs::write(
        &fake.scenario,
        json!({"import_scan":{"payload":{"scanned":0,"recognized_extensions":[]}}}).to_string(),
    )
    .unwrap();
    assert_input_error(&without_editor(&["files.json"]), "editor", None);
    assert!(!fake.cache().exists());
    // `--editor` after `--` is a user argument, not an engine flag.
    assert_input_error(&without_editor(&["--editor"]), "editor", None);
    let scan = fake.harness("import_scan.gd", &["files.json"]);
    assert!(scan.success(), "{}", stderr(&scan));
    assert!(fake.cache().exists());
}

#[test]
fn script_bootstrap_requires_exactly_one_user_argument_like_godot() {
    for mode in ["envelope", "hang"] {
        let fake = Fake::new(json!({
            "script_bootstrap": {"mode": mode, "stdout": "user output\n"},
            "script_bootstrap:res://a.gd": {"mode": mode, "stdout": "user output\n"}
        }));
        for user_args in [
            vec![],
            vec!["res://a.gd", "res://b.gd"],
            vec!["res://a.gd", ""],
        ] {
            // A hanging scenario must still fail promptly: the script never started.
            let result = process::run(
                &fake.harness_spawn("script_bootstrap.gd", &user_args),
                KILL_DEADLINE,
            )
            .unwrap();
            assert!(!result.timed_out, "{mode} {user_args:?}");
            assert_input_error(&result, "arguments", None);
            assert!(!stdout(&result).contains("GDKIT_SCRIPT_STARTED"));
            assert!(!stdout(&result).contains("user output"));
        }
    }
    let fake = Fake::new(json!({}));
    let result = fake.harness("script_bootstrap.gd", &["res://a.gd"]);
    assert!(result.success());
    assert_eq!(stdout(&result), "GDKIT_SCRIPT_STARTED\n");
}

#[test]
fn invalid_configuration_and_unsupported_commands_fail_explicitly() {
    let fake = Fake::new(json!({}));
    for text in [
        "not json",
        "[]",
        r#"{"probe":{"mode":"typo"}}"#,
        r#"{"probe":{"exit":256}}"#,
        r#"{"probe":{"delay_ms":-1}}"#,
        r#"{"probe":{"stdout":[]}}"#,
        r#"{"probe":{"typo":true}}"#,
        r#"{"unknown":{}}"#,
        r#"{"probe":{"mode":"crash","exit":0}}"#,
        r#"{"probe":{"mode":"error_envelope","payload":null}}"#,
    ] {
        fs::write(&fake.scenario, text).unwrap();
        let result = fake.harness("probe.gd", &[]);
        assert_eq!(result.status.unwrap().code(), Some(2), "{text}");
        assert!(stderr(&result).contains("fake-godot:"));
    }
    fs::write(&fake.scenario, "{}").unwrap();
    for args in [vec![], vec!["--script", "unknown.gd"], vec!["--import"]] {
        let result = fake.run(&args);
        assert_eq!(result.status.unwrap().code(), Some(2));
        assert!(stderr(&result).contains("fake-godot:"));
    }
    for args in [
        vec!["runtime-probe"],
        vec!["runtime_probe"],
        vec!["--runtime-probe"],
        vec!["--script", "runtime_probe.gd"],
        vec!["--script", "runtime-probe.gd"],
    ] {
        let result = fake.run(&args);
        assert_eq!(result.status.unwrap().code(), Some(2));
        assert!(stderr(&result).contains("runtime-probe server is not supported"));
    }
    let result = process::run(
        &fake
            .spawn(&["--help"])
            .env("FAKE_GODOT_READY_FILE", "unused"),
        Duration::from_secs(5),
    )
    .unwrap();
    assert_eq!(result.status.unwrap().code(), Some(2));
    assert!(stderr(&result).contains("runtime-probe server is not supported"));
}

#[test]
fn malformed_missing_manifests_and_complex_inheritance_do_not_claim_success() {
    let fake = Fake::new(json!({}));
    fs::write(fake.dir.path().join("extensions.json"), "[]").unwrap();
    for harness in ["check.gd", "import_scan.gd"] {
        let args = |manifest| {
            if harness == "check.gd" {
                vec![manifest, "project-policy", "extensions.json"]
            } else {
                vec![manifest]
            }
        };
        assert_input_error(
            &fake.harness(harness, &args("missing.json")),
            "manifest",
            Some("missing.json"),
        );
        let mut invalid = vec!["{".to_owned(), "{}".into(), "[1]".into()];
        for path in [
            "relative.gd",
            "res://",
            "res://a//b.gd",
            "res://./a.gd",
            "res://../a.gd",
            "res://.godot/a.gd",
            "res://a\\b.gd",
            "res://a\nb.gd",
        ] {
            invalid.push(json!([path]).to_string());
        }
        for manifest in invalid {
            fs::write(fake.dir.path().join("manifest.json"), manifest).unwrap();
            assert_input_error(
                &fake.harness(harness, &args("manifest.json")),
                "manifest",
                Some("manifest.json"),
            );
        }
    }
    fs::write(
        fake.dir.path().join("complex.gd"),
        "class_name Complex\nextends \"res://base.gd\"\n",
    )
    .unwrap();
    let args = ["--import", "--path", fake.dir.path().to_str().unwrap()];
    let result = fake.run(&args);
    assert_eq!(result.status.unwrap().code(), Some(2));
    assert!(stderr(&result).contains("requires explicit class_cache"));
    fs::write(
        &fake.scenario,
        json!({"--import":{"class_cache":"custom"}}).to_string(),
    )
    .unwrap();
    assert!(fake.run(&args).success());
    assert_eq!(fs::read_to_string(fake.cache()).unwrap(), "custom");
}
