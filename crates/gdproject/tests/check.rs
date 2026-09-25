// Acceptance tests for gdproject::check. Offline unless prefixed real_engine_.
#![allow(unused)]

use std::fs;
use std::path::{Path, PathBuf};

use gdproject::Workspace;
use gdproject::check::{self, CheckReport, CheckRequest, NoObserver, Outcome, PhaseKind, PhaseOutcome};
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
    check::run(&workspace, None, &CheckRequest { static_only: true, ..request }, &mut NoObserver)
}

const BROKEN_SCENE: &str = "[gd_scene format=3]\n\n[ext_resource type=\"Script\" path=\"res://gone.gd\" id=\"1\"]\n\n[node name=\"Main\" type=\"Node\"]\nscript = ExtResource(\"1\")\n";

#[test]
fn static_only_needs_no_engine_and_slice_keeps_findings_located_in_it() {
    let dir = project(&[
        ("scenes/main.tscn", BROKEN_SCENE),
        ("scripts/ok.gd", "extends Node\n"),
        ("scripts/bad.gd", "extends Node\nconst X = preload(\"res://nope.tscn\")\n"),
    ]);
    let report = static_check(dir.path(), CheckRequest::default()).unwrap();
    assert_eq!(report.outcome, Outcome::Failed);
    assert!(report.engine.is_none() && report.artifact_dir.is_none());
    assert_eq!(report.phases.len(), 1);
    let phase = &report.phases[0];
    assert_eq!((phase.id.kind, phase.outcome), (PhaseKind::StaticAnalysis, PhaseOutcome::Failed));
    let located: Vec<_> = phase.diagnostics.iter().map(|d| (d.resource.clone().unwrap(), d.line.unwrap())).collect();
    assert_eq!(located, [("res://scenes/main.tscn".to_string(), 3), ("res://scripts/bad.gd".to_string(), 2)]);
    let diagnostic = &phase.diagnostics[0];
    assert_eq!(diagnostic.severity, Severity::Error);
    assert_eq!(diagnostic.code.as_deref(), Some("GDKIT_MISSING_RESOURCE"));
    assert_eq!(report.failures.len(), 2);
    assert!(report.failures[0].message.starts_with("res://scenes/main.tscn:3: "), "{}", report.failures[0].message);
    let counts = report.counts.as_ref().unwrap();
    assert_eq!((counts.scripts, counts.scenes, counts.resources, counts.static_findings), (2, 1, 0, 2));

    let sliced = static_check(dir.path(), CheckRequest { slice: vec![PathBuf::from("scripts")], ..CheckRequest::default() }).unwrap();
    assert!(sliced.project.sliced);
    assert_eq!(sliced.phases[0].diagnostics.len(), 1);
    assert_eq!(sliced.phases[0].diagnostics[0].resource.as_deref(), Some("res://scripts/bad.gd"));
    assert_eq!(sliced.counts.as_ref().unwrap().scripts, 2);

    let clean = static_check(dir.path(), CheckRequest { slice: vec![PathBuf::from("scripts/ok.gd")], ..CheckRequest::default() }).unwrap();
    assert_eq!(clean.outcome, Outcome::Passed);
    assert_eq!(clean.phases[0].outcome, PhaseOutcome::Completed);
    assert!(clean.failures.is_empty());
}

#[test]
fn slice_paths_that_escape_the_project_are_tool_errors() {
    let dir = project(&[]);
    for bad in ["../outside", "/abs/path", ".godot/imported", "a/../../b"] {
        let result = static_check(dir.path(), CheckRequest { slice: vec![PathBuf::from(bad)], ..CheckRequest::default() });
        assert!(matches!(result, Err(gdproject::Error::Invalid(_))), "{bad}: {result:?}");
    }
}

