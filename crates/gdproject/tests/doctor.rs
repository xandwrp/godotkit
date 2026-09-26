//! Offline acceptance tests for gdproject::doctor, with `fake-godot`.
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gdproject::config::SelectionSource;
use gdproject::doctor::{self, ConfigStatus, DoctorReport, Inputs, ProblemCode};
use gdproject::engine::CacheHealth;
use gdproject::workspace::Workspace;
use serde_json::{Value, json};

const DEADLINE: Duration = Duration::from_secs(5);

struct Fixture {
    dir: tempfile::TempDir,
    engine: PathBuf,
    workspace: Workspace,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("project")).unwrap();
        fs::create_dir_all(dir.path().join("config")).unwrap();
        let engine = dir
            .path()
            .join(if cfg!(windows) { "godot.exe" } else { "godot" });
        copy_engine(Path::new(env!("CARGO_BIN_EXE_fake-godot")), &engine);
        let fixture = Self {
            workspace: {
                fs::write(
                    dir.path().join("project/project.godot"),
                    "config_version=5\n",
                )
                .unwrap();
                Workspace::open(&dir.path().join("project")).unwrap()
            },
            dir,
            engine,
        };
        fixture.scenario(json!({}));
        fixture
    }
    fn root(&self) -> &Path {
        self.workspace.root()
    }
    fn write(&self, path: &str, text: &str) {
        let path = self.root().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    fn global_path(&self) -> PathBuf {
        self.dir.path().join("config/config.toml")
    }
    fn scenario(&self, scenario: Value) {
        let mut path = self.engine.clone().into_os_string();
        path.push(".scenario.json");
        fs::write(PathBuf::from(path), scenario.to_string()).unwrap();
    }
    fn probes(&self) -> usize {
        let mut path = self.engine.clone().into_os_string();
        path.push(".log");
        fs::read_to_string(PathBuf::from(path))
            .unwrap_or_default()
            .lines()
            .filter(|line| line.contains("--script"))
            .count()
    }
    /// Doctor with `explicit` as `--godot`, `env` as `GDKIT_GODOT`, and the fixture's global config.
    fn diagnose(&self, explicit: Option<&Path>, env: Option<&OsString>) -> DoctorReport {
        let global = self.global_path();
        doctor::diagnose(
            &self.workspace,
            &Inputs {
                explicit,
                env,
                global_config: Some(&global),
                probe_deadline: DEADLINE,
            },
        )
    }
}

fn copy_engine(source: &Path, destination: &Path) {
    // A waited child owns the copy's writable descriptor (see tests/engine.rs).
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

fn codes(report: &DoctorReport) -> Vec<ProblemCode> {
    report.problems.iter().map(|problem| problem.code).collect()
}

#[test]
fn healthy_project_probes_once_then_reports_a_cache_hit() {
    let f = Fixture::new();
    let first = f.diagnose(Some(&f.engine), None);
    assert!(first.problems.is_empty(), "{:?}", first.problems);
    assert_eq!(first.project.root, fs::canonicalize(f.root()).unwrap());
    assert!(!first.project.imported);
    assert_eq!(first.config.status, ConfigStatus::Absent);
    assert_eq!(first.global_config.status, ConfigStatus::Absent);
    assert_eq!(first.engine.probe_cache, Some(CacheHealth::Missing));
    assert_eq!(first.engine.cache_hit, Some(false));
    let attached = first.engine.attached.as_ref().unwrap();
    assert_eq!(attached.version, "4.7.2.stable.fake");
    assert_eq!(attached.source, SelectionSource::CommandLine);
    assert_eq!(f.probes(), 1);

    let second = f.diagnose(Some(&f.engine), None);
    assert_eq!(second.engine.probe_cache, Some(CacheHealth::Current));
    assert_eq!(second.engine.cache_hit, Some(true));
    assert_eq!(f.probes(), 1);
    // No API dump or docs run, and nothing written beyond the probe cache.
    let state: Vec<_> = fs::read_dir(f.workspace.state_dir())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name != "lock")
        .collect();
    assert_eq!(state, ["engine-probe.json"]);
    let api = second.api_cache.as_ref().unwrap();
    assert_eq!(api.native, CacheHealth::Missing);
    assert_eq!(api.scripts, None);

    // `imported` follows the class cache.
    f.write(".godot/global_script_class_cache.cfg", "list=[]\n");
    assert!(f.diagnose(Some(&f.engine), None).project.imported);
}

