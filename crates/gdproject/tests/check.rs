// Acceptance tests for gdproject::check. Offline unless prefixed real_engine_.
#![allow(unused)]

use std::fs;
use std::path::{Path, PathBuf};

use gdproject::Workspace;
use gdproject::check::{
    self, CheckReport, CheckRequest, NoObserver, Outcome, PhaseKind, PhaseOutcome,
};
use gdproject::config::SelectionSource;
use gdproject::diagnostics::Severity;
use gdproject::engine::Engine;

fn write(root: &Path, files: &[(&str, &str)]) {
    for (path, contents) in files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
}

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), &[("project.godot", "config_version=5\n")]);
    write(dir.path(), files);
    dir
}

fn static_check(root: &Path, request: CheckRequest) -> gdproject::Result<CheckReport> {
    let workspace = Workspace::open(root).unwrap();
    check::run(
        &workspace,
        None,
        &CheckRequest {
            static_only: true,
            ..request
        },
        &mut NoObserver,
    )
}

const BROKEN_SCENE: &str = "[gd_scene format=3]\n\n[ext_resource type=\"Script\" path=\"res://gone.gd\" id=\"1\"]\n\n[node name=\"Main\" type=\"Node\"]\nscript = ExtResource(\"1\")\n";

#[test]
fn static_only_needs_no_engine_and_slice_keeps_findings_located_in_it() {
    let dir = project(&[
        ("scenes/main.tscn", BROKEN_SCENE),
        ("scripts/ok.gd", "extends Node\n"),
        (
            "scripts/bad.gd",
            "extends Node\nconst X = preload(\"res://nope.tscn\")\n",
        ),
    ]);
    let report = static_check(dir.path(), CheckRequest::default()).unwrap();
    assert_eq!(report.outcome, Outcome::Failed);
    assert!(report.engine.is_none() && report.artifact_dir.is_none());
    assert_eq!(report.phases.len(), 1);
    let phase = &report.phases[0];
    assert_eq!(
        (phase.id.kind, phase.outcome),
        (PhaseKind::StaticAnalysis, PhaseOutcome::Failed)
    );
    let located: Vec<_> = phase
        .diagnostics
        .iter()
        .map(|d| (d.resource.clone().unwrap(), d.line.unwrap()))
        .collect();
    assert_eq!(
        located,
        [
            ("res://scenes/main.tscn".to_string(), 3),
            ("res://scripts/bad.gd".to_string(), 2)
        ]
    );
    let diagnostic = &phase.diagnostics[0];
    assert_eq!(diagnostic.severity, Severity::Error);
    assert_eq!(diagnostic.code.as_deref(), Some("GDKIT_MISSING_RESOURCE"));
    assert_eq!(report.failures.len(), 2);
    assert!(
        report.failures[0]
            .message
            .starts_with("res://scenes/main.tscn:3: "),
        "{}",
        report.failures[0].message
    );
    let counts = report.counts.as_ref().unwrap();
    assert_eq!(
        (
            counts.scripts,
            counts.scenes,
            counts.resources,
            counts.static_findings
        ),
        (2, 1, 0, 2)
    );

    let sliced = static_check(
        dir.path(),
        CheckRequest {
            slice: vec![PathBuf::from("scripts")],
            ..CheckRequest::default()
        },
    )
    .unwrap();
    assert!(sliced.project.sliced);
    assert_eq!(sliced.phases[0].diagnostics.len(), 1);
    assert_eq!(
        sliced.phases[0].diagnostics[0].resource.as_deref(),
        Some("res://scripts/bad.gd")
    );
    assert_eq!(sliced.counts.as_ref().unwrap().scripts, 2);

    let clean = static_check(
        dir.path(),
        CheckRequest {
            slice: vec![PathBuf::from("scripts/ok.gd")],
            ..CheckRequest::default()
        },
    )
    .unwrap();
    assert_eq!(clean.outcome, Outcome::Passed);
    assert_eq!(clean.phases[0].outcome, PhaseOutcome::Completed);
    assert!(clean.failures.is_empty());
}

#[test]
fn slice_paths_that_escape_the_project_are_tool_errors() {
    let dir = project(&[]);
    for bad in ["../outside", "/abs/path", ".godot/imported", "a/../../b"] {
        let result = static_check(
            dir.path(),
            CheckRequest {
                slice: vec![PathBuf::from(bad)],
                ..CheckRequest::default()
            },
        );
        assert!(
            matches!(result, Err(gdproject::Error::Invalid(_))),
            "{bad}: {result:?}"
        );
    }
}

#[cfg(feature = "test-engine")]
fn fake(scenario: serde_json::Value) -> (tempfile::TempDir, Engine) {
    let dir = tempfile::tempdir().unwrap();
    let executable = dir.path().join("godot");
    // Keep writable executable descriptors out of the multithreaded test process:
    // another test's fork can inherit them until exec and cause ETXTBSY. A waited
    // child owns the copy's writable descriptor instead, as in the runner fixtures.
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
    fs::write(dir.path().join("godot.scenario.json"), scenario.to_string()).unwrap();
    let engine = Engine {
        executable,
        version: "4.7.2.fake".into(),
        fingerprint: "fake".into(),
        source: SelectionSource::CommandLine,
    };
    (dir, engine)
}

fn engine_check(root: &Path, engine: &Engine, request: CheckRequest) -> CheckReport {
    check::run(
        &Workspace::open(root).unwrap(),
        Some(engine),
        &request,
        &mut NoObserver,
    )
    .unwrap()
}

fn phase(report: &CheckReport, id: &str) -> PhaseOutcome {
    report
        .phases
        .iter()
        .find(|p| p.id.id == id)
        .unwrap()
        .outcome
}

#[test]
#[cfg(feature = "test-engine")]
fn passes_when_every_phase_completes_without_errors() {
    let dir = project(&[
        ("base.gd", "class_name Base\nextends RefCounted\n"),
        ("t.gd", "extends SceneTree\n"),
    ]);
    let (_fake, engine) = fake(serde_json::json!({}));
    let report = engine_check(
        dir.path(),
        &engine,
        CheckRequest {
            scripts: vec![gdview::ResPath::parse("res://t.gd").unwrap()],
            ..CheckRequest::default()
        },
    );
    assert_eq!(report.outcome, Outcome::Passed, "{report:#?}");
    assert_eq!(report.phases.len(), 6);
    assert!(
        report
            .phases
            .iter()
            .all(|p| p.outcome == PhaseOutcome::Completed)
    );
    assert_eq!(report.counts.unwrap().project_scripts_run, 1);
    assert!(
        !dir.path()
            .join(".godot/global_script_class_cache.cfg")
            .exists()
    );
}