#[test]
#[ignore = "scaffold"]
fn passes_when_every_phase_completes_without_errors() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn zero_exit_script_error_on_either_stream_fails_resource_loading() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn import_errors_fail_before_resource_loading_and_skip_runtime_phases() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn missing_completion_marker_is_incomplete_not_passed() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn ignore_rules_suppress_and_are_counted_in_policy() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn class_cache_audit_reports_missing_moved_and_stale_entries() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn slice_builds_minimal_project_and_rejects_bad_paths() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn project_script_timeout_is_recorded_as_timeout_and_stops_further_runtime_phases() {
    todo!()
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
    let request = CheckRequest { scripts: vec![gdview::ResPath::parse("res://t.gd").unwrap()], ..CheckRequest::default() };
    let workspace = Workspace::open(dir.path()).unwrap();
    let report = check::run(&workspace, Some(&engine), &request, &mut NoObserver).unwrap();
    assert_eq!(report.outcome, Outcome::Failed);
    assert_eq!(report.engine.as_ref().unwrap().executable, engine.executable);
    let phases: Vec<_> = report.phases.iter().map(|p| (p.id.id.as_str(), p.outcome)).collect();
    assert_eq!(
        phases,
        [
            ("static_analysis", PhaseOutcome::Failed),
            ("import", PhaseOutcome::Skipped),
            ("resource_loading", PhaseOutcome::Skipped),
            ("project_script:0:res://t.gd", PhaseOutcome::Skipped),
        ]
    );
    assert!(report.phases[1..].iter().all(|p| p.skipped_reason.as_deref() == Some("static analysis failed")));
    // Without an engine, a full check is a tool error, not a report.
    assert!(matches!(check::run(&workspace, None, &CheckRequest::default(), &mut NoObserver), Err(gdproject::Error::NoEngine)));
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
    write(dir.path(), &[
        ("a.gd", "extends Node\n\n\nconst KEEP = preload(\"res://keep_missing.tscn\")\nconst NEW = preload(\"res://new_missing.tscn\")\n"),
    ]);
    let after = static_check(dir.path(), CheckRequest { baseline: Some(before.clone()), ..CheckRequest::default() }).unwrap();
    let baseline = after.baseline.as_ref().unwrap();
    assert_eq!(baseline.baseline_fingerprint, before.project.fingerprint);
    assert_ne!(after.project.fingerprint, before.project.fingerprint);
    let targets = |diagnostics: &[gdproject::diagnostics::Diagnostic]| {
        diagnostics.iter().map(|d| (d.message.clone(), d.line.unwrap())).collect::<Vec<_>>()
    };
    assert_eq!(targets(&baseline.new), [("preload of res://new_missing.tscn, which does not exist".to_string(), 5)]);
    assert_eq!(targets(&baseline.carried), [("preload of res://keep_missing.tscn, which does not exist".to_string(), 4)]);
    assert_eq!(targets(&baseline.resolved), [("preload of res://fixed_later.tscn, which does not exist".to_string(), 3)]);
}

#[test]
#[ignore = "scaffold"]
fn suggestions_are_attached_only_when_an_api_index_is_cached() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn artifacts_hold_raw_streams_and_event_log_for_every_phase() {
    todo!()
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
    let passed = static_check(dir.path(), CheckRequest { slice: vec![PathBuf::from("ok.gd")], ..CheckRequest::default() }).unwrap();
    assert_eq!(passed.outcome.exit_code(), 0);
    assert_eq!(Outcome::Incomplete.exit_code(), 1);
    // Exit 2 is a tool failure: an `Err`, never a report.
    let not_a_project = tempfile::tempdir().unwrap();
    assert!(Workspace::open(not_a_project.path()).is_err());
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT"]
fn real_engine_missing_method_is_reported_with_res_path_and_line() {
    todo!()
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT"]
fn real_engine_strict_methods_turns_unsafe_call_into_error() {
    todo!()
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT"]
fn real_engine_autoloads_are_available_to_project_scripts() {
    todo!()
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT"]
fn real_engine_blocked_autoload_import_times_out_without_orphans() {
    todo!()
}