#[test]
fn candidates_list_every_configured_source_in_precedence_order() {
    let f = Fixture::new();
    f.write("gdkit.toml", "[engine]\nexecutable = \"../godot\"\n");
    fs::write(
        f.global_path(),
        "[engine]\nexecutable = \"/elsewhere/godot\"\n",
    )
    .unwrap();
    let env = OsString::from("/from/env/godot");
    let report = f.diagnose(Some(&f.engine), Some(&env));
    let candidates: Vec<(SelectionSource, &Path)> = report
        .engine
        .candidates
        .iter()
        .map(|candidate| (candidate.source, candidate.value.as_path()))
        .collect();
    assert_eq!(
        candidates,
        [
            (SelectionSource::CommandLine, f.engine.as_path()),
            (SelectionSource::Environment, Path::new("/from/env/godot")),
            (SelectionSource::ProjectConfig, Path::new("../godot")),
            (SelectionSource::GlobalConfig, Path::new("/elsewhere/godot")),
        ]
    );
    assert_eq!(report.config.status, ConfigStatus::Valid);
    assert_eq!(report.global_config.status, ConfigStatus::Valid);
    assert!(report.problems.is_empty(), "{:?}", report.problems);

    // Without the flag and with a blank GDKIT_GODOT, gdkit.toml's relative path wins.
    let blank = OsString::from("  ");
    let report = f.diagnose(None, Some(&blank));
    assert_eq!(
        report.engine.candidates[0].source,
        SelectionSource::ProjectConfig
    );
    let selected = report.engine.selected.as_ref().unwrap();
    assert_eq!(selected.source, SelectionSource::ProjectConfig);
    assert_eq!(selected.executable, fs::canonicalize(&f.engine).unwrap());
}

#[test]
fn invalid_configs_are_problems_and_selection_falls_through() {
    let f = Fixture::new();
    f.write(
        "gdkit.toml",
        "[engine]\nexecutable = \"../godot\"\nbogus = 1\n",
    );
    fs::write(f.global_path(), "not toml [").unwrap();
    let report = f.diagnose(Some(&f.engine), None);
    assert_eq!(
        codes(&report),
        [
            ProblemCode::ProjectConfigInvalid,
            ProblemCode::GlobalConfigInvalid
        ]
    );
    assert!(
        report.problems[0].message.contains("engine.bogus"),
        "{:?}",
        report.problems
    );
    assert_eq!(report.config.status, ConfigStatus::Invalid);
    assert_eq!(report.global_config.status, ConfigStatus::Invalid);
    // The flag still selects and probes an engine.
    assert!(report.engine.attached.is_some());

    // With neither config readable and nothing else set, there is no engine.
    let report = f.diagnose(None, None);
    assert_eq!(
        codes(&report),
        [
            ProblemCode::ProjectConfigInvalid,
            ProblemCode::GlobalConfigInvalid,
            ProblemCode::NoEngine
        ]
    );
    assert!(report.api_cache.is_none());

    // An unlocatable global config is not a problem by itself.
    let global = doctor::diagnose(
        &f.workspace,
        &Inputs {
            explicit: Some(&f.engine),
            env: None,
            global_config: None,
            probe_deadline: DEADLINE,
        },
    );
    assert_eq!(global.global_config.status, ConfigStatus::Unlocatable);
    assert_eq!(codes(&global), [ProblemCode::ProjectConfigInvalid]);
}

#[test]
fn missing_engines_and_failed_probes_are_problems_with_the_probe_output() {
    let f = Fixture::new();
    let missing = f.dir.path().join("nope/godot");
    let report = f.diagnose(Some(&missing), None);
    assert_eq!(codes(&report), [ProblemCode::EngineNotFound]);
    assert!(report.engine.selected.is_none());
    assert!(report.engine.probe_cache.is_none());

    f.scenario(json!({"probe": {"mode": "error_envelope", "stderr": "resource failed\n"}}));
    let report = f.diagnose(Some(&f.engine), None);
    assert_eq!(codes(&report), [ProblemCode::ProbeFailed]);
    assert!(report.engine.selected.is_some());
    assert!(report.engine.attached.is_none());
    let output = report.problems[0].output.as_deref().unwrap();
    assert!(output.contains("resource failed"), "{output}");

    f.scenario(
        json!({"probe": {"payload": {"version": "3.6.stable", "major": 3, "editor": true}}}),
    );
    let report = f.diagnose(Some(&f.engine), None);
    assert_eq!(codes(&report), [ProblemCode::ProbeFailed]);
}

