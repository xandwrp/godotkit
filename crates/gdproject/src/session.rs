//! Durable named game processes with immutable per-launch records.
//!
//! One process per session. The runtime probe is injected with
//! `--script runtime_probe.gd`, which then instantiates the requested (or main)
//! scene itself; there is no second "bridge" editor process.
//!
//! Launch flow:
//! 1. Resolve scene, build `process::Spawn` (detached, own group, per-session user dir, log file).
//! 2. Write a `Launching` record first so a crash here leaves a visible, stoppable trail.
//! 3. `process::spawn`; wait (bounded) for the probe's ready file; write `Running` with `ProcessId` and endpoint.
//! 4. On any failure after spawn: terminate, write `Failed`, return the error.
//!
//! # Tests (tests/session.rs, offline with `fake-godot` acting as the game)
//! - `launch_writes_launching_then_running_records_and_releases_the_child`
//! - `launch_refuses_a_name_whose_latest_generation_is_still_alive`
//! - `launch_failure_after_spawn_terminates_and_records_failed`
//! - `list_hides_superseded_generations_unless_all`
//! - `resolve_accepts_name_and_name_at_generation`
//! - `stop_waits_and_reports_exited_or_killed_and_a_second_stop_is_not_found`
//! - `restart_stops_the_old_generation_before_launching_the_new_one`
//! - `stale_pid_reuse_is_not_reported_as_running`
//! - `corrupt_record_files_are_listed_as_problems_not_fatal`
//! - `sessions_survive_parent_shell_signals` (spawn detached, send SIGINT to test process group)

use std::path::PathBuf;
use std::time::Duration;

use gdview::ResPath;
use serde::{Deserialize, Serialize};

use crate::engine::Engine;
use crate::process::ProcessId;
use crate::records::{Record, RecordProblem};
use crate::workspace::Workspace;

pub const SESSION_RECORD_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchSpec {
    pub name: String,
    pub scene: Option<ResPath>,
    pub headless: bool,
    pub arguments: Vec<String>,
    pub environment: Vec<(String, String)>,
    /// How long to wait for the runtime probe to report ready.
    pub ready_deadline: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRecord {
    pub schema_version: u32,
    pub name: String,
    pub generation: u32,
    pub state: SessionState,
    pub engine: crate::check::EngineIdentity,
    pub scene: Option<ResPath>,
    pub headless: bool,
    pub arguments: Vec<String>,
    pub environment: Vec<(String, String)>,
    pub process: Option<ProcessId>,
    pub probe: Option<crate::probe::ProbeEndpoint>,
    pub user_dir: PathBuf,
    pub log_path: PathBuf,
    pub started_unix_ms: u64,
    pub ended_unix_ms: Option<u64>,
    pub exit_status: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Launching,
    Running,
    Stopped,
    Failed,
}

impl Record for SessionRecord {
    const SCHEMA_VERSION: u32 = SESSION_RECORD_SCHEMA_VERSION;
    fn schema_version(&self) -> u32 {
        self.schema_version
    }
    fn file_stem(&self) -> String {
        format!("{}-{}@{}", self.started_unix_ms, self.name, self.generation)
    }
}

/// `name` or `name@generation`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionSelector {
    pub name: String,
    pub generation: Option<u32>,
}

impl SessionSelector {
    pub fn parse(text: &str) -> crate::Result<Self> {
        todo!()
    }
}

pub fn validate_name(name: &str) -> crate::Result<()> {
    todo!()
}

pub fn launch(workspace: &Workspace, engine: &Engine, spec: LaunchSpec) -> crate::Result<SessionRecord> {
    todo!()
}

pub struct SessionList {
    pub records: Vec<SessionRecord>,
    pub problems: Vec<RecordProblem>,
}

/// Latest generation per name unless `all`. Liveness is re-checked via `ProcessId`.
pub fn list(workspace: &Workspace, all: bool) -> crate::Result<SessionList> {
    todo!()
}

pub fn resolve(workspace: &Workspace, selector: &SessionSelector) -> crate::Result<SessionRecord> {
    todo!()
}

pub fn is_running(record: &SessionRecord) -> bool {
    todo!()
}

pub fn stop(
    workspace: &Workspace,
    record: &SessionRecord,
    grace: Duration,
) -> crate::Result<crate::process::TerminateOutcome> {
    todo!()
}

pub fn restart(workspace: &Workspace, engine: &Engine, record: &SessionRecord) -> crate::Result<SessionRecord> {
    todo!()
}