#[test]
#[cfg(feature = "test-engine")]
fn zero_exit_script_error_on_either_stream_fails_resource_loading() {
    for stream in ["stdout", "stderr"] {
        let dir = project(&[]);
        let mut scenario = serde_json::json!({"check": {}});
        scenario["check"][stream] = "SCRIPT ERROR: bad call\n   at: test (res://a.gd:7)\n".into();
        let (_fake, engine) = fake(scenario);
        let report = engine_check(dir.path(), &engine, CheckRequest::default());
        assert_eq!(report.outcome, Outcome::Failed);
        assert_eq!(phase(&report, "resource_loading"), PhaseOutcome::Failed);
        assert_eq!(report.phases.last().unwrap().diagnostics[0].line, Some(7));
    }
}

#[test]
#[cfg(feature = "test-engine")]
fn import_errors_fail_before_resource_loading_and_skip_runtime_phases() {
    for stage in ["--import", "import_scan"] {
        let dir = project(&[]);
        let mut scenario = serde_json::json!({});
        scenario[stage] = serde_json::json!({"stderr":"ERROR: import failed\n"});
        let (_fake, engine) = fake(scenario);
        let report = engine_check(dir.path(), &engine, CheckRequest::default());
        assert_eq!(report.outcome, Outcome::Failed);
        assert_eq!(phase(&report, "resource_loading"), PhaseOutcome::Skipped);
        assert_eq!(phase(&report, "class_cache_audit"), PhaseOutcome::Skipped);
    }
}

#[test]
#[cfg(feature = "test-engine")]
fn missing_completion_marker_is_incomplete_not_passed() {
    for behavior in [
        serde_json::json!({"mode":"no_envelope"}),
        serde_json::json!({"mode":"crash"}),
        serde_json::json!({"mode":"no_envelope","stdout":"GDKIT_RESULT:broken\n"}),
        serde_json::json!({"payload": {"counts":{},"failures":[]}}),
    ] {
        let dir = project(&[]);
        let (_fake, engine) = fake(serde_json::json!({"check":behavior}));
        let report = engine_check(dir.path(), &engine, CheckRequest::default());
        assert_eq!(report.outcome, Outcome::Incomplete, "{report:#?}");
        assert_eq!(report.outcome.exit_code(), 1);
        assert!(
            report
                .phases
                .last()
                .unwrap()
                .artifacts
                .iter()
                .all(|p| p.is_file())
        );
    }
}

#[test]
#[cfg(feature = "test-engine")]
fn ignore_rules_suppress_and_are_counted_in_policy() {
    let dir = project(&[(
        "gdkit.toml",
        "[check]\nstrict_methods=true\n[[check.ignore_import_errors]]\nmessage='ERROR: vendor noise'\nsource='res://vendor.gd'\n",
    )]);
    let noise = "ERROR: vendor noise\n   at: test (res://vendor.gd:1)\n";
    let (_fake, engine) =
        fake(serde_json::json!({"--import":{"stderr":noise},"import_scan":{"stdout":noise}}));
    let report = engine_check(dir.path(), &engine, CheckRequest::default());
    assert_eq!(report.outcome, Outcome::Passed);
    assert_eq!(report.suppressed_diagnostics, 2);
    assert_eq!(report.policy.ignore_rules, 1);
    assert!(report.policy.strict_methods);
    let (_fake, engine) = fake(serde_json::json!({"check":{"stderr":noise}}));
    let report = engine_check(
        dir.path(),
        &engine,
        CheckRequest {
            strict_methods: Some(false),
            ..CheckRequest::default()
        },
    );
    assert_eq!(report.outcome, Outcome::Failed);
    assert_eq!(report.suppressed_diagnostics, 0);
    assert!(!report.policy.strict_methods);
}

#[test]
#[cfg(feature = "test-engine")]
fn class_cache_audit_reports_missing_moved_and_stale_entries() {
    for cache in [
        "list=[]",
        "list=[{\"class\": &\"Live\", \"path\": \"res://old.gd\"}]",
        "list=[{\"class\": &\"Dead\", \"path\": \"res://dead.gd\"}]",
        "not a cache",
    ] {
        let dir = project(&[("live.gd", "class_name Live\nextends Node\n")]);
        let (_fake, engine) = fake(serde_json::json!({"import_scan":{"class_cache":cache}}));
        let report = engine_check(dir.path(), &engine, CheckRequest::default());
        assert_eq!(report.outcome, Outcome::Failed, "{report:#?}");
        assert_eq!(phase(&report, "class_cache_audit"), PhaseOutcome::Failed);
        assert_eq!(phase(&report, "resource_loading"), PhaseOutcome::Skipped);
        assert!(
            report
                .failures
                .iter()
                .any(|f| f.kind == check::FailureKind::ClassCache)
        );
    }
}

