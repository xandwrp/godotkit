//! Effective AnimationTree/AnimationPlayer inspection via the engine.
//! Offline GLB listing lives in `gdview::gltf`; the CLI calls it directly.
//!
//! # Tests (tests/animation.rs)
//! Offline: `inspection_annotates_tree_nodes_by_full_path_not_leaf_name` (fixture scene with two trees named the same),
//! `scene_argument_accepts_os_or_res_paths_relative_to_cwd`.
//! Engine (`#[ignore]`): `real_engine_reports_graph_parameters_track_targets_and_findings`.

use std::time::Duration;

use gdview::ResPath;
use gdview::respath::NodePath;
use serde::{Deserialize, Serialize};

use crate::engine::Engine;
use crate::workspace::Workspace;

pub const DEFAULT_DEADLINE: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnimationInspection {
    pub schema_version: u32,
    pub scene: ResPath,
    pub trees: Vec<TreeInspection>,
    pub players: Vec<PlayerInspection>,
    pub findings: Vec<Finding>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TreeInspection {
    pub path: NodePath,
    pub source_line: Option<usize>,
    pub root_node_type: Option<String>,
    pub player: Option<NodePath>,
    pub parameters: Vec<String>,
    pub graph: serde_json::Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlayerInspection {
    pub path: NodePath,
    pub animations: Vec<String>,
    pub track_targets: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub severity: crate::diagnostics::Severity,
    pub at: Option<NodePath>,
    pub message: String,
}

pub fn inspect(
    workspace: &Workspace,
    engine: &Engine,
    scene: &ResPath,
    tree: Option<&NodePath>,
    deadline: Duration,
) -> crate::Result<AnimationInspection> {
    todo!()
}
