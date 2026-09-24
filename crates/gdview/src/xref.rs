//! Static cross-file checks: the editor's red text that is not a parse error.
//! Runs in milliseconds with no engine, as phase 0 of `gdkit check` and behind
//! `gdkit refs`.
//!
//! Findings, each with the referencing location and the missing target:
//! - a scene connection to a method the target node's script does not declare
//! - `$A/B`, `%Unique`, `get_node("A/B")` in a script with no such node in any scene that owns the script
//! - `preload("res://…")` / `load("res://…")` string literals that point at nothing
//! - `[ext_resource path=…]`, `instance=`, and `uid://` references that do not resolve
//! - `extends "res://…"` to a missing script; `class_name` references to undeclared classes
//! - `.import` sidecar missing for an imported asset that a scene references
//!
//! Node-path checks are conservative: a script owned by no scene, or reached via
//! a dynamic path, yields no finding rather than a false one.
//!
//! # Tests (tests/xref.rs)
//! - `connection_to_missing_method_is_reported_with_scene_line_and_script`
//! - `connection_arity_mismatch_is_reported_when_binds_are_known`
//! - `node_path_in_onready_is_checked_against_every_owning_scene`
//! - `unique_name_paths_resolve_through_unique_name_in_owner`
//! - `dynamic_or_unowned_node_paths_produce_no_finding`
//! - `missing_preload_ext_resource_instance_and_uid_targets_are_reported`
//! - `extends_path_and_class_name_references_are_checked`
//! - `references_to_lists_every_inbound_reference_for_a_path`
//! - `findings_are_sorted_by_location_and_deterministic`

use serde::Serialize;

use crate::declarations::ProjectDeclarations;
use crate::project::Project;
use crate::respath::{NodePath, ResPath};
use crate::scene::SceneFile;
use crate::uid::UidMap;

/// Everything `xref` needs, loaded once by the caller so `check` and `refs` share it.
pub struct ProjectGraph<'a> {
    pub project: &'a Project,
    pub declarations: &'a ProjectDeclarations,
    pub scenes: Vec<(ResPath, SceneFile)>,
    pub uids: &'a UidMap,
}

impl<'a> ProjectGraph<'a> {
    /// Parses every `.tscn`/`.tres` the file query returns. Parse failures become findings.
    pub fn load(project: &'a Project, declarations: &'a ProjectDeclarations, uids: &'a UidMap) -> crate::Result<Self> {
        todo!()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub kind: FindingKind,
    pub at: Location,
    pub message: String,
    /// The thing that could not be found, for `did you mean` and for grep.
    pub target: String,
    pub suggestions: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    MissingMethod,
    MethodArity,
    MissingNode,
    MissingResource,
    UnresolvedUid,
    MissingBaseScript,
    UnknownClass,
    UnparseableScene,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Location {
    pub path: ResPath,
    pub line: usize,
}

pub fn analyze(graph: &ProjectGraph<'_>) -> Vec<Finding> {
    todo!()
}

/// Every place `path` is referenced: scenes (ext_resource, instance, script),
/// scripts (preload/load/extends), `project.godot` (autoloads, main scene).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Reference {
    pub at: Location,
    pub kind: ReferenceKind,
    /// Node path inside the scene, when the reference is a node's script or instance.
    pub node: Option<NodePath>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceKind {
    ExtResource,
    Instance,
    Script,
    Preload,
    Load,
    Extends,
    Autoload,
    MainScene,
    Uid,
}

pub fn references_to(graph: &ProjectGraph<'_>, path: &ResPath) -> Vec<Reference> {
    todo!()
}
