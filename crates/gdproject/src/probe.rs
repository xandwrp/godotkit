//! Talking to a live session's runtime probe over loopback TCP.
//!
//! Request/response, one JSON line each way, token-authenticated, bounded.
//! The probe answers `network` (peers, authority, spawners, synchronizers,
//! recent RPC events observed via MultiplayerAPI signals) and `checkpoints`
//! (project adapter output). Payloads use the `gdview::variant` grammar.
//!
//! # Tests (tests/probe.rs, offline with an in-process fake TCP responder)
//! - `request_sends_token_and_rejects_replies_without_matching_request_id`
//! - `request_times_out_and_reports_connection_refused_distinctly`
//! - `network_observation_decodes_peers_authority_and_events`
//! - `checkpoints_report_adapter_errors_as_status_not_transport_error`
//! - `compare_produces_json_pointer_differences_in_stable_order`
//!   Engine (`#[ignore]`): `real_engine_probe_reports_ready_and_answers_network_query`,
//!   `real_engine_script_error_does_not_freeze_the_session`.

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
pub struct NetworkObservation {
    pub observed_unix_ms: u64,
    pub unique_id: Option<i64>,
    pub is_server: Option<bool>,
    pub peers: Vec<i64>,
    pub authorities: Vec<AuthorityEntry>,
    pub events: Vec<NetworkEvent>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AuthorityEntry {
    pub node: gdview::respath::NodePath,
    pub authority: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum NetworkEvent {
    PeerConnected { peer: i64, at_unix_ms: u64 },
    PeerDisconnected { peer: i64, at_unix_ms: u64 },
    ConnectedToServer { at_unix_ms: u64 },
    ServerDisconnected { at_unix_ms: u64 },
    Spawned { node: gdview::respath::NodePath, at_unix_ms: u64 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CheckpointObservation {
    pub observed_unix_ms: u64,
    pub status: CheckpointStatus,
    pub adapter: gdview::ResPath,
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

pub fn observe_network(endpoint: &ProbeEndpoint, deadline: Duration) -> crate::Result<NetworkObservation> {
    todo!()
}

pub fn collect_checkpoints(
    endpoint: &ProbeEndpoint,
    adapter: &gdview::ResPath,
    deadline: Duration,
) -> crate::Result<CheckpointObservation> {
    todo!()
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CheckpointDifference {
    /// RFC 6901 pointer.
    pub pointer: String,
    pub left: Option<serde_json::Value>,
    pub right: Option<serde_json::Value>,
}

pub fn compare(left: &serde_json::Value, right: &serde_json::Value) -> Vec<CheckpointDifference> {
    todo!()
}
