//! Declarative multiplayer topologies on top of `session`.
//!
//! Start flow:
//! 1. Validate config; allocate a run record (`Starting`).
//! 2. Launch `server` participants; poll their readiness checkpoint until the deadline.
//!    Adapter errors and transport timeouts during polling mean "not ready yet", not failure.
//! 3. Resolve `{port.*}` placeholders from server checkpoints; launch `client`s; poll readiness.
//! 4. Launch `late_client`s the same way. Persist `Ready`.
//!   Any failure: stop every participant launched so far (kill after grace), persist `Failed` with the reason.
//!
//! # Tests (tests/scenario.rs, offline with `fake-godot` scripted per participant)
//! - `config_validation_rejects_unknown_roles_duplicate_names_and_bad_pointers`
//! - `argument_expansion_substitutes_ports_and_rejects_unknown_placeholders`
//! - `start_orders_server_then_clients_then_late_clients`
//! - `readiness_polling_tolerates_adapter_errors_until_deadline`
//! - `participant_exit_before_readiness_fails_fast_with_its_log_path`
//! - `failure_stops_everything_launched_and_records_the_cause`
//! - `disconnect_and_crash_operate_on_the_participants_current_generation`
//! - `stop_falls_back_to_kill_and_still_reports_success`
//! - `status_re_checks_liveness`

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::engine::Engine;
use crate::records::Record;
use crate::workspace::Workspace;

pub const SCENARIO_RUN_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioConfig {
    pub transport: Transport,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub ports: BTreeMap<String, PortSource>,
    pub participants: Vec<ParticipantConfig>,
}

fn default_timeout() -> u64 {
    20
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    DedicatedEnet,
    SteamP2p,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortSource {
    /// JSON pointer into the server's checkpoints, e.g. `/network_session/port`.
    pub checkpoint: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParticipantConfig {
    pub name: String,
    pub role: Role,
    #[serde(default)]
    pub scene: Option<gdview::ResPath>,
    #[serde(default)]
    pub arguments: Vec<String>,
    #[serde(default)]
    pub headless: Option<bool>,
    pub readiness: Readiness,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Server,
    Client,
    LateClient,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Readiness {
    pub path: String,
    pub equals: serde_json::Value,
}

impl ScenarioConfig {
    pub fn validate(&self, name: &str) -> crate::Result<()> {
        todo!()
    }
}

/// Substitutes `{port.name}`; literal braces are written `{{`/`}}`.
pub fn expand_argument(argument: &str, ports: &BTreeMap<String, u16>) -> crate::Result<String> {
    todo!()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScenarioRun {
    pub schema_version: u32,
    pub scenario: String,
    pub started_unix_ms: u64,
    pub state: RunState,
    pub ports: BTreeMap<String, u16>,
    pub participants: Vec<ParticipantRun>,
    pub failure: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Starting,
    Ready,
    Failed,
    Stopped,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParticipantRun {
    pub name: String,
    pub role: Role,
    pub session: String,
    pub generation: u32,
    pub ready: bool,
    pub log_path: PathBuf,
    pub last_readiness_value: Option<serde_json::Value>,
}

impl Record for ScenarioRun {
    const SCHEMA_VERSION: u32 = SCENARIO_RUN_SCHEMA_VERSION;
    fn schema_version(&self) -> u32 {
        self.schema_version
    }
    fn file_stem(&self) -> String {
        format!("{}-{}", self.started_unix_ms, self.scenario)
    }
}

pub fn start(workspace: &Workspace, engine: &Engine, name: &str) -> crate::Result<ScenarioRun> {
    todo!()
}

pub fn status(workspace: &Workspace, name: &str) -> crate::Result<ScenarioRun> {
    todo!()
}

pub fn disconnect(workspace: &Workspace, name: &str, participant: &str, grace: Duration) -> crate::Result<ScenarioRun> {
    todo!()
}

pub fn crash(workspace: &Workspace, name: &str, participant: &str) -> crate::Result<ScenarioRun> {
    todo!()
}

pub fn stop(workspace: &Workspace, name: &str, grace: Duration) -> crate::Result<ScenarioRun> {
    todo!()
}
