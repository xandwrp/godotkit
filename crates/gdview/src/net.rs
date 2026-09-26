//! Offline multiplayer observations, not runtime compatibility or lint verdicts.
//! `scan_script` reads syntax; `analyze` links supplied observations and authored
//! scenes; `analyze_project` performs read-only discovery. Unknowns stay explicit.
//! See docs/NET_SCOPE.md for the delivered contract and explicit limits.
//!
//! # Acceptance tests (tests/net.rs; all implemented, offline)
//! - `finds_rpc_endpoints_from_annotations_with_godot_defaults`
//! - `finds_rpc_calls_in_every_form`
//! - `does_not_classify_os_get_unique_id_as_authority_use`
//! - `finds_spawners_and_synchronizers_with_replication_config_properties`
//! - `old_format_replication_configs_respect_sync_false`
//! - `scene_anchors_match_set_multiplayer_subtree_roots`
//! - `receiver_resolution_prefers_same_scene_before_global_labels`
//! - `inner_class_rpcs_are_reported`
//! - `report_json_is_deterministic_across_runs`
//! - `explain_matches_method_receiver_method_scene_and_node_queries`
//!
//! Additional regression gates: net_edges.rs, net_paths.rs, net_schema.rs, net_syntax.rs;
//! CLI no-engine/no-write tests in tests/net_cli.rs; shared-fixture differential
//! check in gdproject/tests/net_real.rs (opt-in engine, executed on scratch only).

use crate::autoload::ResolvedAutoload;
use crate::respath::{NodePath, ResPath};
use crate::scene::SceneFile;
use crate::uid::UidMap;
use serde::Serialize;

mod analysis;
mod explanation;
mod project;
mod replication;
mod source;
pub use analysis::analyze;
pub use explanation::explain;
pub use project::analyze_project;
pub use source::scan_script;

/// Refactor shape; deliberately distinct from legacy's incompatible version 2.
pub const NET_REPORT_SCHEMA_VERSION: u32 = 3;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NetReport {
    pub schema_version: u32,
    pub coverage: Coverage,
    pub endpoints: Vec<RpcEndpoint>,
    pub calls: Vec<RpcCall>,
    pub anchors: Vec<ScriptAnchor>,
    pub contexts: Vec<MultiplayerContext>,
    pub spawners: Vec<Spawner>,
    pub synchronizers: Vec<Synchronizer>,
    pub authority_uses: Vec<AuthorityUse>,
    pub autoloads: Vec<NetAutoload>,
    /// Sorted and deduplicated. Unknowns do not make this observation report fail.
    pub unknowns: Vec<Unknown>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Coverage {
    pub scripts_scanned: usize,
    pub scenes_scanned: usize,
    pub resources_scanned: usize,
    pub limitations: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct SourceLocation {
    #[serde(rename = "resource")]
    pub path: ResPath,
    pub line: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RpcEndpoint {
    pub script: ResPath,
    /// Qualified inner class, None for the script's outer class.
    pub class: Option<String>,
    pub method: String,
    pub config: crate::declarations::RpcConfig,
    pub location: SourceLocation,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RpcCall {
    pub location: SourceLocation,
    pub class: Option<String>,
    pub form: CallForm,
    /// Entire authored call, preserving dynamic method/peer expressions.
    pub expression: String,
    /// Receiver text, None for implicit self. For unresolved Callables this is
    /// the callable expression, not a proven target node.
    pub receiver: Option<String>,
    /// A local/non-alias member binding must not accidentally resolve to an
    /// autoload or @onready field.
    pub receiver_is_local: bool,
    pub method: Option<String>,
    pub target_peer: Option<String>,
    /// Source candidates, NOT proof of runtime compatibility. Indexes into the
    /// enclosing report's endpoints (remapped for Explanation).
    pub candidates: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CallForm {
    Rpc,
    RpcId,
    CallableRpc,
    MultiplayerRpc,
    /// Syntax alone cannot distinguish Node.rpc from Callable.rpc or a custom method.
    AmbiguousRpc,
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
    pub spawn: Option<bool>,
    pub mode: Option<SyncMode>,
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
    pub class: Option<String>,
    pub call: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NetAutoload {
    #[serde(flatten)]
    pub autoload: ResolvedAutoload,
    /// Positive source evidence only; false is not proof of no networking.
    pub networked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Unknown {
    pub location: Option<SourceLocation>,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ScriptAnchor {
    pub script: ResPath,
    pub scene: Option<ResPath>,
    /// Relative to the authored scene root; absolute for script autoloads.
    pub node: NodePath,
    /// Only established for autoload trees. Ordinary scene placement is unknown.
    pub runtime_path: Option<NodePath>,
    /// Observed contexts matching a known runtime path, never assumed active.
    pub context_candidates: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MultiplayerContext {
    pub location: SourceLocation,
    pub api: Option<String>,
    /// Empty means the API's default context; None means dynamic/invalid.
    pub root: Option<NodePath>,
}

/// Parsed source observations, with no file I/O or endpoint linking.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ScriptObservations {
    pub script: ResPath,
    pub endpoints: Vec<RpcEndpoint>,
    pub calls: Vec<RpcCall>,
    pub authority_uses: Vec<AuthorityUse>,
    pub contexts: Vec<MultiplayerContext>,
    pub unknowns: Vec<Unknown>,
    /// Direct @onready node aliases only, not a dataflow claim.
    pub bindings: Vec<NodeBinding>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NodeBinding {
    pub class: Option<String>,
    pub name: String,
    pub path: NodePath,
}

/// All inputs are caller-owned and read-only. Text resources are included in
/// scenes so external replication configs can be inspected without engine loads.
pub struct NetInput<'a> {
    pub scripts: &'a [ScriptObservations],
    pub scenes: &'a [(ResPath, SceneFile)],
    pub autoloads: &'a [ResolvedAutoload],
    pub uids: &'a UidMap,
    pub unknowns: &'a [Unknown],
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Explanation {
    pub schema_version: u32,
    pub query: String,
    pub matched: bool,
    pub contexts: Vec<MultiplayerContext>,
    pub endpoints: Vec<RpcEndpoint>,
    pub calls: Vec<RpcCall>,
    pub anchors: Vec<ScriptAnchor>,
    pub synchronizers: Vec<Synchronizer>,
    pub spawners: Vec<Spawner>,
    pub authority_uses: Vec<AuthorityUse>,
    pub unknowns: Vec<Unknown>,
    pub notes: Vec<String>,
}

fn unknown(unknowns: &mut Vec<Unknown>, path: &ResPath, line: usize, message: impl Into<String>) {
    unknowns.push(Unknown {
        location: Some(SourceLocation {
            path: path.clone(),
            line,
        }),
        message: message.into(),
    });
}
