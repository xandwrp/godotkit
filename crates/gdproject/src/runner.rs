//! One engine invocation. Every operation module builds an [`Invocation`] and
//! calls [`run_harness`] or [`run_engine`]; nothing else spawns Godot.
//!
//! # Tests (tests/runner.rs, offline with `fake-godot`)
//! - `run_harness_writes_harness_and_protocol_to_a_temp_dir_and_passes_user_args_after_double_dash`
//! - `run_harness_adds_editor_flag_only_when_requested`
//! - `run_harness_decodes_envelope_and_attaches_diagnostics_and_captured_output`
//! - `run_harness_maps_error_envelope_to_error_harness_with_stage`
//! - `run_harness_maps_missing_envelope_to_error_protocol`
//! - `run_harness_enforces_deadline_and_reports_timeout_with_partial_output`
//! - `run_engine_is_the_raw_form_used_for_import_dump_and_run`
//! - `temp_files_are_removed_after_every_outcome_including_panic`

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use serde::de::DeserializeOwned;

use crate::diagnostics::Diagnostic;
use crate::engine::Engine;
use crate::process::Captured;
use crate::protocol::Envelope;

/// Embedded GDScript harnesses. Each one `extends SceneTree` and emits a single
/// protocol envelope. `protocol.gd` is always written next to the harness.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Harness {
    Probe,
    Check,
    ImportScan,
    ResourceSchema,
    ResourceCreate,
    RuntimeProbe,
    ScriptBootstrap,
}

impl Harness {
    pub fn name(self) -> &'static str {
        match self {
            Harness::Probe => "probe",
            Harness::Check => "check",
            Harness::ImportScan => "import_scan",
            Harness::ResourceSchema => "resource_schema",
            Harness::ResourceCreate => "resource_create",
            Harness::RuntimeProbe => "runtime_probe",
            Harness::ScriptBootstrap => "script_bootstrap",
        }
    }
    pub fn source(self) -> &'static str {
        match self {
            Harness::Probe => include_str!("harness/probe.gd"),
            Harness::Check => include_str!("harness/check.gd"),
            Harness::ImportScan => include_str!("harness/import_scan.gd"),
            Harness::ResourceSchema => include_str!("harness/resource_schema.gd"),
            Harness::ResourceCreate => include_str!("harness/resource_create.gd"),
            Harness::RuntimeProbe => include_str!("harness/runtime_probe.gd"),
            Harness::ScriptBootstrap => include_str!("harness/script_bootstrap.gd"),
        }
    }
    /// Needs `--editor` (EditorInterface, importers).
    pub fn needs_editor(self) -> bool {
        matches!(self, Harness::ImportScan)
    }
}

pub const PROTOCOL_SOURCE: &str = include_str!("harness/protocol.gd");

/// Hash of every embedded harness; part of the engine probe key.
pub fn harness_hash() -> u64 {
    todo!()
}

pub struct Invocation<'a> {
    pub engine: &'a Engine,
    /// Directory passed to `--path`. A scratch copy for `check`, the real project otherwise.
    pub project_dir: &'a Path,
    pub deadline: Duration,
    /// Extra engine flags before `--script` (e.g. `--quiet`, `--import`, `--dump-extension-api-with-docs`).
    pub engine_args: Vec<OsString>,
    /// Arguments after `--`, visible via `OS.get_cmdline_user_args()`.
    pub user_args: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
}

impl<'a> Invocation<'a> {
    pub fn new(engine: &'a Engine, project_dir: &'a Path, deadline: Duration) -> Self {
        Self { engine, project_dir, deadline, engine_args: Vec::new(), user_args: Vec::new(), env: Vec::new() }
    }
}

pub struct HarnessRun<T> {
    pub envelope: Envelope<T>,
    pub captured: Captured,
    pub diagnostics: Vec<Diagnostic>,
}

/// `<engine> --headless --no-header [--editor] --path <dir> --script <tmp>/<harness>.gd -- <user_args…>`
/// Decodes the envelope; an `ok: false` envelope becomes `Error::Harness`.
pub fn run_harness<T: DeserializeOwned>(invocation: &Invocation<'_>, harness: Harness) -> crate::Result<HarnessRun<T>> {
    todo!()
}

/// Raw engine run with no harness: `--editor --import`, the extension-api dump, etc.
/// The caller interprets exit status and diagnostics.
pub fn run_engine(invocation: &Invocation<'_>) -> crate::Result<(Captured, Vec<Diagnostic>)> {
    todo!()
}

/// Spawns the engine as the game for [`crate::run`]: `--path <project> [--headless] --script runtime_probe.gd [scene] -- <args>`.
/// Returns the guard and the temp dir holding the harness (dropped with the guard).
pub fn spawn_game(invocation: &Invocation<'_>, log: &Path) -> crate::Result<crate::process::ChildGuard> {
    todo!()
}
