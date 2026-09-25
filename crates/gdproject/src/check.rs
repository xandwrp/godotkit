//! Project validation: static cross-reference checks, then import a disposable
//! copy, load every script/scene/resource in a fresh process, then optionally run
//! project scripts.
//!
//! Pipeline:
//! 0. `static_analysis` gdview::xref over the (sliced) sources, no engine   (phase StaticAnalysis, milliseconds)
//! 1. `scan`            gdview file query on the copy → manifest + project fingerprint
//! 2. `cache_import`    `run_engine --editor --quiet --import`      (phase Import)
//! 3. `import_scan`     `run_harness ImportScan`  (waits for the FS scanner, loads scripts)
//! 4. `class_cache_audit`  compares `.godot/global_script_class_cache.cfg` in the copy to gdview declarations
//! 5. `load_all`        `run_harness Check`       (phase ResourceLoading; strict policy set in `_init`)
//! 6. `project_script`  per `--script`: `run_harness ScriptBootstrap` with deadline
//! API-cache diagnostic enrichment is explicitly deferred; this operation never dumps or loads an API index.
//! 8. `baseline`        when given, classify diagnostics as new / carried / resolved by `identity`
//!
//! Step 6 is skipped (recorded as skipped) if anything before failed. Static
//! findings not carried by the baseline block engine phases. Carried findings
//! retain their failures and failed verdict but permit engine validation. Every captured stream and event
//! log is preserved under the artifact dir before its verdict is interpreted.
//!
//! Static analysis always covers the whole project (a slice cannot see what
//! references it), then keeps only the findings located inside the slice.
//!
//! Missing/malformed completion, crashes, timeouts and capture limits are
//! incomplete reports (exit 1), not successful validation or tool errors.
//! Check's valid failure payload and exit 1 remain failed reports.
//!
//! # Tests (tests/check.rs)
//! Offline with `fake-godot` (scripted to emit chosen output per phase):
//! - `passes_when_every_phase_completes_without_errors`
//! - `zero_exit_script_error_on_either_stream_fails_resource_loading`
//! - `import_errors_fail_before_resource_loading_and_skip_runtime_phases`
//! - `missing_completion_marker_is_incomplete_not_passed`
//! - `ignore_rules_suppress_and_are_counted_in_policy`
//! - `class_cache_audit_reports_missing_moved_and_stale_entries`
//! - `slice_builds_minimal_project_and_rejects_bad_paths`
//! - `project_script_timeout_is_recorded_as_timeout_and_stops_further_runtime_phases`
//! - `static_findings_fail_the_check_before_any_engine_phase_runs`
//! - `baseline_classifies_new_carried_and_resolved_by_identity_not_line`
//! - `suggestions_are_attached_only_when_an_api_index_is_cached`
//! - `artifacts_hold_raw_streams_and_event_log_for_every_phase`
//! - `report_json_round_trips_and_exit_mapping_is_0_1_2`
//!   Engine (`#[ignore]`, GDKIT_TEST_GODOT):
//! - `real_engine_missing_method_is_reported_with_res_path_and_line`
//! - `real_engine_strict_methods_turns_unsafe_call_into_error`
//! - `real_engine_autoloads_are_available_to_project_scripts`
//! - `real_engine_blocked_autoload_import_times_out_without_orphans`

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use gdview::ResPath;
use serde::{Deserialize, Serialize};

use crate::diagnostics::{Diagnostic, Severity, Stream};
use crate::engine::Engine;
use crate::workspace::Workspace;

pub const CHECK_REPORT_SCHEMA_VERSION: u32 = 3;

#[derive(Clone, Debug)]
pub struct CheckRequest {
    /// Relative paths; empty means the whole project.
    pub slice: Vec<PathBuf>,
    /// `None` = take it from `gdkit.toml`.
    pub strict_methods: Option<bool>,
    pub scripts: Vec<gdview::ResPath>,
    pub script_deadline: Duration,
    /// Deadline for the import and load phases, which are otherwise unbounded.
    pub phase_deadline: Duration,
    /// Skip the engine phases; static analysis only.
    pub static_only: bool,
    /// A previous report to diff against.
    pub baseline: Option<CheckReport>,
}

impl Default for CheckRequest {
    fn default() -> Self {
        Self {
            slice: Vec::new(),
            strict_methods: None,
            scripts: Vec::new(),
            script_deadline: Duration::from_secs(30),
            phase_deadline: Duration::from_secs(600),
            static_only: false,
            baseline: None,
        }
    }
}

