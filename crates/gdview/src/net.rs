//! Static multiplayer analysis: what the project's sources say about RPCs,
//! replication, and authority. Observations, not lint. Unknowns stay explicit.
//! Pure function of declarations + scenes + autoloads. No engine is involved;
//! `gdkit net` is entirely offline.
//!
//! # Tests (tests/net.rs)
//! - `finds_rpc_endpoints_from_annotations_with_godot_defaults`
//! - `finds_rpc_calls_in_every_form` (`rpc("m")`, `rpc_id(1,"m")`, `self.rpc`, `node.rpc_id`, `Callable.rpc`, `multiplayer.rpc`)
//! - `does_not_classify_os_get_unique_id_as_authority_use`
//! - `finds_spawners_and_synchronizers_with_replication_config_properties`
//! - `old_format_replication_configs_respect_sync_false`
//! - `scene_anchors_match_set_multiplayer_subtree_roots`
//! - `receiver_resolution_prefers_same_scene_before_global_labels`
//! - `inner_class_rpcs_are_reported`
//! - `report_json_is_deterministic_across_runs` (no HashMap iteration)
//! - `explain_matches_method_receiver_method_scene_and_node_queries`

use serde::Serialize;

use crate::declarations::{ProjectDeclarations, RpcConfig};
use crate::respath::{NodePath, ResPath};
use crate::scene::SceneFile;

pub const NET_REPORT_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct NetReport {
    pub schema_version: u32,
    pub endpoints: Vec<RpcEndpoint>,
    pub calls: Vec<RpcCall>,
    pub spawners: Vec<Spawner>,
    pub synchronizers: Vec<Synchronizer>,
    pub authority_uses: Vec<AuthorityUse>,
    pub autoloads: Vec<NetAutoload>,
    /// Sorted, deduplicated.
    pub unknowns: Vec<Unknown>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SourceLocation {
    pub path: ResPath,
    pub line: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RpcEndpoint {
    pub script: ResPath,
    /// `Outer.Inner` for inner classes, `None` at top level.
    pub class: Option<String>,
    pub method: String,
    pub config: RpcConfig,
    pub location: SourceLocation,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RpcCall {
    pub location: SourceLocation,
    pub form: CallForm,
    /// Receiver expression text, `None` for implicit self.
    pub receiver: Option<String>,
    pub method: Option<String>,
    pub target_peer: Option<String>,
    /// Endpoints this call could reach, by index into `NetReport::endpoints`.
    pub candidates: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CallForm {
    Rpc,
    RpcId,
    CallableRpc,
    MultiplayerRpc,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Spawner {
    pub scene: ResPath,
    pub node: NodePath,
    pub spawn_path: Option<NodePath>,
    pub auto_spawn_list: Vec<ResPath>,
    pub line: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Synchronizer {
    pub scene: ResPath,
    pub node: NodePath,
    pub root_path: Option<NodePath>,
    pub properties: Vec<SyncedProperty>,
    pub line: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SyncedProperty {
    pub path: NodePath,
    pub mode: SyncMode,
    pub watch: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncMode {
    Never,
    Always,
    OnChange,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AuthorityUse {
    pub location: SourceLocation,
    pub call: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NetAutoload {
    pub name: String,
    pub path: ResPath,
    pub networked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Unknown {
    pub location: Option<SourceLocation>,
    pub message: String,
}

impl PartialOrd for SourceLocation {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for SourceLocation {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (&self.path, self.line).cmp(&(&other.path, other.line))
    }
}
impl Eq for SourceLocation {}

/// Scenes to analyze, already parsed. Callers decide which files to load.
pub struct NetInput<'a> {
    pub declarations: &'a ProjectDeclarations,
    pub scenes: Vec<(ResPath, &'a SceneFile)>,
    pub autoloads: &'a crate::autoload::Autoloads,
}

pub fn analyze(input: &NetInput<'_>) -> NetReport {
    todo!()
}


#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Explanation {
    pub query: String,
    pub endpoints: Vec<RpcEndpoint>,
    pub calls: Vec<RpcCall>,
    pub synchronizers: Vec<Synchronizer>,
    pub spawners: Vec<Spawner>,
    pub notes: Vec<String>,
}

/// `method`, `receiver.method`, a scene path, or a replication node path.
pub fn explain(report: &NetReport, query: &str) -> Explanation {
    todo!()
}