#[test]
#[cfg(feature = "test-engine")]
fn slice_builds_minimal_project_and_rejects_bad_paths() {
    let dir = project(&[
        ("selected/a.gd", "extends Node\n"),
        ("outside.gd", "class_name Outside\nextends Node\n"),
    ]);
    let (fake_dir, engine) = fake(serde_json::json!({}));
    let report = engine_check(
        dir.path(),
        &engine,
        CheckRequest {
            slice: vec!["selected".into()],
            ..CheckRequest::default()
        },
    );
    assert_eq!(report.outcome, Outcome::Passed);
    let manifest: Vec<String> = serde_json::from_slice(
        &fs::read(report.artifact_dir.unwrap().join("manifest.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest, ["res://project.godot", "res://selected/a.gd"]);
    let log = fs::read_to_string(fake_dir.path().join("godot.log")).unwrap();
    let args: Vec<String> = serde_json::from_str(log.lines().next().unwrap()).unwrap();
    let copy = PathBuf::from(&args[args.iter().position(|a| a == "--path").unwrap() + 1]);
    assert_ne!(copy, dir.path());
    assert!(!copy.exists(), "scratch copy must be removed");
    slice_paths_that_escape_the_project_are_tool_errors();
}

#[test]
#[cfg(feature = "test-engine")]
fn project_script_timeout_is_recorded_as_timeout_and_stops_further_runtime_phases() {
    let dir = project(&[]);
    let (_fake, engine) =
        fake(serde_json::json!({"script_bootstrap":{"mode":"hang","stdout":"partial\n"}}));
    let report = engine_check(
        dir.path(),
        &engine,
        CheckRequest {
            scripts: vec![
                gdview::ResPath::parse("res://a.gd").unwrap(),
                gdview::ResPath::parse("res://b.gd").unwrap(),
            ],
            script_deadline: std::time::Duration::from_millis(100),
            ..CheckRequest::default()
        },
    );
    assert_eq!(report.outcome, Outcome::Incomplete);
    assert_eq!(
        phase(&report, "project_script:0:res://a.gd"),
        PhaseOutcome::TimedOut
    );
    assert_eq!(
        phase(&report, "project_script:1:res://b.gd"),
        PhaseOutcome::Skipped
    );
    assert_eq!(report.counts.unwrap().project_scripts_run, 1);
}

#[test]
fn static_findings_fail_the_check_before_any_engine_phase_runs() {
    let dir = project(&[("main.tscn", BROKEN_SCENE)]);
    // An engine that cannot run: if any engine phase started, this would error.
    let engine = Engine {
        executable: dir.path().join("no-such-godot"),
        version: "4.x".into(),
        fingerprint: "none".into(),
        source: SelectionSource::CommandLine,
    };
    let request = CheckRequest {
        scripts: vec![gdview::ResPath::parse("res://t.gd").unwrap()],
        ..CheckRequest::default()
    };
    let workspace = Workspace::open(dir.path()).unwrap();
    let report = check::run(&workspace, Some(&engine), &request, &mut NoObserver).unwrap();
    assert_eq!(report.outcome, Outcome::Failed);
    assert_eq!(
        report.engine.as_ref().unwrap().executable,
        engine.executable
    );
    let phases: Vec<_> = report
        .phases
        .iter()
        .map(|p| (p.id.id.as_str(), p.outcome))
        .collect();
    assert_eq!(
        phases,
        [
            ("static_analysis", PhaseOutcome::Failed),
            ("import", PhaseOutcome::Skipped),
            ("resource_loading", PhaseOutcome::Skipped),
            ("project_script:0:res://t.gd", PhaseOutcome::Skipped),
        ]
    );
    assert!(
        report.phases[1..]
            .iter()
            .all(|p| p.skipped_reason.as_deref() == Some("static analysis failed"))
    );
    // Without an engine, a full check is a tool error, not a report.
    assert!(matches!(
        check::run(&workspace, None, &CheckRequest::default(), &mut NoObserver),
        Err(gdproject::Error::NoEngine)
    ));
}

#[test]
fn baseline_classifies_new_carried_and_resolved_by_identity_not_line() {
    let dir = project(&[(
        "a.gd",
        "extends Node\nconst KEEP = preload(\"res://keep_missing.tscn\")\nconst FIX = preload(\"res://fixed_later.tscn\")\n",
    )]);
    let before = static_check(dir.path(), CheckRequest::default()).unwrap();
    assert_eq!(before.phases[0].diagnostics.len(), 2);
    // Same missing KEEP two lines lower, FIX fixed, one new problem.
    write(
        dir.path(),
        &[(
            "a.gd",
            "extends Node\n\n\nconst KEEP = preload(\"res://keep_missing.tscn\")\nconst NEW = preload(\"res://new_missing.tscn\")\n",
        )],
    );
    let after = static_check(
        dir.path(),
        CheckRequest {
            baseline: Some(before.clone()),
            ..CheckRequest::default()
        },
    )
    .unwrap();
    let baseline = after.baseline.as_ref().unwrap();
    assert_eq!(baseline.baseline_fingerprint, before.project.fingerprint);
    assert_ne!(after.project.fingerprint, before.project.fingerprint);
    let targets = |diagnostics: &[gdproject::diagnostics::Diagnostic]| {
        diagnostics
            .iter()
            .map(|d| (d.message.clone(), d.line.unwrap()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        targets(&baseline.new),
        [(
            "preload of res://new_missing.tscn, which does not exist".to_string(),
            5
        )]
    );
    assert_eq!(
        targets(&baseline.carried),
        [(
            "preload of res://keep_missing.tscn, which does not exist".to_string(),
            4
        )]
    );
    assert_eq!(
        targets(&baseline.resolved),
        [(
            "preload of res://fixed_later.tscn, which does not exist".to_string(),
            3
        )]
    );
}

#[test]
#[ignore = "deferred: check orchestration does not load an API index; API-cache enrichment is a separate workstream"]
fn suggestions_are_attached_only_when_an_api_index_is_cached() {
    panic!("API-cache enrichment is intentionally not implemented or claimed by check");
}

#[test]
#[cfg(feature = "test-engine")]
fn artifacts_hold_raw_streams_and_event_log_for_every_phase() {
    let dir = project(&[]);
    let (_fake, engine) = fake(
        serde_json::json!({"--import":{"stdout":"raw out\n","stderr":"raw err\n"},"check":{"mode":"no_envelope","stdout":"partial"}}),
    );
    let report = engine_check(dir.path(), &engine, CheckRequest::default());
    assert_eq!(report.outcome, Outcome::Incomplete);
    for phase in report.phases.iter().filter(|p| p.process_pid.is_some()) {
        assert_eq!(phase.artifacts.len(), 3);
        let events: Vec<serde_json::Value> =
            serde_json::from_slice(&fs::read(&phase.artifacts[2]).unwrap()).unwrap();
        for (stream, path) in [
            ("stdout", &phase.artifacts[0]),
            ("stderr", &phase.artifacts[1]),
        ] {
            let bytes: Vec<u8> = events
                .iter()
                .filter(|e| e["stream"] == stream)
                .flat_map(|e| {
                    e["bytes"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|n| n.as_u64().unwrap() as u8)
                })
                .collect();
            assert_eq!(bytes, fs::read(path).unwrap());
        }
    }
    assert_eq!(
        fs::read(&report.phases[1].artifacts[0]).unwrap(),
        b"raw out\n"
    );
    assert_eq!(
        fs::read(&report.phases.last().unwrap().artifacts[0]).unwrap(),
        b"partial"
    );
}

#[test]
fn report_json_round_trips_and_exit_mapping_is_0_1_2() {
    let dir = project(&[("main.tscn", BROKEN_SCENE), ("ok.gd", "extends Node\n")]);
    let failed = static_check(dir.path(), CheckRequest::default()).unwrap();
    let json = serde_json::to_string(&failed).unwrap();
    assert_eq!(serde_json::from_str::<CheckReport>(&json).unwrap(), failed);
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["schema_version"], check::CHECK_REPORT_SCHEMA_VERSION);
    assert_eq!(value["outcome"], "failed");
    assert_eq!(value["phases"][0]["id"]["kind"], "static_analysis");
    assert_eq!(failed.outcome.exit_code(), 1);
    let passed = static_check(
        dir.path(),
        CheckRequest {
            slice: vec![PathBuf::from("ok.gd")],
            ..CheckRequest::default()
        },
    )
    .unwrap();
    assert_eq!(passed.outcome.exit_code(), 0);
    assert_eq!(Outcome::Incomplete.exit_code(), 1);
    // Exit 2 is a tool failure: an `Err`, never a report.
    let not_a_project = tempfile::tempdir().unwrap();
    assert!(Workspace::open(not_a_project.path()).is_err());
}

#[test]
fn static_only_rejects_project_scripts_at_api_boundary() {
    let dir = project(&[]);
    let workspace = Workspace::open(dir.path()).unwrap();
    let engine = Engine {
        executable: dir.path().join("must-not-run"),
        version: "test".into(),
        fingerprint: "test".into(),
        source: SelectionSource::CommandLine,
    };
    let request = CheckRequest {
        static_only: true,
        scripts: vec![gdview::ResPath::parse("res://test.gd").unwrap()],
        ..CheckRequest::default()
    };
    for engine in [None, Some(&engine)] {
        let result = check::run(&workspace, engine, &request, &mut NoObserver);
        assert!(
            matches!(result, Err(gdproject::Error::Invalid(ref message)) if message.contains("static-only") && message.contains("scripts")),
            "{result:?}"
        );
    }
}

#[test]
fn carried_static_findings_retain_static_only_failure_verdict() {
    let dir = project(&[("main.tscn", BROKEN_SCENE)]);
    let baseline = static_check(dir.path(), CheckRequest::default()).unwrap();
    let report = static_check(
        dir.path(),
        CheckRequest {
            baseline: Some(baseline.clone()),
            ..CheckRequest::default()
        },
    )
    .unwrap();
    assert_eq!(report.outcome, Outcome::Failed);
    assert_eq!(report.failures, baseline.failures);
    assert_eq!(report.phases[0].outcome, PhaseOutcome::Failed);
    let comparison = report.baseline.unwrap();
    assert_eq!(comparison.carried.len(), 1);
    assert!(comparison.new.is_empty());
}

#[test]
#[cfg(feature = "test-engine")]
fn carried_static_findings_allow_engine_but_new_findings_block() {
    let dir = project(&[(
        "a.gd",
        "extends Node\nconst A = preload(\"res://missing.tscn\")\n",
    )]);
    let baseline = static_check(dir.path(), CheckRequest::default()).unwrap();
    let (fake_dir, engine) = fake(serde_json::json!({}));
    let report = engine_check(
        dir.path(),
        &engine,
        CheckRequest {
            baseline: Some(baseline.clone()),
            scripts: vec![gdview::ResPath::parse("res://test.gd").unwrap()],
            ..CheckRequest::default()
        },
    );
    assert_eq!(report.outcome, Outcome::Failed);
    assert_eq!(report.outcome.exit_code(), 1);
    assert_eq!(report.failures, baseline.failures);
    assert_eq!(report.phases[0].outcome, PhaseOutcome::Failed);
    assert_eq!(report.phases.len(), 6);
    assert!(
        report.phases[1..]
            .iter()
            .all(|p| p.outcome == PhaseOutcome::Completed)
    );
    assert_eq!(report.counts.as_ref().unwrap().project_scripts_run, 1);
    assert_eq!(report.baseline.unwrap().carried.len(), 1);
    assert!(fake_dir.path().join("godot.log").exists());
    write(
        dir.path(),
        &[(
            "b.gd",
            "extends Node\nconst B = preload(\"res://other.tscn\")\n",
        )],
    );
    let (fake_dir, engine) = fake(serde_json::json!({}));
    let report = engine_check(
        dir.path(),
        &engine,
        CheckRequest {
            baseline: Some(baseline),
            ..CheckRequest::default()
        },
    );
    assert_eq!(report.outcome, Outcome::Failed);
    assert_eq!(report.failures.len(), 2);
    assert!(
        report
            .failures
            .iter()
            .all(|f| f.kind == check::FailureKind::StaticFinding)
    );
    assert!(
        report.phases[1..]
            .iter()
            .all(|p| p.outcome == PhaseOutcome::Skipped)
    );
    let comparison = report.baseline.unwrap();
    assert_eq!(comparison.carried.len(), 1);
    assert_eq!(comparison.new.len(), 1);
    assert!(!fake_dir.path().join("godot.log").exists());
}

#[test]
#[cfg(feature = "test-engine")]
fn check_exit_one_with_failure_payload_is_a_report_not_tool_error() {
    let dir = project(&[("a.gd", "extends Node\n")]);
    let (_fake, engine) = fake(
        serde_json::json!({"import_scan":{"payload":{"scanned":1,"recognized_extensions":["gd"]}},"check":{"exit":1,"payload":{"counts":{"scripts":1,"scenes":0,"resources":0},"failures":["res://a.gd"],"recognized_extensions":["gd"]}}}),
    );
    let report = engine_check(dir.path(), &engine, CheckRequest::default());
    assert_eq!(report.outcome, Outcome::Failed);
    assert!(
        report
            .failures
            .iter()
            .any(|f| f.kind == check::FailureKind::ResourceLoad)
    );
}

#[test]
#[cfg(feature = "test-engine")]
fn script_handoff_requires_exact_single_marker_clean_streams_and_exit() {
    for (behavior, outcome) in [
        (
            serde_json::json!({"mode":"no_envelope","stdout":" GDKIT_SCRIPT_STARTED\n"}),
            Outcome::Incomplete,
        ),
        (
            serde_json::json!({"stdout":"GDKIT_SCRIPT_STARTED\n"}),
            Outcome::Incomplete,
        ),
        (serde_json::json!({"exit":7}), Outcome::Failed),
        (
            serde_json::json!({"stderr":"SCRIPT ERROR: after handoff\n"}),
            Outcome::Failed,
        ),
        (
            serde_json::json!({"mode":"error_envelope","payload":{"stage":"base","message":"not SceneTree","field":"res://a.gd"}}),
            Outcome::Failed,
        ),
    ] {
        let dir = project(&[]);
        let (_fake, engine) = fake(serde_json::json!({"script_bootstrap":behavior}));
        let report = engine_check(
            dir.path(),
            &engine,
            CheckRequest {
                scripts: vec![
                    gdview::ResPath::parse("res://a.gd").unwrap(),
                    gdview::ResPath::parse("res://b.gd").unwrap(),
                ],
                ..CheckRequest::default()
            },
        );
        assert_eq!(report.outcome, outcome, "{report:#?}");
        assert_eq!(report.phases.last().unwrap().outcome, PhaseOutcome::Skipped);
    }
}

#[test]
#[cfg(feature = "test-engine")]
fn diagnostics_after_completion_and_duplicate_envelopes_are_not_lost() {
    let completion = "GDKIT_RESULT:{\"protocol\":1,\"harness\":\"check\",\"ok\":true,\"payload\":{\"counts\":{\"scripts\":0,\"scenes\":0,\"resources\":0},\"failures\":[],\"recognized_extensions\":[]},\"error\":null}\n";
    for (output, outcome) in [
        (format!("{completion}ERROR: late error\n"), Outcome::Failed),
        (format!("{completion}{completion}"), Outcome::Incomplete),
    ] {
        let dir = project(&[]);
        let (_fake, engine) = fake(
            serde_json::json!({"import_scan":{"payload":{"scanned":0,"recognized_extensions":[]}},"check":{"mode":"no_envelope","stdout":output}}),
        );
        let report = engine_check(dir.path(), &engine, CheckRequest::default());
        assert_eq!(report.outcome, outcome);
    }
}

#[test]
#[cfg(feature = "test-engine")]
fn output_limit_is_explicit_incomplete_failure_with_retained_artifacts() {
    let dir = project(&[]);
    let (_fake, engine) =
        fake(serde_json::json!({"check":{"stdout":"x".repeat(20 * 1024 * 1024)}}));
    let report = engine_check(dir.path(), &engine, CheckRequest::default());
    assert_eq!(report.outcome, Outcome::Incomplete);
    assert!(
        report
            .failures
            .iter()
            .any(|f| f.kind == check::FailureKind::OutputLimit)
    );
    assert_eq!(report.phases.last().unwrap().artifacts.len(), 3);
}

#[test]
#[cfg(feature = "test-engine")]
fn full_inventory_uses_editor_handoff_and_runtime_union_without_loading_unknown_files() {
    let dir = project(&[
        ("pixel.BMP", "fake image"),
        ("shared.gdshaderinc", "const float VALUE = 1.0;\n"),
        ("custom.special", "custom resource"),
        ("notes.unknown", "not a resource"),
        ("metadata.json", "{}"),
        (".godot/gdkit/artifacts/old.json", "must not load"),
        ("excluded/.gdignore", ""),
        ("excluded/hidden.bmp", "must not load"),
    ]);
    let editor = serde_json::json!(["bmp", "gdshaderinc", "json"]);
    let runtime_union = serde_json::json!(["bmp", "gdshaderinc", "json", "special"]);
    let (fake_dir, engine) = fake(serde_json::json!({
        "import_scan":{"payload":{"scanned":0,"recognized_extensions":editor}},
        "check":{"payload":{"counts":{"scripts":0,"scenes":0,"resources":4},"failures":[],"recognized_extensions":runtime_union}}
    }));
    let report = engine_check(dir.path(), &engine, CheckRequest::default());
    assert_eq!(report.outcome, Outcome::Passed, "{report:#?}");
    assert_eq!(report.counts.as_ref().unwrap().resources, 4);
    let artifacts = report.artifact_dir.unwrap();
    let inventory: Vec<String> =
        serde_json::from_slice(&fs::read(artifacts.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(
        inventory,
        [
            "res://custom.special",
            "res://metadata.json",
            "res://notes.unknown",
            "res://pixel.BMP",
            "res://project.godot",
            "res://shared.gdshaderinc"
        ]
    );
    let extensions_file = artifacts.join("editor-extensions.json");
    let saved: serde_json::Value =
        serde_json::from_slice(&fs::read(&extensions_file).unwrap()).unwrap();
    assert_eq!(saved, editor);
    let invocations: Vec<Vec<String>> = fs::read_to_string(fake_dir.path().join("godot.log"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let scan = &invocations[1];
    assert_eq!(
        scan.len() - scan.iter().position(|a| a == "--").unwrap() - 1,
        1
    );
    let check = &invocations[2];
    let args = &check[check.iter().position(|a| a == "--").unwrap() + 1..];
    assert_eq!(
        args,
        [
            artifacts.join("manifest.json").to_str().unwrap(),
            "project-policy",
            extensions_file.to_str().unwrap()
        ]
    );
}

#[test]
#[cfg(feature = "test-engine")]
fn default_fake_registry_loads_bmp_and_shader_include_but_not_unknown_files() {
    let dir = project(&[
        ("pixel.BMP", "fake"),
        ("shared.gdshaderinc", "fake"),
        ("notes.unknown", "ignored"),
    ]);
    let (_fake, engine) = fake(serde_json::json!({}));
    let report = engine_check(dir.path(), &engine, CheckRequest::default());
    assert_eq!(report.outcome, Outcome::Passed, "{report:#?}");
    assert_eq!(report.counts.unwrap().resources, 2);
}

#[test]
#[cfg(feature = "test-engine")]
fn invalid_extension_metadata_and_invalid_eligible_counts_are_incomplete() {
    let invalid_sets = [
        serde_json::Value::Null,
        serde_json::json!([1]),
        serde_json::json!([""]),
        serde_json::json!([".bmp"]),
        serde_json::json!(["a/b"]),
        serde_json::json!(["a\\b"]),
        serde_json::json!(["a:b"]),
        serde_json::json!(["a b"]),
        serde_json::json!(["a\nb"]),
        serde_json::json!(["BMP"]),
        serde_json::json!(["bmp", "bmp"]),
        serde_json::json!(["gd", "bmp"]),
    ];
    for stage in ["import_scan", "check"] {
        for extensions in &invalid_sets {
            let dir = project(&[]);
            let mut scenario = serde_json::json!({"import_scan":{"payload":{"scanned":0,"recognized_extensions":[]}}});
            scenario[stage] = if stage == "import_scan" {
                serde_json::json!({"payload":{"scanned":0,"recognized_extensions":extensions}})
            } else {
                serde_json::json!({"payload":{"counts":{"scripts":0,"scenes":0,"resources":0},"failures":[],"recognized_extensions":extensions}})
            };
            let (_fake, engine) = fake(scenario);
            let report = engine_check(dir.path(), &engine, CheckRequest::default());
            assert_eq!(
                report.outcome,
                Outcome::Incomplete,
                "{stage}: {extensions}: {report:#?}"
            );
            assert!(
                report
                    .phases
                    .iter()
                    .find(
                        |p| p.id.id == stage || (stage == "check" && p.id.id == "resource_loading")
                    )
                    .unwrap()
                    .artifacts
                    .iter()
                    .all(|p| p.is_file())
            );
        }
    }
    // All paths are in the manifest, but only bmp is eligible. Reject unknown
    // failure paths, overcounts, and a runtime set that loses editor capabilities.
    for payload in [
        serde_json::json!({"counts":{"scripts":0,"scenes":0,"resources":1},"failures":["res://notes.unknown"],"recognized_extensions":["bmp"]}),
        serde_json::json!({"counts":{"scripts":0,"scenes":0,"resources":2},"failures":[],"recognized_extensions":["bmp"]}),
        serde_json::json!({"counts":{"scripts":0,"scenes":0,"resources":0},"failures":[],"recognized_extensions":[]}),
        serde_json::json!({"counts":{"scripts":0,"scenes":0,"resources":1},"failures":[]}),
    ] {
        let dir = project(&[("pixel.bmp", "fake"), ("notes.unknown", "ignored")]);
        let (_fake, engine) = fake(
            serde_json::json!({"import_scan":{"payload":{"scanned":0,"recognized_extensions":["bmp"]}},"check":{"payload":payload}}),
        );
        let report = engine_check(dir.path(), &engine, CheckRequest::default());
        assert_eq!(report.outcome, Outcome::Incomplete, "{report:#?}");
    }
}

#[test]
#[cfg(feature = "test-engine")]
fn recognized_custom_failure_is_failed_not_malformed_completion() {
    let dir = project(&[("custom.special", "broken")]);
    let (_fake, engine) = fake(
        serde_json::json!({"import_scan":{"payload":{"scanned":0,"recognized_extensions":[]}},"check":{"exit":1,"payload":{"counts":{"scripts":0,"scenes":0,"resources":1},"failures":["res://custom.special"],"recognized_extensions":["special"]}}}),
    );
    let report = engine_check(dir.path(), &engine, CheckRequest::default());
    assert_eq!(report.outcome, Outcome::Failed);
    assert!(report.failures.iter().any(
        |f| f.kind == check::FailureKind::ResourceLoad && f.message.contains("custom.special")
    ));
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT"]
fn real_engine_bmp_shader_include_and_unknown_inventory() {
    let dir = project(&[
        ("shared.gdshaderinc", "const float VALUE = 1.0;\n"),
        ("notes.unknown", "not a resource"),
    ]);
    // One 24-bit BGR pixel, padded to a four-byte BMP scanline.
    let mut bmp = vec![0_u8; 58];
    bmp[0..2].copy_from_slice(b"BM");
    bmp[2..6].copy_from_slice(&58_u32.to_le_bytes());
    bmp[10..14].copy_from_slice(&54_u32.to_le_bytes());
    bmp[14..18].copy_from_slice(&40_u32.to_le_bytes());
    bmp[18..22].copy_from_slice(&1_u32.to_le_bytes());
    bmp[22..26].copy_from_slice(&1_u32.to_le_bytes());
    bmp[26..28].copy_from_slice(&1_u16.to_le_bytes());
    bmp[28..30].copy_from_slice(&24_u16.to_le_bytes());
    bmp[34..38].copy_from_slice(&4_u32.to_le_bytes());
    bmp[56] = 255;
    fs::write(dir.path().join("pixel.BMP"), bmp).unwrap();
    let report = engine_check(dir.path(), &real_engine(), CheckRequest::default());
    assert_eq!(report.outcome, Outcome::Passed, "{report:#?}");
    assert_eq!(report.counts.unwrap().resources, 2);
    let extensions: Vec<String> = serde_json::from_slice(
        &fs::read(report.artifact_dir.unwrap().join("editor-extensions.json")).unwrap(),
    )
    .unwrap();
    assert!(extensions.iter().any(|e| e == "bmp"));
    assert!(extensions.iter().any(|e| e == "gdshaderinc"));
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT"]
fn real_engine_corrupt_bmp_zero_exit_import_error_stops_pipeline() {
    let dir = project(&[("broken.bmp", "not a bitmap")]);
    let report = engine_check(dir.path(), &real_engine(), CheckRequest::default());
    assert_eq!(report.outcome, Outcome::Failed, "{report:#?}");
    assert_eq!(report.phases[1].exit_status, Some(0));
    assert_eq!(phase(&report, "import"), PhaseOutcome::Failed);
    assert_eq!(phase(&report, "import_scan"), PhaseOutcome::Skipped);
    assert_eq!(phase(&report, "resource_loading"), PhaseOutcome::Skipped);
    assert!(
        report.phases[1]
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error && d.message.contains("broken.bmp"))
    );
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT"]
fn real_engine_runtime_custom_loader_extends_editor_capabilities() {
    let dir = project(&[
        (
            "project.godot",
            "config_version=5\n[autoload]\nCustom=\"*res://loader.gd\"\n",
        ),
        (
            "loader.gd",
            r#"extends Node
class CustomLoader extends ResourceFormatLoader:
    func _get_recognized_extensions() -> PackedStringArray:
        return PackedStringArray(["special"])
    func _handles_type(type: StringName) -> bool:
        return type == &"Resource"
    func _get_resource_type(path: String) -> String:
        return "Resource" if path.get_extension() == "special" else ""
    func _load(_path: String, _original_path: String, _use_sub_threads: bool, _cache_mode: int) -> Variant:
        return Resource.new()
var loader: CustomLoader
func _ready() -> void:
    loader = CustomLoader.new()
    ResourceLoader.add_resource_format_loader(loader)
func _exit_tree() -> void:
    ResourceLoader.remove_resource_format_loader(loader)
"#,
        ),
        ("custom.special", "handled by runtime loader"),
    ]);
    let report = engine_check(dir.path(), &real_engine(), CheckRequest::default());
    assert_eq!(report.outcome, Outcome::Passed, "{report:#?}");
    assert_eq!(report.counts.as_ref().unwrap().resources, 1);
    let extensions: Vec<String> = serde_json::from_slice(
        &fs::read(report.artifact_dir.unwrap().join("editor-extensions.json")).unwrap(),
    )
    .unwrap();
    assert!(
        !extensions.iter().any(|e| e == "special"),
        "custom capability must come from runtime, not editor"
    );
}

#[test]
#[cfg(feature = "test-engine")]
fn operational_error_envelopes_are_incomplete_but_script_load_and_base_are_failed() {
    for (harness, stages) in [
        (
            "import_scan",
            vec!["arguments", "manifest", "editor", "load"],
        ),
        ("check", vec!["arguments", "manifest", "extensions"]),
        (
            "script_bootstrap",
            vec!["arguments", "load", "base", "unknown"],
        ),
    ] {
        for stage in stages {
            let dir = project(&[]);
            let mut scenario = serde_json::json!({});
            scenario[harness] = serde_json::json!({"mode":"error_envelope","exit":2,"payload":{"stage":stage,"message":"failure detail","field":"fixture"}});
            let (_fake, engine) = fake(scenario);
            let report = engine_check(
                dir.path(),
                &engine,
                CheckRequest {
                    scripts: vec![
                        gdview::ResPath::parse("res://a.gd").unwrap(),
                        gdview::ResPath::parse("res://b.gd").unwrap(),
                    ],
                    ..CheckRequest::default()
                },
            );
            let validation_failure =
                harness == "script_bootstrap" && matches!(stage, "load" | "base");
            assert_eq!(
                report.outcome,
                if validation_failure {
                    Outcome::Failed
                } else {
                    Outcome::Incomplete
                },
                "{harness}/{stage}: {report:#?}"
            );
            assert_eq!(report.outcome.exit_code(), 1);
            let failure = report
                .failures
                .iter()
                .find(|f| f.message.contains("failure detail"))
                .unwrap();
            assert_eq!(
                failure.kind,
                if validation_failure {
                    check::FailureKind::ResourceLoad
                } else {
                    check::FailureKind::MissingCompletion
                }
            );
            assert!(failure.message.contains(stage) && failure.message.contains("fixture"));
            let failed = report
                .phases
                .iter()
                .find(|p| Some(&p.id) == failure.phase.as_ref())
                .unwrap();
            assert_eq!(failed.artifacts.len(), 3);
            assert!(failed.artifacts.iter().all(|p| p.is_file()));
            assert_eq!(report.phases.last().unwrap().outcome, PhaseOutcome::Skipped);
        }
    }
}

#[test]
#[cfg(feature = "test-engine")]
fn baseline_resolution_requires_exact_completed_phase_not_just_kind() {
    let dir = project(&[]);
    let scripts = vec![
        gdview::ResPath::parse("res://a.gd").unwrap(),
        gdview::ResPath::parse("res://b.gd").unwrap(),
    ];
    let (_fake, engine) = fake(serde_json::json!({
        "script_bootstrap:res://a.gd":{"stderr":"WARNING: old A\n"},
        "script_bootstrap:res://b.gd":{"stderr":"WARNING: old B\n"}
    }));
    let baseline = engine_check(
        dir.path(),
        &engine,
        CheckRequest {
            scripts: scripts.clone(),
            ..CheckRequest::default()
        },
    );
    assert_eq!(baseline.outcome, Outcome::Passed);
    let (_fake, engine) =
        fake(serde_json::json!({"script_bootstrap:res://a.gd":{"stderr":"SCRIPT ERROR: new A\n"}}));
    let report = engine_check(
        dir.path(),
        &engine,
        CheckRequest {
            scripts: scripts.clone(),
            baseline: Some(baseline.clone()),
            ..CheckRequest::default()
        },
    );
    assert_eq!(report.outcome, Outcome::Failed);
    assert_eq!(
        phase(&report, "project_script:1:res://b.gd"),
        PhaseOutcome::Skipped
    );
    assert!(report.baseline.unwrap().resolved.is_empty());
    let (_fake, engine) = fake(serde_json::json!({}));
    let report = engine_check(
        dir.path(),
        &engine,
        CheckRequest {
            scripts,
            baseline: Some(baseline),
            ..CheckRequest::default()
        },
    );
    assert_eq!(report.baseline.unwrap().resolved.len(), 2);

    let (_fake, engine) = fake(serde_json::json!({"import_scan":{"stderr":"WARNING: old scan\n"}}));
    let baseline = engine_check(dir.path(), &engine, CheckRequest::default());
    let (_fake, engine) = fake(serde_json::json!({"--import":{"stderr":"ERROR: new import\n"}}));
    let report = engine_check(
        dir.path(),
        &engine,
        CheckRequest {
            baseline: Some(baseline),
            ..CheckRequest::default()
        },
    );
    assert_eq!(phase(&report, "import_scan"), PhaseOutcome::Skipped);
    assert!(report.baseline.unwrap().resolved.is_empty());
}

#[test]
#[cfg(feature = "test-engine")]
fn incomplete_phases_never_resolve_baseline_diagnostics() {
    for stage in ["--import", "import_scan", "check", "script_bootstrap"] {
        let dir = project(&[]);
        let scripts = vec![gdview::ResPath::parse("res://a.gd").unwrap()];
        let mut scenario = serde_json::json!({});
        scenario[stage] = serde_json::json!({"stderr":"WARNING: old diagnostic\n"});
        let (_fake, engine) = fake(scenario);
        let baseline = engine_check(
            dir.path(),
            &engine,
            CheckRequest {
                scripts: scripts.clone(),
                ..CheckRequest::default()
            },
        );
        for mode in ["hang", "crash", "no_envelope", "error_envelope"] {
            // Raw import has no completion envelope contract.
            if stage == "--import" && mode != "hang" {
                continue;
            }
            let mut scenario = serde_json::json!({});
            scenario[stage] = serde_json::json!({"mode":mode});
            let (_fake, engine) = fake(scenario);
            let report = engine_check(
                dir.path(),
                &engine,
                CheckRequest {
                    scripts: scripts.clone(),
                    baseline: Some(baseline.clone()),
                    phase_deadline: std::time::Duration::from_millis(150),
                    script_deadline: std::time::Duration::from_millis(150),
                    ..CheckRequest::default()
                },
            );
            assert_eq!(
                report.outcome,
                Outcome::Incomplete,
                "{stage}/{mode}: {report:#?}"
            );
            assert!(
                report.baseline.unwrap().resolved.is_empty(),
                "{stage}/{mode}"
            );
        }
    }
}

#[test]
#[cfg(feature = "test-engine")]
fn completed_inventory_with_new_errors_can_resolve_old_diagnostics() {
    let dir = project(&[]);
    for stage in ["import_scan", "check"] {
        let mut scenario = serde_json::json!({});
        scenario[stage] = serde_json::json!({"stderr":"WARNING: old diagnostic\n"});
        let (_fake, engine) = fake(scenario);
        let baseline = engine_check(dir.path(), &engine, CheckRequest::default());
        let mut scenario = serde_json::json!({});
        scenario[stage] = serde_json::json!({"stderr":"ERROR: new diagnostic\n","exit":if stage == "check" { 1 } else { 0 }});
        let (_fake, engine) = fake(scenario);
        let report = engine_check(
            dir.path(),
            &engine,
            CheckRequest {
                baseline: Some(baseline),
                ..CheckRequest::default()
            },
        );
        assert_eq!(report.outcome, Outcome::Failed);
        let comparison = report.baseline.unwrap();
        assert_eq!(comparison.resolved.len(), 1);
        assert_eq!(comparison.resolved[0].message, "old diagnostic");
        assert_eq!(comparison.new.len(), 1);
    }
}

#[test]
fn narrow_static_slice_resolves_only_covered_findings() {
    let dir = project(&[
        ("inside/a.tscn", BROKEN_SCENE),
        ("outside/b.tscn", BROKEN_SCENE),
    ]);
    let baseline = static_check(dir.path(), CheckRequest::default()).unwrap();
    assert_eq!(baseline.failures.len(), 2);
    write(
        dir.path(),
        &[(
            "inside/a.tscn",
            "[gd_scene format=3]\n[node name=\"Root\" type=\"Node\"]\n",
        )],
    );
    let report = static_check(
        dir.path(),
        CheckRequest {
            slice: vec!["inside".into()],
            baseline: Some(baseline),
            ..CheckRequest::default()
        },
    )
    .unwrap();
    assert_eq!(report.outcome, Outcome::Passed);
    let comparison = report.baseline.unwrap();
    assert_eq!(comparison.resolved.len(), 1);
    assert_eq!(
        comparison.resolved[0].resource.as_deref(),
        Some("res://inside/a.tscn")
    );
    assert!(comparison.new.is_empty() && comparison.carried.is_empty());
}

#[test]
#[cfg(feature = "test-engine")]
fn narrow_engine_slice_does_not_resolve_outside_or_unlocated_diagnostics() {
    let dir = project(&[
        ("inside/a.gd", "extends Node\n"),
        ("outside/b.gd", "extends Node\n"),
    ]);
    let (_fake, engine) = fake(
        serde_json::json!({"check":{"stderr":"WARNING: inside\n   at: test (res://inside/a.gd:1)\nWARNING: outside\n   at: test (res://outside/b.gd:1)\nWARNING: unlocated\n"}}),
    );
    let baseline = engine_check(dir.path(), &engine, CheckRequest::default());
    assert_eq!(baseline.phases.last().unwrap().diagnostics.len(), 3);
    let (_fake, engine) = fake(serde_json::json!({}));
    let report = engine_check(
        dir.path(),
        &engine,
        CheckRequest {
            slice: vec!["inside".into()],
            baseline: Some(baseline.clone()),
            ..CheckRequest::default()
        },
    );
    let comparison = report.baseline.unwrap();
    assert_eq!(comparison.resolved.len(), 1);
    assert_eq!(comparison.resolved[0].message, "inside");
    let report = engine_check(
        dir.path(),
        &engine,
        CheckRequest {
            baseline: Some(baseline),
            ..CheckRequest::default()
        },
    );
    assert_eq!(report.baseline.unwrap().resolved.len(), 3);
}

fn real_engine() -> Engine {
    Engine {
        executable: std::env::var_os("GDKIT_TEST_GODOT")
            .expect("set GDKIT_TEST_GODOT to opt in")
            .into(),
        version: "real".into(),
        fingerprint: "test".into(),
        source: SelectionSource::Environment,
    }
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT"]
fn real_engine_missing_method_is_reported_with_res_path_and_line() {
    let dir = project(&[(
        "test.gd",
        "extends SceneTree\nfunc _initialize():\n    var node = Node.new()\n    node.free.call_deferred()\n    quit.call_deferred()\n    node.call(\"missing_method\")\n",
    )]);
    let report = engine_check(
        dir.path(),
        &real_engine(),
        CheckRequest {
            scripts: vec![gdview::ResPath::parse("res://test.gd").unwrap()],
            ..CheckRequest::default()
        },
    );
    assert_eq!(report.outcome, Outcome::Failed, "{report:#?}");
    assert!(
        report
            .phases
            .iter()
            .flat_map(|p| &p.diagnostics)
            .any(|d| d.resource.as_deref() == Some("res://test.gd") && d.line.is_some())
    );
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT"]
fn real_engine_strict_methods_turns_unsafe_call_into_error() {
    let dir = project(&[(
        "test.gd",
        "extends Node\nfunc test(value: Variant):\n    value.unknown_method()\n",
    )]);
    let engine = real_engine();
    let relaxed = engine_check(
        dir.path(),
        &engine,
        CheckRequest {
            strict_methods: Some(false),
            ..CheckRequest::default()
        },
    );
    assert_eq!(relaxed.outcome, Outcome::Passed, "{relaxed:#?}");
    let strict = engine_check(
        dir.path(),
        &engine,
        CheckRequest {
            strict_methods: Some(true),
            ..CheckRequest::default()
        },
    );
    assert_eq!(strict.outcome, Outcome::Failed, "{strict:#?}");
    assert!(
        strict
            .phases
            .iter()
            .flat_map(|p| &p.diagnostics)
            .any(|d| d.severity == Severity::Error)
    );
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT"]
fn real_engine_autoloads_are_available_to_project_scripts() {
    let dir = project(&[
        (
            "project.godot",
            "config_version=5\n[autoload]\nState=\"*res://state.gd\"\n",
        ),
        (
            "state.gd",
            "extends Node\nvar ready_value = false\nfunc _ready():\n    ready_value = true\n",
        ),
        (
            "test.gd",
            "extends SceneTree\nfunc _initialize():\n    if not State.ready_value:\n        push_error(\"autoload not ready\")\n    quit()\n",
        ),
    ]);
    let report = engine_check(
        dir.path(),
        &real_engine(),
        CheckRequest {
            scripts: vec![gdview::ResPath::parse("res://test.gd").unwrap()],
            ..CheckRequest::default()
        },
    );
    assert_eq!(report.outcome, Outcome::Passed, "{report:#?}");
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT"]
fn real_engine_blocked_autoload_import_times_out_without_orphans() {
    let dir = project(&[
        (
            "project.godot",
            "config_version=5\n[autoload]\nBlocked=\"*res://blocked.gd\"\n",
        ),
        (
            "blocked.gd",
            "@tool\nextends Node\nfunc _init():\n    while true:\n        OS.delay_msec(10)\n",
        ),
    ]);
    let report = engine_check(
        dir.path(),
        &real_engine(),
        CheckRequest {
            phase_deadline: std::time::Duration::from_secs(3),
            ..CheckRequest::default()
        },
    );
    assert_eq!(report.outcome, Outcome::Incomplete, "{report:#?}");
    assert!(
        report
            .phases
            .iter()
            .any(|p| p.outcome == PhaseOutcome::TimedOut)
    );
    #[cfg(target_os = "linux")]
    for pid in report.phases.iter().filter_map(|p| p.process_pid) {
        assert!(!PathBuf::from(format!("/proc/{pid}")).exists());
    }
}