/// Progress callbacks so the CLI can stream to stderr while JSON stays on stdout.
pub trait CheckObserver {
    fn phase_started(&mut self, phase: &PhaseId) {}
    fn phase_finished(&mut self, phase: &PhaseId, outcome: PhaseOutcome, elapsed: Duration) {}
    fn diagnostics(&mut self, phase: &PhaseId, diagnostics: &[Diagnostic]) {}
    fn note(&mut self, message: &str) {}
}

pub struct NoObserver;
impl CheckObserver for NoObserver {}

/// `engine` may be `None` only for a static-only request.
pub fn run(
    workspace: &Workspace,
    engine: Option<&Engine>,
    request: &CheckRequest,
    observer: &mut dyn CheckObserver,
) -> crate::Result<CheckReport> {
    if request.static_only && !request.scripts.is_empty() {
        return Err(crate::Error::Invalid(
            "static-only checks cannot run project scripts".into(),
        ));
    }
    let slice = validate_slice(&request.slice)?;
    let engine = match (engine, request.static_only) {
        (Some(engine), _) => Some(engine),
        (None, true) => None,
        (None, false) => return Err(crate::Error::NoEngine),
    };
    let config = crate::config::Config::load(workspace.root())?.unwrap_or_default();
    let analysis = static_analysis(workspace, &slice, observer)?;
    let mut resolution_coverage =
        HashSet::from([(PhaseKind::StaticAnalysis, analysis.phase.id.id.clone())]);
    let phases = vec![analysis.phase];
    // Baselines relax only the engine gate, never the static verdict or evidence.
    let has_new_static_findings = analysis.findings.iter().any(|diagnostic| {
        !request.baseline.as_ref().is_some_and(|baseline| {
            baseline
                .phases
                .iter()
                .filter(|p| p.id.kind == PhaseKind::StaticAnalysis)
                .flat_map(|p| &p.diagnostics)
                .any(|d| d.identity == diagnostic.identity)
        })
    });
    let failures: Vec<Failure> = analysis
        .findings
        .iter()
        .map(|diagnostic| Failure {
            phase: Some(phases[0].id.clone()),
            kind: FailureKind::StaticFinding,
            message: format!(
                "{}:{}: {}",
                diagnostic.resource.as_deref().unwrap_or("?"),
                diagnostic.line.unwrap_or(0),
                diagnostic.message
            ),
        })
        .collect();
    let mut report = CheckReport {
        schema_version: CHECK_REPORT_SCHEMA_VERSION,
        outcome: if failures.is_empty() {
            Outcome::Passed
        } else {
            Outcome::Failed
        },
        engine: engine.map(EngineIdentity::from),
        project: ProjectIdentity {
            root: workspace.root().to_path_buf(),
            fingerprint: analysis.fingerprint,
            sliced: !slice.is_empty(),
        },
        policy: Policy {
            strict_methods: request
                .strict_methods
                .unwrap_or(config.check.strict_methods),
            ignore_rules: config.check.ignore_import_errors.len(),
        },
        phases,
        counts: Some(analysis.counts),
        failures,
        suppressed_diagnostics: 0,
        baseline: None,
        artifact_dir: None,
    };
    if !request.static_only {
        if !has_new_static_findings {
            engine_phases(
                workspace,
                engine.expect("validated engine"),
                request,
                &config.check,
                observer,
                &mut report,
                &mut resolution_coverage,
            )?;
        } else {
            report
                .phases
                .extend(skipped_engine_phases(request, "static analysis failed"));
        }
    }
    if let Some(baseline) = &request.baseline {
        report.baseline = Some(compare_to_baseline(
            &report,
            baseline,
            &slice,
            &resolution_coverage,
        ));
    }
    Ok(report)
}

#[derive(Deserialize)]
struct LoadCounts {
    scripts: usize,
    scenes: usize,
    resources: usize,
}
#[derive(Deserialize)]
struct LoadPayload {
    counts: LoadCounts,
    failures: Vec<ResPath>,
    recognized_extensions: Vec<String>,
}
#[derive(Deserialize)]
struct ScanPayload {
    scanned: usize,
    recognized_extensions: Vec<String>,
}

// Both harnesses promise a canonical capability list, not successful-load metadata.
fn valid_extensions(extensions: &[String]) -> bool {
    extensions.windows(2).all(|pair| pair[0] < pair[1])
        && extensions.iter().all(|extension| {
            !extension.is_empty()
                && *extension == extension.to_lowercase()
                && !extension.chars().any(|c| {
                    matches!(c, '.' | '/' | '\\' | ':') || c.is_whitespace() || c.is_control()
                })
        })
}

