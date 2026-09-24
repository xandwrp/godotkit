use serde::{Deserialize, Serialize};

pub const NET_REPORT_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NetReport {
    pub schema_version: u32,
    pub project: String,
    pub engine: NetEngine,
    pub coverage: NetCoverage,
    pub autoloads: Vec<NetAutoload>,
    pub multiplayer_contexts: Vec<MultiplayerContext>,
    pub rpc_endpoints: Vec<RpcEndpoint>,
    pub rpc_calls: Vec<RpcCall>,
    pub rpc_contracts: Vec<RpcContract>,
    pub peer_constructions: Vec<SourceFinding>,
    pub peer_assignments: Vec<SourceFinding>,
    pub lifecycle: Vec<LifecycleFinding>,
    pub authority: Vec<SourceFinding>,
    pub authority_assignments: Vec<AuthorityAssignment>,
    pub replication_nodes: Vec<ReplicationNode>,
    pub unknowns: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NetEngine {
    pub executable: String,
    pub version: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NetCoverage {
    pub scripts_scanned: usize,
    pub scenes_scanned: usize,
    pub languages: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NetAutoload {
    pub index: usize,
    pub name: String,
    pub path: String,
    pub resolved_path: Option<String>,
    pub singleton: bool,
    pub networked: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RpcEndpoint {
    pub method: String,
    pub signature: Option<String>,
    pub source: SourceLocation,
    pub rpc_mode: String,
    pub call: String,
    pub transfer_mode: String,
    pub channel: i64,
    pub inherited: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RpcCall {
    pub method: String,
    pub kind: String,
    pub expression: String,
    pub receiver: Option<String>,
    pub target: Option<String>,
    pub source: SourceLocation,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MultiplayerContext {
    pub subtree_root: String,
    pub multiplayer_api: String,
    pub source: Option<SourceLocation>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RpcContract {
    pub call: RpcCall,
    pub compatible_endpoints: Vec<RpcContractEndpoint>,
    pub unresolved_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RpcContractEndpoint {
    pub endpoint: RpcEndpoint,
    pub receiver_path: Option<String>,
    pub multiplayer_root: String,
    pub recipient: String,
    pub stable_path: String,
    pub sender_identity: Vec<SourceFinding>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SourceFinding {
    pub value: String,
    pub source: SourceLocation,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LifecycleFinding {
    pub signal: String,
    pub operation: String,
    pub source: SourceLocation,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReplicationNode {
    pub kind: String,
    pub node_path: String,
    pub scene: String,
    pub root_path: Option<String>,
    pub spawn_path: Option<String>,
    pub spawn_limit: Option<i64>,
    pub spawnable_scenes: Vec<String>,
    pub replication_properties: Vec<ReplicationProperty>,
    pub related_nodes: Vec<String>,
    pub authority_assignments: Vec<AuthorityAssignment>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReplicationProperty {
    pub path: String,
    pub spawn: bool,
    pub sync: bool,
    pub mode: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AuthorityAssignment {
    pub node: String,
    pub authority: String,
    pub recursive: bool,
    pub source: SourceLocation,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct SourceLocation {
    pub path: String,
    pub line: usize,
}
