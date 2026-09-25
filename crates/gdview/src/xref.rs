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
//! - the same `uid://` claimed by two files; the same `class_name` declared twice
//! - a `.tscn`/`.tres` that does not parse
//!
//! Every check is conservative: when the answer depends on something gdview
//! cannot see (a native method, a node added at runtime, a scene that does not
//! parse), there is no finding rather than a false one. In particular:
//! - Node paths are checked only in `@onready` initializers, which run before
//!   any of the script's own code can add children, and only against scenes
//!   that attach the script (directly, through a subclass, or by inheritance).
//! - A connection handler missing from the project's scripts is reported only
//!   when it cannot be a native method: its name starts with `_` (native
//!   underscore methods are virtuals, callable only when a script implements them).
//! - Unknown engine classes are left to the engine phase; gdview has no class list.
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
use crate::files::FileQuery;
use crate::project::Project;
use crate::respath::{NodePath, ResPath};
use crate::scene::SceneFile;
use crate::uid::UidMap;

mod analysis;

/// Everything `xref` needs, loaded once by the caller so `check` and `refs` share it.
pub struct ProjectGraph<'a> {
    pub project: &'a Project,
    pub declarations: &'a ProjectDeclarations,
    /// Sorted by path.
    pub scenes: Vec<(ResPath, SceneFile)>,
    pub uids: &'a UidMap,
    /// `.tscn`/`.tres` files that did not parse, as `UnparseableScene` findings.
    pub unparseable: Vec<Finding>,
    /// Every project file, sorted; used for "moved to" suggestions.
    pub files: Vec<ResPath>,
}

impl<'a> ProjectGraph<'a> {
    /// Parses every `.tscn`/`.tres` the file query returns. Parse failures become findings.
    pub fn load(
        project: &'a Project,
        declarations: &'a ProjectDeclarations,
        uids: &'a UidMap,
    ) -> crate::Result<Self> {
        let mut files = Vec::new();
        for file in project.files(&FileQuery::default())? {
            if let Ok(path) = project.localize(&file) {
                files.push(path);
            }
        }
        files.sort();
        let mut scenes = Vec::new();
        let mut unparseable = Vec::new();
        for path in &files {
            if !matches!(
                path.extension().map(str::to_ascii_lowercase).as_deref(),
                Some("tscn" | "tres")
            ) {
                continue;
            }
            let parsed = project
                .read_to_string(path)
                .and_then(|source| crate::scene::parse(&source));
            match parsed {
                Ok(scene) => scenes.push((path.clone(), scene)),
                Err(error) => {
                    let (line, message) = match error {
                        crate::Error::Parse { line, message, .. } => (line, message),
                        other => (1, other.to_string()),
                    };
                    unparseable.push(Finding {
                        kind: FindingKind::UnparseableScene,
                        at: Location {
                            path: path.clone(),
                            line,
                        },
                        message: format!("cannot be parsed: {message}"),
                        target: path.to_string(),
                        suggestions: Vec::new(),
                    });
                }
            }
        }
        Ok(Self {
            project,
            declarations,
            scenes,
            uids,
            unparseable,
            files,
        })
    }

    pub fn scene(&self, path: &ResPath) -> Option<&SceneFile> {
        self.scenes
            .binary_search_by(|(candidate, _)| candidate.cmp(path))
            .ok()
            .map(|index| &self.scenes[index].1)
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
    DuplicateUid,
    DuplicateClassName,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Location {
    pub path: ResPath,
    pub line: usize,
}

/// Every finding, sorted by location, then kind, then target. Deterministic.
pub fn analyze(graph: &ProjectGraph<'_>) -> Vec<Finding> {
    analysis::run(graph)
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