fn blank_phase(kind: PhaseKind, id: impl Into<String>) -> Phase {
    Phase {
        id: PhaseId {
            kind,
            id: id.into(),
        },
        outcome: PhaseOutcome::Completed,
        skipped_reason: None,
        elapsed_ms: 0,
        process_pid: None,
        exit_status: None,
        diagnostics: Vec::new(),
        artifacts: Vec::new(),
    }
}

fn fail(
    report: &mut CheckReport,
    phase: &mut Phase,
    kind: FailureKind,
    message: impl Into<String>,
    incomplete: bool,
) {
    phase.outcome = if kind == FailureKind::Timeout {
        PhaseOutcome::TimedOut
    } else {
        PhaseOutcome::Failed
    };
    if incomplete {
        report.outcome = Outcome::Incomplete;
    } else if report.outcome != Outcome::Incomplete {
        report.outcome = Outcome::Failed;
    }
    report.failures.push(Failure {
        phase: Some(phase.id.clone()),
        kind,
        message: message.into(),
    });
}

fn engine_phases(
    workspace: &Workspace,
    engine: &Engine,
    request: &CheckRequest,
    config: &crate::config::CheckConfig,
    observer: &mut dyn CheckObserver,
    report: &mut CheckReport,
    resolution_coverage: &mut HashSet<(PhaseKind, String)>,
) -> crate::Result<()> {
    use crate::runner::{self, Harness, Invocation};
    use crate::workspace::IsolatedCopy;
    let copy = if request.slice.is_empty()
        || request.slice.iter().any(|p| p == std::path::Path::new("."))
    {
        IsolatedCopy::full(workspace.project())?
    } else {
        IsolatedCopy::slice(workspace.project(), &request.slice)?
    };
    let project = Workspace::open(&copy.path)?;
    // Inventory precedes import and artifact creation. The default query excludes
    // .godot (including old artifacts); engine registries decide load eligibility.
    let files = project
        .project()
        .files(&gdview::files::FileQuery::default())?;
    let manifest: Vec<ResPath> = files
        .iter()
        .map(|p| project.project().localize(p))
        .collect::<gdview::Result<_>>()?;
    let artifacts = workspace.new_artifact_dir("check")?;
    report.artifact_dir = Some(artifacts.path.clone());
    let manifest_path = artifacts.write(
        "manifest.json",
        &serde_json::to_vec(&manifest).expect("serializable paths"),
    )?;
    let mut stages = vec![
        (blank_phase(PhaseKind::Import, "import"), None),
        (
            blank_phase(PhaseKind::Import, "import_scan"),
            Some(Harness::ImportScan),
        ),
        (
            blank_phase(PhaseKind::ClassCacheAudit, "class_cache_audit"),
            None,
        ),
        (
            blank_phase(PhaseKind::ResourceLoading, "resource_loading"),
            Some(Harness::Check),
        ),
    ];
    stages.extend(request.scripts.iter().enumerate().map(|(i, path)| {
        (
            blank_phase(
                PhaseKind::ProjectScript,
                format!("project_script:{i}:{path}"),
            ),
            Some(Harness::ScriptBootstrap),
        )
    }));
    let mut stopped = false;
    let mut script_index = 0;
    let mut editor_extensions = None;
    let editor_extensions_path = artifacts.path.join("editor-extensions.json");
    for (mut phase, harness) in stages {
        if stopped {
            phase.outcome = PhaseOutcome::Skipped;
            phase.skipped_reason = Some("previous engine phase failed".into());
            report.phases.push(phase);
            continue;
        }
        observer.phase_started(&phase.id);
        if phase.id.kind == PhaseKind::ClassCacheAudit {
            let started = Instant::now();
            for message in audit_class_cache(project.project())? {
                let mut diagnostic = Diagnostic {
                    sequence: phase.diagnostics.len() as u64,
                    severity: Severity::Error,
                    stream: Stream::Tool,
                    code: Some("GDKIT_SCRIPT_CLASS_CACHE".into()),
                    message: message.clone(),
                    resource: None,
                    line: None,
                    column: None,
                    frames: Vec::new(),
                    timestamp_unix_ms: None,
                    occurrences: 1,
                    is_shutdown_noise: false,
                    identity: String::new(),
                    suggestions: Vec::new(),
                };
                diagnostic.identity = diagnostic.compute_identity();
                phase.diagnostics.push(diagnostic);
                fail(report, &mut phase, FailureKind::ClassCache, message, false);
            }
            phase.elapsed_ms = started.elapsed().as_millis() as u64;
        } else {
            let mut invocation = Invocation::new(
                engine,
                &copy.path,
                if harness == Some(Harness::ScriptBootstrap) {
                    request.script_deadline
                } else {
                    request.phase_deadline
                },
            );
            // Godot --quiet also suppresses the harness protocol's print lines.
            if harness.is_none() {
                invocation.engine_args.push("--quiet".into());
            }
            match harness {
                None => invocation
                    .engine_args
                    .extend(["--editor".into(), "--import".into()]),
                Some(Harness::ScriptBootstrap) => {
                    invocation
                        .user_args
                        .push(request.scripts[script_index].as_str().into());
                    script_index += 1;
                    report.counts.as_mut().unwrap().project_scripts_run += 1;
                }
                Some(_) => {
                    invocation
                        .user_args
                        .push(manifest_path.as_os_str().to_owned());
                    if harness == Some(Harness::Check) {
                        invocation.user_args.push(
                            if report.policy.strict_methods {
                                "strict-methods"
                            } else {
                                "project-policy"
                            }
                            .into(),
                        );
                        invocation
                            .user_args
                            .push(editor_extensions_path.as_os_str().to_owned());
                    }
                }
            }
            let (captured, mut diagnostics) = match harness {
                Some(harness) => runner::run_harness_raw(&invocation, harness)?,
                None => runner::run_engine(&invocation)?,
            };
            // Persist complete streams and observation order before interpreting any verdict.
            let prefix = format!("{:02}", report.phases.len());
            phase
                .artifacts
                .push(artifacts.write(&format!("{prefix}.stdout"), &captured.stdout())?);
            phase
                .artifacts
                .push(artifacts.write(&format!("{prefix}.stderr"), &captured.stderr())?);
            let events: Vec<_> = captured.lines.iter().map(|line| serde_json::json!({"sequence":line.sequence,"stream":match line.stream { crate::process::OutputStream::Stdout => "stdout", crate::process::OutputStream::Stderr => "stderr" },"bytes":line.bytes,"observed_at_unix_ms":line.observed_at_unix_ms})).collect();
            phase.artifacts.push(artifacts.write(
                &format!("{prefix}.events.json"),
                &serde_json::to_vec(&events).expect("serializable events"),
            )?);
            phase.elapsed_ms = captured.duration.as_millis() as u64;
            phase.process_pid = Some(captured.pid);
            phase.exit_status = captured.status.and_then(|s| s.code());
            if phase.id.kind == PhaseKind::Import {
                report.suppressed_diagnostics += crate::diagnostics::apply_ignore_rules(
                    &mut diagnostics,
                    &config.ignore_import_errors,
                );
            }
            for diagnostic in diagnostics.iter().filter(|d| d.severity == Severity::Error) {
                fail(
                    report,
                    &mut phase,
                    FailureKind::Diagnostic,
                    &diagnostic.message,
                    false,
                );
            }
            phase.diagnostics = diagnostics;
            if captured.output_limit_exceeded {
                fail(
                    report,
                    &mut phase,
                    FailureKind::OutputLimit,
                    "engine output capture limit exceeded; retained output is incomplete",
                    true,
                );
            } else if captured.timed_out {
                fail(
                    report,
                    &mut phase,
                    FailureKind::Timeout,
                    "engine deadline exceeded",
                    true,
                );
            } else if captured.status.is_none_or(|s| s.code().is_none()) {
                fail(
                    report,
                    &mut phase,
                    FailureKind::ProcessExit,
                    "engine crashed without normal exit",
                    true,
                );
            } else {
                let completed_work = interpret_completion(
                    report,
                    &mut phase,
                    harness,
                    &captured,
                    &manifest,
                    &mut editor_extensions,
                );
                // A valid completed scan/load can report validation failures and
                // still cover its inventory. A script startup marker cannot: user
                // code may have stopped early, so only clean script exits resolve.
                let completed_inventory = completed_work
                    && match harness {
                        Some(Harness::ImportScan) => phase.exit_status == Some(0),
                        Some(Harness::Check) => matches!(phase.exit_status, Some(0 | 1)),
                        _ => false,
                    };
                if completed_inventory {
                    resolution_coverage.insert((phase.id.kind, phase.id.id.clone()));
                }
                if !captured.success() {
                    fail(
                        report,
                        &mut phase,
                        FailureKind::ProcessExit,
                        format!("engine exited {:?}", captured.status),
                        false,
                    );
                }
            }
            if harness == Some(Harness::ImportScan) && phase.outcome == PhaseOutcome::Completed {
                artifacts.write(
                    "editor-extensions.json",
                    &serde_json::to_vec(
                        editor_extensions
                            .as_ref()
                            .expect("validated ImportScan handoff"),
                    )
                    .expect("serializable extensions"),
                )?;
            }
        }
        observer.diagnostics(&phase.id, &phase.diagnostics);
        observer.phase_finished(
            &phase.id,
            phase.outcome,
            Duration::from_millis(phase.elapsed_ms),
        );
        if phase.outcome == PhaseOutcome::Completed {
            resolution_coverage.insert((phase.id.kind, phase.id.id.clone()));
        }
        stopped = phase.outcome != PhaseOutcome::Completed;
        report.phases.push(phase);
    }
    Ok(())
}

