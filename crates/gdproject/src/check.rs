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
//! findings fail the check like engine errors do. Every captured stream is
//! preserved under the artifact dir before it is parsed.
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

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::diagnostics::Diagnostic;
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

pub fn run(
    workspace: &Workspace,
    engine: &Engine,
    request: &CheckRequest,
    observer: &mut dyn CheckObserver,
) -> crate::Result<CheckReport> {
    todo!()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckReport {
    pub schema_version: u32,
    pub outcome: Outcome,
    pub engine: EngineIdentity,
    pub project: ProjectIdentity,
    pub policy: Policy,
    pub phases: Vec<Phase>,
    pub counts: Option<Counts>,
    pub failures: Vec<Failure>,
    pub suppressed_diagnostics: usize,
    /// Present only when a baseline was supplied.
    pub baseline: Option<BaselineComparison>,
    pub artifact_dir: PathBuf,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
