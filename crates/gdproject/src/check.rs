//! Project validation: static cross-reference checks, then import a disposable
//! copy, load every script/scene/resource in a fresh process, then optionally run
//! project scripts.
//!
//! Flow (each step is a private fn; `run` only sequences them and fills the report):
//! 0. `static_analysis` gdview::xref over the (sliced) sources, no engine   (phase StaticAnalysis, milliseconds)
//! 1. `scan`            gdview file query on the copy → manifest + project fingerprint
//! 2. `cache_import`    `run_engine --editor --quiet --import`      (phase Import)
//! 3. `import_scan`     `run_harness ImportScan`  (waits for the FS scanner, loads scripts)
//! 4. `class_cache_audit`  compares `.godot/global_script_class_cache.cfg` in the copy to gdview declarations
//! 5. `load_all`        `run_harness Check`       (phase ResourceLoading; strict policy set in `_init`)
//! 6. `project_script`  per `--script`: `run_harness ScriptBootstrap` with deadline
//! 7. `enrich`          `diagnostics::suggest` with the api index when it is already cached (never dumps)
//! 8. `baseline`        when given, classify diagnostics as new / carried / resolved by `identity`
//!
//! Step 6 is skipped (recorded as skipped) if anything before failed. Static
//! findings fail the check like engine errors do, and when they do, no engine
//! phase runs: they are recorded as skipped. Every captured stream is
//! preserved under the artifact dir before it is parsed.
//!
//! Static analysis always covers the whole project (a slice cannot see what
//! references it), then keeps only the findings located inside the slice.
//!
//! Status: step 0 and the report/baseline plumbing are implemented. Steps 1–7
//! are not; a request that needs them returns `Error::Invalid` rather than a
//! report that would claim the engine phases passed.
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
    let slice = validate_slice(&request.slice)?;
    let engine = match (engine, request.static_only) {
        (Some(engine), _) => Some(engine),
        (None, true) => None,
        (None, false) => return Err(crate::Error::NoEngine),
    };
    let analysis = static_analysis(workspace, &slice, observer)?;
    let mut phases = vec![analysis.phase];
    if !request.static_only {
        if phases[0].outcome != PhaseOutcome::Completed {
            phases.extend(skipped_engine_phases(request, "static analysis failed"));
        } else {
            return Err(crate::Error::Invalid(
                "the engine phases of `check` (import, resource loading, project scripts) are not implemented yet; use static-only".into(),
            ));
        }
    }
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
        outcome: if failures.is_empty() { Outcome::Passed } else { Outcome::Failed },
        engine: engine.map(EngineIdentity::from),
        project: ProjectIdentity {
            root: workspace.root().to_path_buf(),
            fingerprint: analysis.fingerprint,
            sliced: !slice.is_empty(),
        },
        policy: Policy { strict_methods: request.strict_methods.unwrap_or(false), ignore_rules: 0 },
        phases,
        counts: Some(analysis.counts),
        failures,
        suppressed_diagnostics: 0,
        baseline: None,
        artifact_dir: None,
    };
    if let Some(baseline) = &request.baseline {
        report.baseline = Some(compare_to_baseline(&report, baseline));
    }
    Ok(report)
}