fn interpret_completion(
    report: &mut CheckReport,
    phase: &mut Phase,
    harness: Option<crate::runner::Harness>,
    captured: &crate::process::Captured,
    manifest: &[ResPath],
    editor_extensions: &mut Option<Vec<String>>,
) -> bool {
    use crate::runner::Harness;
    let Some(harness) = harness else { return false };
    let stdout = captured.stdout();
    let text = String::from_utf8_lossy(&stdout);
    let markers = text
        .lines()
        .filter(|line| *line == "GDKIT_SCRIPT_STARTED")
        .count();
    if harness == Harness::ScriptBootstrap
        && !text
            .lines()
            .any(|l| l.starts_with(crate::protocol::RESULT_PREFIX))
    {
        if markers != 1 {
            fail(
                report,
                phase,
                FailureKind::MissingCompletion,
                format!("expected exactly one script startup marker, received {markers}"),
                true,
            );
        }
        return false;
    }
    let result =
        crate::protocol::parse_envelope::<serde_json::Value>(text.lines().map(str::to_owned));
    let envelope = match result {
        Ok(e)
            if e.harness == harness.name()
                && ((e.ok && e.payload.is_some() && e.error.is_none())
                    || (!e.ok && e.payload.is_none() && e.error.is_some())) =>
        {
            e
        }
        other => {
            fail(
                report,
                phase,
                FailureKind::MissingCompletion,
                format!("invalid {} completion: {other:?}", harness.name()),
                true,
            );
            return false;
        }
    };
    if !envelope.ok {
        if harness == Harness::ScriptBootstrap && markers != 0 {
            fail(
                report,
                phase,
                FailureKind::MissingCompletion,
                "error envelope after script handoff",
                true,
            );
        } else {
            let error = envelope.error.unwrap();
            let validation_failure = harness == Harness::ScriptBootstrap
                && matches!(error.stage.as_str(), "load" | "base");
            fail(
                report,
                phase,
                if validation_failure {
                    FailureKind::ResourceLoad
                } else {
                    FailureKind::MissingCompletion
                },
                format!("{}: {} ({:?})", error.stage, error.message, error.field),
                !validation_failure,
            );
        }
        return false;
    }
    let payload = envelope.payload.unwrap();
    let valid = match harness {
        Harness::ImportScan => serde_json::from_value::<ScanPayload>(payload).is_ok_and(|p| {
            let valid = valid_extensions(&p.recognized_extensions)
                && p.scanned
                    == manifest
                        .iter()
                        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("gd")))
                        .count();
            if valid {
                *editor_extensions = Some(p.recognized_extensions);
            }
            valid
        }),
        Harness::Check => match serde_json::from_value::<LoadPayload>(payload) {
            Ok(payload) => {
                let extensions: HashSet<&str> = payload
                    .recognized_extensions
                    .iter()
                    .map(String::as_str)
                    .collect();
                let eligible: Vec<ResPath> = manifest
                    .iter()
                    .filter(|path| {
                        path.extension()
                            .is_some_and(|ext| extensions.contains(ext.to_lowercase().as_str()))
                    })
                    .cloned()
                    .collect();
                let scripts = eligible
                    .iter()
                    .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("gd")))
                    .count();
                let scenes = eligible
                    .iter()
                    .filter(|p| {
                        p.extension().is_some_and(|e| {
                            e.eq_ignore_ascii_case("tscn") || e.eq_ignore_ascii_case("scn")
                        })
                    })
                    .count();
                if !valid_extensions(&payload.recognized_extensions)
                    || editor_extensions.as_ref().is_none_or(|editor| {
                        editor.iter().any(|ext| !extensions.contains(ext.as_str()))
                    })
                    || payload.counts.scripts != scripts
                    || payload.counts.scenes != scenes
                    || payload.counts.resources != eligible.len() - scripts - scenes
                    || payload.failures.iter().any(|p| !eligible.contains(p))
                {
                    false
                } else {
                    let counts = report.counts.as_mut().unwrap();
                    counts.scripts = scripts;
                    counts.scenes = scenes;
                    counts.resources = payload.counts.resources;
                    for path in payload.failures {
                        fail(
                            report,
                            phase,
                            FailureKind::ResourceLoad,
                            format!("failed to load {path}"),
                            false,
                        );
                    }
                    true
                }
            }
            Err(_) => false,
        },
        _ => false,
    };
    if !valid {
        fail(
            report,
            phase,
            FailureKind::MissingCompletion,
            "malformed completion payload or unexpected success envelope",
            true,
        );
    }
    valid
}