#[test]
fn api_cache_health_follows_the_engine_and_the_scripts() {
    let f = Fixture::new();
    f.write("player.gd", "class_name Player\nextends Node\n");
    f.scenario(json!({"--gdscript-docs": {"files": {
        "Player.xml": "<class name=\"Player\" inherits=\"Node\"><brief_description>Hero.</brief_description></class>"
    }}}));
    let report = f.diagnose(Some(&f.engine), None);
    let engine = report.engine.attached.clone().unwrap();
    let api = report.api_cache.unwrap();
    assert_eq!(api.native, CacheHealth::Missing);
    assert_eq!(api.scripts, Some(CacheHealth::Missing));

    // Native: only the key is read, and it must name this engine and schema.
    let native = |fingerprint: &str| {
        let key = json!({"engine_fingerprint": fingerprint, "schema_version": gdview::api::API_INDEX_SCHEMA_VERSION});
        fs::write(
            f.workspace.api_cache_path(),
            json!({"key": key, "index": {}}).to_string(),
        )
        .unwrap();
        f.diagnose(Some(&f.engine), None).api_cache.unwrap().native
    };
    assert_eq!(native(&engine.fingerprint), CacheHealth::Current);
    assert_eq!(native("another engine"), CacheHealth::Stale);
    fs::write(f.workspace.api_cache_path(), "{").unwrap();
    assert_eq!(
        f.diagnose(Some(&f.engine), None).api_cache.unwrap().native,
        CacheHealth::Malformed
    );

    // Scripts: current after a load, stale once a script changes.
    gdproject::api::load_scripts(&f.workspace, &engine, DEADLINE).unwrap();
    let scripts = || f.diagnose(Some(&f.engine), None).api_cache.unwrap().scripts;
    assert_eq!(scripts(), Some(CacheHealth::Current));
    f.write(
        "player.gd",
        "class_name Player\nextends Node\nvar hp := 3\n",
    );
    assert_eq!(scripts(), Some(CacheHealth::Stale));
}

#[test]
fn warning_policy_and_strict_methods_are_reported_and_bad_settings_are_problems() {
    let f = Fixture::new();
    f.write("gdkit.toml", "[check]\nstrict_methods = true\n");
    f.write(
        "project.godot",
        "config_version=5\n[debug]\ngdscript/warnings/exclude_addons=false\ngdscript/warnings/unused_variable=2\n",
    );
    let report = f.diagnose(Some(&f.engine), None);
    assert!(report.problems.is_empty(), "{:?}", report.problems);
    assert!(report.strict_methods);
    let policy = report.warnings.unwrap();
    assert!(policy.enabled);
    assert_eq!(policy.directory_rules[0].path, "res://addons");
    assert_eq!(policy.overrides["unused_variable"], "2");

    f.write(
        "project.godot",
        "config_version=5\n[debug]\ngdscript/warnings/enable=maybe\n",
    );
    let report = f.diagnose(Some(&f.engine), None);
    assert_eq!(codes(&report), [ProblemCode::WarningSettingsInvalid]);
    assert!(report.warnings.is_none());
}

#[test]
fn check_artifacts_report_the_count_and_the_newest_run() {
    let f = Fixture::new();
    let empty = f.diagnose(Some(&f.engine), None).check_artifacts;
    assert_eq!(
        (empty.runs, empty.latest, empty.latest_unix_ms),
        (0, None, None)
    );

    let older = f.workspace.new_artifact_dir("check").unwrap().path;
    let newer = f.workspace.new_artifact_dir("check").unwrap().path;
    // Other kinds and stray files are not check runs.
    f.workspace.new_artifact_dir("run").unwrap();
    fs::write(older.parent().unwrap().join("stray.txt"), "").unwrap();
    let artifacts = f.diagnose(Some(&f.engine), None).check_artifacts;
    assert_eq!(artifacts.runs, 2);
    assert_eq!(artifacts.latest.as_deref(), Some(newer.as_path()));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let created = artifacts.latest_unix_ms.unwrap();
    assert!(
        created <= now && now - created < 60_000,
        "{created} vs {now}"
    );
}
