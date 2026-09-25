//! Run a scene to a stopping condition and report. The agent-shaped replacement
//! for durable sessions: launch, observe, stop, all inside one gdkit invocation.
//!
//! Flow:
//! 1. Resolve scene (arg, else `project.godot` main scene). Build the probe env
//!    (ready file, token, adapter from `[run]`). `runner::spawn_game` with a log under
//!    a fresh artifact dir. The guard owns the process for the whole call.
//! 2. Wait for the ready file (bounded). Not ready → terminate, `Outcome::NotReady`.
//! 3. Poll `probe::status` every `poll_interval`. After each poll, if a `--until`
//!    condition is set, `probe::checkpoints` and test the JSON pointer. Stop when
//!    `frames >= max_frames`, the condition holds, the process exits, or `deadline`.
//! 4. Final `probe::checkpoints` (and `probe::network` if requested), then terminate
//!    with grace. Diagnostics are parsed from the log.
//!
//! Every exit path ends with the process dead; the guard's drop guarantees it.
//!
//! # Tests (tests/run.rs, offline with `fake-godot` acting as the game)
//! - `stops_after_max_frames_and_reports_final_checkpoints`
//! - `stops_when_until_condition_holds_and_records_the_frame`
//! - `until_condition_never_met_is_reported_as_timed_out_not_error`
//! - `process_exit_before_ready_is_not_ready_with_log_excerpt`
//! - `process_exit_during_run_reports_exit_status_and_diagnostics`
//! - `script_errors_in_the_log_make_the_verdict_failed`
//! - `adapter_errors_are_reported_as_checkpoint_status`
//! - `game_is_always_dead_when_run_returns` (including on panic inside polling)
//!
//! Engine (`#[ignore]`): `real_engine_runs_main_scene_with_autoloads_to_a_checkpoint`

use std::path::PathBuf;
use std::time::Duration;

use gdview::ResPath;
use serde::{Deserialize, Serialize};

use crate::engine::Engine;
use crate::workspace::Workspace;

pub const RUN_REPORT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq)]
pub struct RunRequest {
    pub scene: Option<ResPath>,
    pub headless: bool,
    pub arguments: Vec<String>,
    pub max_frames: Option<u64>,
    /// `(json pointer, expected value)`; stop when the checkpoint at `pointer` equals it.
    pub until: Option<(String, serde_json::Value)>,
    pub ready_deadline: Duration,
    /// Wall-clock cap for the whole run.
    pub deadline: Duration,
    pub poll_interval: Duration,
    pub observe_network: bool,
    pub grace: Duration,
}

impl Default for RunRequest {
    fn default() -> Self {
        Self {
            scene: None,
            headless: true,
            arguments: Vec::new(),
            max_frames: Some(120),
            until: None,
            ready_deadline: Duration::from_secs(20),
            deadline: Duration::from_secs(60),
            poll_interval: Duration::from_millis(50),
            observe_network: false,
            grace: Duration::from_secs(5),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunReport {
    pub schema_version: u32,
    pub outcome: Outcome,
    pub verdict: Verdict,
    pub scene: ResPath,
    pub frames: u64,
    pub stopped_by: StopReason,
    pub exit_status: Option<i32>,
    pub checkpoints: Option<crate::probe::CheckpointObservation>,
    pub network: Option<crate::probe::NetworkObservation>,
    pub diagnostics: Vec<crate::diagnostics::Diagnostic>,
    pub log_path: PathBuf,
    pub elapsed_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Completed,
    NotReady,
    TimedOut,
    Exited,
}

/// The exit code: passed unless the log had errors, the process crashed, or `--until` never held.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Passed,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    MaxFrames,
    UntilCondition,
    ProcessExit,
    Deadline,
    NotReady,
}

pub fn run(
    workspace: &Workspace,
    engine: &Engine,
    request: &RunRequest,
) -> crate::Result<RunReport> {
    todo!()
}