fn audit_class_cache(project: &gdview::Project) -> crate::Result<Vec<String>> {
    use gdview::scene::Value;
    let declarations = gdview::declarations::index_project(project)?;
    let expected: Vec<_> = declarations
        .scripts
        .iter()
        .filter_map(|s| {
            s.declaration
                .class_name
                .as_ref()
                .map(|n| (n.name.as_str(), s.declaration.path.as_str()))
        })
        .collect();
    let path = project.root().join(".godot/global_script_class_cache.cfg");
    let source = match std::fs::read_to_string(&path) {
        Ok(source) => source,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && expected.is_empty() => {
            return Ok(Vec::new());
        }
        Err(e) => return Ok(vec![format!("class cache unavailable: {e}")]),
    };
    // The cache is a root-level ConfigFile assignment using the same Variant text grammar.
    let parsed = gdview::scene::parse(&format!(
        "[gd_resource type=\"Resource\" format=3]\n[resource]\n{source}"
    ));
    let parsed = match parsed {
        Ok(p) => p,
        Err(e) => return Ok(vec![format!("malformed class cache: {e}")]),
    };
    let list = parsed.resource.as_ref().and_then(|p| p.get("list"));
    let entries = match list {
        Some(Value::Array(a)) => Some(a),
        Some(Value::Call { name, args }) if name == "Array[Dictionary]" => match args.as_slice() {
            [Value::Array(a)] => Some(a),
            _ => None,
        },
        _ => None,
    };
    let Some(entries) = entries else {
        return Ok(vec!["malformed class cache list".into()]);
    };
    let mut actual = Vec::new();
    let mut failures = Vec::new();
    for entry in entries {
        let Value::Dict(fields) = entry else {
            failures.push("malformed class cache entry".into());
            continue;
        };
        let get = |key| {
            fields
                .iter()
                .find(|(k, _)| k.as_str() == Some(key))
                .and_then(|(_, v)| v.as_str())
        };
        match (get("class"), get("path")) {
            (Some(name), Some(path)) if ResPath::parse(path).is_ok() => {
                if actual.contains(&(name, path)) {
                    failures.push(format!("duplicate class cache entry {name}: {path}"));
                }
                actual.push((name, path));
                if !expected.contains(&(name, path)) {
                    failures.push(format!("stale or moved class cache entry {name}: {path}"));
                }
            }
            _ => failures.push("malformed class cache class/path".into()),
        }
    }
    for (name, path) in expected {
        if !actual.contains(&(name, path)) {
            failures.push(format!("missing class cache entry {name}: {path}"));
        }
    }
    Ok(failures)
}