/// Slice entries become `res://` paths; `..`, absolute paths, and `.godot` are rejected.
fn validate_slice(slice: &[PathBuf]) -> crate::Result<Vec<ResPath>> {
    slice
        .iter()
        .map(|entry| {
            let invalid = || crate::Error::Invalid(format!("--slice {}: expected a project-relative path without `..`, outside .godot", entry.display()));
            if entry.is_absolute() {
                return Err(invalid());
            }
            let mut segments = Vec::new();
            for component in entry.components() {
                match component {
                    std::path::Component::Normal(segment) => segments.push(segment.to_str().ok_or_else(invalid)?),
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

fn static_analysis(workspace: &Workspace, slice: &[ResPath], observer: &mut dyn CheckObserver) -> crate::Result<StaticAnalysis> {
    let id = PhaseId { kind: PhaseKind::StaticAnalysis, id: "static_analysis".into() };
    observer.phase_started(&id);
    let started = Instant::now();
    let project = workspace.project();
    let declarations = gdview::declarations::index_project(project)?;
    let uids = gdview::uid::UidMap::build(project)?;
    let graph = gdview::xref::ProjectGraph::load(project, &declarations, &uids)?;
    let in_slice = |path: &ResPath| slice.is_empty() || slice.iter().any(|entry| path.starts_with(entry));

    let findings: Vec<Diagnostic> = gdview::xref::analyze(&graph)
        .into_iter()
        .filter(|finding| in_slice(&finding.at.path))
        .enumerate()
        .map(|(sequence, finding)| finding_diagnostic(sequence as u64, finding))
        .collect();
    let scripts = declarations.scripts.iter().filter(|s| in_slice(&s.declaration.path)).count();
    let (mut scenes, mut resources) = (0, 0);
    for (_, file) in graph.scenes.iter().filter(|(path, _)| in_slice(path)) {
        match file.kind {
            gdview::scene::FileKind::Scene => scenes += 1,
            gdview::scene::FileKind::Resource => resources += 1,
        }
    }
    let fingerprint = fingerprint(project, &graph.files)?;

    let outcome = if findings.is_empty() { PhaseOutcome::Completed } else { PhaseOutcome::Failed };
    let elapsed = started.elapsed();
    observer.diagnostics(&id, &findings);
    observer.phase_finished(&id, outcome, elapsed);
    Ok(StaticAnalysis {
        counts: Counts { scripts, scenes, resources, static_findings: findings.len(), project_scripts_run: 0 },
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
    let code = serde_json::to_value(finding.kind)
        .ok()
        .and_then(|kind| kind.as_str().map(|kind| format!("GDKIT_{}", kind.to_ascii_uppercase())));
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
        let wanted = path.extension().is_some_and(|ext| READ.iter().any(|read| ext.eq_ignore_ascii_case(read)));
        if !wanted {
            continue;
        }
        let os_path = project.globalize(path);
        let bytes = std::fs::read(&os_path).map_err(|source| crate::Error::Io { path: os_path, source })?;
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
    let mut phases = vec![skipped(PhaseKind::Import, "import".into()), skipped(PhaseKind::ResourceLoading, "resource_loading".into())];
    phases.extend(
        request
            .scripts
            .iter()
            .enumerate()
            .map(|(index, script)| skipped(PhaseKind::ProjectScript, format!("project_script:{index}:{script}"))),
    );
    phases
}

/// Partitions by identity. Only phase kinds that ran this time can resolve a
/// baseline diagnostic, so a static-only run never "resolves" engine errors.
fn compare_to_baseline(report: &CheckReport, baseline: &CheckReport) -> BaselineComparison {
    let ran: HashSet<PhaseKind> = report
        .phases
        .iter()
        .filter(|phase| phase.outcome != PhaseOutcome::Skipped)
        .map(|phase| phase.id.kind)
        .collect();
    let current: Vec<&Diagnostic> = report.phases.iter().flat_map(|phase| &phase.diagnostics).collect();
    let previous: Vec<(&Phase, &Diagnostic)> =
        baseline.phases.iter().flat_map(|phase| phase.diagnostics.iter().map(move |d| (phase, d))).collect();
    let current_ids: HashSet<&str> = current.iter().map(|d| d.identity.as_str()).collect();
    let previous_ids: HashSet<&str> = previous.iter().map(|(_, d)| d.identity.as_str()).collect();
    let (carried, new): (Vec<Diagnostic>, Vec<Diagnostic>) =
        current.into_iter().cloned().partition(|d| previous_ids.contains(d.identity.as_str()));
    let resolved = previous
        .into_iter()
        .filter(|(phase, d)| ran.contains(&phase.id.kind) && !current_ids.contains(d.identity.as_str()))
        .map(|(_, d)| d.clone())
        .collect();
    BaselineComparison { baseline_fingerprint: baseline.project.fingerprint.clone(), new, carried, resolved }
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
    ResourceLoad,
    ClassCache,
}
