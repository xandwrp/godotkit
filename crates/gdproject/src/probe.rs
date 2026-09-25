//! Talking to a game launched by [`crate::run`] over loopback TCP.
//!
//! Request/response, one JSON line each way, token-authenticated, bounded.
//! The probe answers `status` (frame count, ready), `checkpoints` (project
//! adapter output), and `network` (peers, authority, spawner/synchronizer
//! inventory). Payloads use the `gdview::variant` grammar.
//!
//! # Tests (tests/probe.rs, offline with an in-process fake TCP responder)
//! - `request_sends_token_and_rejects_replies_without_matching_request_id`
//! - `request_times_out_and_reports_connection_refused_distinctly`
//! - `status_reports_frames_and_ready`
//! - `checkpoints_report_adapter_errors_as_status_not_transport_error`
//! - `network_observation_decodes_peers_authority_and_inventory`
//!
//! Engine (`#[ignore]`): `real_engine_probe_reports_ready_and_answers_every_query`,
//! `real_engine_script_error_does_not_freeze_the_probe`.

use std::time::Duration;

use serde::{Deserialize, Serialize};

pub const PROBE_PROTOCOL_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeEndpoint {
    pub port: u16,
    pub token: String,
    pub protocol: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Status {
    pub ready: bool,
    pub frames: u64,
    pub scene: Option<gdview::ResPath>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CheckpointObservation {
    pub observed_unix_ms: u64,
    pub frame: u64,
    pub status: CheckpointStatus,
    pub values: serde_json::Value,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointStatus {
    Collected,
    AdapterError,
    NotConfigured,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NetworkObservation {
    pub observed_unix_ms: u64,
    pub unique_id: Option<i64>,
    pub is_server: Option<bool>,
    pub peers: Vec<i64>,
    pub authorities: Vec<(gdview::respath::NodePath, i64)>,
    pub spawners: Vec<gdview::respath::NodePath>,
    pub synchronizers: Vec<gdview::respath::NodePath>,
}

pub fn status(endpoint: &ProbeEndpoint, deadline: Duration) -> crate::Result<Status> {
    todo!()
}

pub fn checkpoints(
    endpoint: &ProbeEndpoint,
    deadline: Duration,
) -> crate::Result<CheckpointObservation> {
    todo!()
}

pub fn network(endpoint: &ProbeEndpoint, deadline: Duration) -> crate::Result<NetworkObservation> {
    todo!()
}