/// Slice entries become `res://` paths; `..`, absolute paths, and `.godot` are rejected.
fn validate_slice(slice: &[PathBuf]) -> crate::Result<Vec<ResPath>> {
    slice
        .iter()
        .map(|entry| {
            let invalid = || {
                crate::Error::Invalid(format!(
                    "--slice {}: expected a project-relative path without `..`, outside .godot",
                    entry.display()
                ))
            };
            if entry.is_absolute() {
                return Err(invalid());
            }
            let mut segments = Vec::new();
            for component in entry.components() {
                match component {
                    std::path::Component::Normal(segment) => {
                        segments.push(segment.to_str().ok_or_else(invalid)?)
                    }
                    std::path::Component::CurDir => {}
                    _ => return Err(invalid()),
                }
            }
            if segments.is_empty() {
                return Ok(ResPath::root());
            }
            ResPath::from_relative(&segments.join("/")).map_err(|_| invalid())
        })
        .collect()
}

struct StaticAnalysis {
    phase: Phase,
    findings: Vec<Diagnostic>,
    counts: Counts,
    fingerprint: String,
}

fn static_analysis(
    workspace: &Workspace,
    slice: &[ResPath],
    observer: &mut dyn CheckObserver,
) -> crate::Result<StaticAnalysis> {
    let id = PhaseId {
        kind: PhaseKind::StaticAnalysis,
        id: "static_analysis".into(),
    };
    observer.phase_started(&id);
    let started = Instant::now();
    let project = workspace.project();
    let declarations = gdview::declarations::index_project(project)?;
    let uids = gdview::uid::UidMap::build(project)?;
    let graph = gdview::xref::ProjectGraph::load(project, &declarations, &uids)?;
    let in_slice =
        |path: &ResPath| slice.is_empty() || slice.iter().any(|entry| path.starts_with(entry));

    let findings: Vec<Diagnostic> = gdview::xref::analyze(&graph)
        .into_iter()
        .filter(|finding| in_slice(&finding.at.path))
        .enumerate()
        .map(|(sequence, finding)| finding_diagnostic(sequence as u64, finding))
        .collect();
    let scripts = declarations
        .scripts
        .iter()
        .filter(|s| in_slice(&s.declaration.path))
        .count();
    let (mut scenes, mut resources) = (0, 0);
    for (_, file) in graph.scenes.iter().filter(|(path, _)| in_slice(path)) {
        match file.kind {
            gdview::scene::FileKind::Scene => scenes += 1,
            gdview::scene::FileKind::Resource => resources += 1,
        }
    }
    let fingerprint = fingerprint(project, &graph.files)?;

    let outcome = if findings.is_empty() {
        PhaseOutcome::Completed
    } else {
        PhaseOutcome::Failed
    };
    let elapsed = started.elapsed();
    observer.diagnostics(&id, &findings);
    observer.phase_finished(&id, outcome, elapsed);
    Ok(StaticAnalysis {
        counts: Counts {
            scripts,
            scenes,
            resources,
            static_findings: findings.len(),
            project_scripts_run: 0,
        },
        phase: Phase {
            id,
            outcome,
            skipped_reason: None,
            elapsed_ms: elapsed.as_millis() as u64,
            process_pid: None,
            exit_status: None,
            diagnostics: findings.clone(),
            artifacts: Vec::new(),
        },
        findings,
        fingerprint,
    })
}

fn finding_diagnostic(sequence: u64, finding: gdview::xref::Finding) -> Diagnostic {
    let code = serde_json::to_value(finding.kind).ok().and_then(|kind| {
        kind.as_str()
            .map(|kind| format!("GDKIT_{}", kind.to_ascii_uppercase()))
    });
    let mut diagnostic = Diagnostic {
        sequence,
        severity: Severity::Error,
        stream: Stream::Tool,
        code,
        message: finding.message,
        resource: Some(finding.at.path.to_string()),
        line: Some(finding.at.line as u32),
        column: None,
        frames: Vec::new(),
        timestamp_unix_ms: None,
        occurrences: 1,
        is_shutdown_noise: false,
        identity: String::new(),
        suggestions: finding.suggestions,
    };
    diagnostic.identity = diagnostic.compute_identity();
    diagnostic
}

/// blake3 over the sources static analysis reads: scripts, scenes, resources,
/// uid sidecars, import files, and `project.godot`, by path and content.
fn fingerprint(project: &gdview::Project, files: &[ResPath]) -> crate::Result<String> {
    const READ: &[&str] = &["gd", "tscn", "tres", "uid", "import", "godot"];
    let mut hasher = blake3::Hasher::new();
    for path in files {
        let wanted = path
            .extension()
            .is_some_and(|ext| READ.iter().any(|read| ext.eq_ignore_ascii_case(read)));
        if !wanted {
            continue;
        }
        let os_path = project.globalize(path);
        let bytes = std::fs::read(&os_path).map_err(|source| crate::Error::Io {
            path: os_path,
            source,
        })?;
        for part in [path.as_str().as_bytes(), &bytes] {
            hasher.update(&(part.len() as u64).to_le_bytes());
            hasher.update(part);
        }
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn skipped_engine_phases(request: &CheckRequest, reason: &str) -> Vec<Phase> {
    let skipped = |kind, id: String| Phase {
        id: PhaseId { kind, id },
        outcome: PhaseOutcome::Skipped,
        skipped_reason: Some(reason.to_owned()),
        elapsed_ms: 0,
        process_pid: None,
        exit_status: None,
        diagnostics: Vec::new(),
        artifacts: Vec::new(),
    };
    let mut phases = vec![
        skipped(PhaseKind::Import, "import".into()),
        skipped(PhaseKind::ResourceLoading, "resource_loading".into()),
    ];
    phases.extend(request.scripts.iter().enumerate().map(|(index, script)| {
        skipped(
            PhaseKind::ProjectScript,
            format!("project_script:{index}:{script}"),
        )
    }));
    phases
}

/// Resolution requires corresponding completed work and current path coverage.
/// Merely attempting a phase (or another phase of the same kind) proves nothing
/// about absent diagnostics. Unlocated findings are not resolved by narrow slices.
fn compare_to_baseline(
    report: &CheckReport,
    baseline: &CheckReport,
    slice: &[ResPath],
    resolution_coverage: &HashSet<(PhaseKind, String)>,
) -> BaselineComparison {
    let covers = |diagnostic: &Diagnostic| {
        slice.is_empty()
            || slice.iter().any(|path| path == &ResPath::root())
            || diagnostic
                .resource
                .as_deref()
                .and_then(|path| ResPath::parse(path).ok())
                .is_some_and(|path| slice.iter().any(|entry| path.starts_with(entry)))
    };
    let current: Vec<&Diagnostic> = report
        .phases
        .iter()
        .flat_map(|phase| &phase.diagnostics)
        .collect();
    let previous: Vec<(&Phase, &Diagnostic)> = baseline
        .phases
        .iter()
        .flat_map(|phase| phase.diagnostics.iter().map(move |d| (phase, d)))
        .collect();
    let current_ids: HashSet<&str> = current.iter().map(|d| d.identity.as_str()).collect();
    let previous_ids: HashSet<&str> = previous.iter().map(|(_, d)| d.identity.as_str()).collect();
    let (carried, new): (Vec<Diagnostic>, Vec<Diagnostic>) = current
        .into_iter()
        .cloned()
        .partition(|d| previous_ids.contains(d.identity.as_str()));
    let resolved = previous
        .into_iter()
        .filter(|(phase, d)| {
            resolution_coverage.contains(&(phase.id.kind, phase.id.id.clone()))
                && covers(d)
                && !current_ids.contains(d.identity.as_str())
        })
        .map(|(_, d)| d.clone())
        .collect();
    BaselineComparison {
        baseline_fingerprint: baseline.project.fingerprint.clone(),
        new,
        carried,
        resolved,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckReport {
    pub schema_version: u32,
    pub outcome: Outcome,
    /// `None` for a static-only check, which never touches an engine.
    pub engine: Option<EngineIdentity>,
    pub project: ProjectIdentity,
    pub policy: Policy,
    pub phases: Vec<Phase>,
    pub counts: Option<Counts>,
    pub failures: Vec<Failure>,
    pub suppressed_diagnostics: usize,
    /// Present only when a baseline was supplied.
    pub baseline: Option<BaselineComparison>,
    /// Raw engine output. `None` when no engine phase ran.
    pub artifact_dir: Option<PathBuf>,
}

/// Diagnostics partitioned by `Diagnostic::identity` against a previous report.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaselineComparison {
    pub baseline_fingerprint: String,
    pub new: Vec<Diagnostic>,
    pub carried: Vec<Diagnostic>,
    pub resolved: Vec<Diagnostic>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Passed,
    Failed,
    Incomplete,
}

impl Outcome {
    /// 0 passed, 1 failed/incomplete. Tool failures never reach here (they are `Err`).
    pub fn exit_code(self) -> u8 {
        match self {
            Outcome::Passed => 0,
            Outcome::Failed | Outcome::Incomplete => 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineIdentity {
    pub executable: PathBuf,
    pub version: String,
    pub fingerprint: String,
}

impl From<&Engine> for EngineIdentity {
    fn from(engine: &Engine) -> Self {
        Self {
            executable: engine.executable.clone(),
            version: engine.version.clone(),
            fingerprint: engine.fingerprint.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectIdentity {
    pub root: PathBuf,
    /// blake3 over manifest paths + contents, so two reports can be compared.
    pub fingerprint: String,
    pub sliced: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Policy {
    pub strict_methods: bool,
    pub ignore_rules: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhaseId {
    pub kind: PhaseKind,
    /// `static_analysis`, `import`, `project_script:2:res://x.gd`, …
    pub id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseKind {
    StaticAnalysis,
    FileScan,
    EngineValidation,
    Import,
    ClassCacheAudit,
    ResourceLoading,
    ProjectScript,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseOutcome {
    Completed,
    Failed,
    TimedOut,
    Skipped,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Phase {
    pub id: PhaseId,
    pub outcome: PhaseOutcome,
    pub skipped_reason: Option<String>,
    pub elapsed_ms: u64,
    pub process_pid: Option<u32>,
    pub exit_status: Option<i32>,
    pub diagnostics: Vec<Diagnostic>,
    pub artifacts: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    pub scripts: usize,
    pub scenes: usize,
    pub resources: usize,
    pub static_findings: usize,
    pub project_scripts_run: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failure {
    pub phase: Option<PhaseId>,
    pub kind: FailureKind,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    StaticFinding,
    Diagnostic,
    ProcessExit,
    Timeout,
    MissingCompletion,
    OutputLimit,
    ResourceLoad,
    ClassCache,
}
