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
//! - `references_to_a_directory_cover_every_file_under_it`
//! - `references_to_a_missing_path_find_what_a_move_left_behind`
//! - `findings_are_sorted_by_location_and_deterministic`

use serde::Serialize;

use crate::declarations::ProjectDeclarations;
use crate::files::FileQuery;
use crate::project::Project;
use crate::respath::{NodePath, ResPath, Uid};
use crate::scene::SceneFile;
use crate::uid::UidMap;

mod analysis;
mod references;

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
    #[serde(rename = "resource")]
    pub path: ResPath,
    pub line: usize,
}

/// Every finding, sorted by location, then kind, then target. Deterministic.
pub fn analyze(graph: &ProjectGraph<'_>) -> Vec<Finding> {
    analysis::run(graph)
}

/// One place a file is referenced from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Reference {
    /// Serialized flat as `resource` and `line`, like every gdkit report.
    #[serde(flatten)]
    pub at: Location,
    pub kind: ReferenceKind,
    /// The file referenced. Differs from the query when the query is a directory.
    pub target: ResPath,
    /// Node path inside the scene, when the reference is a node's script, instance, or property.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<NodePath>,
    /// The property named, for [`ReferenceKind::Property`], or the
    /// `section/key` in project.godot. The location of a property is its
    /// node's or sub-resource's header line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Resolved through a `uid://`, so it keeps working after a move as long as
    /// the file's uid moves with it (its `.uid` sidecar, `.import` file, or
    /// resource header). A reference by path alone breaks.
    pub by_uid: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceKind {
    /// An `[ext_resource]` line in a `.tscn`/`.tres`: the line a move rewrites.
    ExtResource,
    /// A node's `script` in a scene, or a resource's, through an ext_resource.
    Script,
    /// A node that instances the scene, through an ext_resource.
    Instance,
    /// The scene's root instances it: the scene inherits from it.
    Inherits,
    /// `instance_placeholder="res://…"` on a node.
    Placeholder,
    /// A string property value in a scene or resource.
    Property,
    Preload,
    Load,
    Extends,
    /// Any other `res://` or `uid://` string literal in a script.
    String,
    Autoload,
    MainScene,
    /// Any other project.godot value naming it, such as the icon, a theme,
    /// a bus layout, a translation, or an enabled plugin.
    ProjectSetting,
}

/// Every place `path`, or with a directory any file under it, is referenced:
/// scenes and resources (ext_resource, script, instance, placeholder, string
/// properties), scripts (extends, preload, load, other path strings), and
/// `project.godot` (autoloads, main scene, other settings). `path` need not
/// exist, so references left behind by a move are still found. Sorted by
/// location, then kind.
pub fn references_to(graph: &ProjectGraph<'_>, path: &ResPath) -> crate::Result<Vec<Reference>> {
    references::find(graph, path)
}

/// What `gdkit refs` reports: the references, and what a move of the file has
/// to carry along with it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Refs {
    pub path: ResPath,
    pub exists: bool,
    pub directory: bool,
    /// The file's own uid, which references `by_uid` resolve through.
    pub uid: Option<Uid>,
    /// `.uid` and `.import` files next to it that hold that uid and must move with it.
    pub sidecars: Vec<ResPath>,
    /// The script's `class_name`. Uses by that name are not listed: they follow
    /// a move, but not a deletion.
    pub class_name: Option<String>,
    pub references: Vec<Reference>,
    /// When `path` does not exist: where it may have moved.
    pub suggestions: Vec<String>,
}

pub fn refs(graph: &ProjectGraph<'_>, path: &ResPath) -> crate::Result<Refs> {
    let project = graph.project;
    let exists = project.exists(path);
    let directory = project.globalize(path).is_dir();
    let sidecars = match directory {
        true => Vec::new(),
        false => ["uid", "import"]
            .into_iter()
            .filter_map(|extension| {
                let parent = path.parent()?;
                parent
                    .join(&format!("{}.{extension}", path.file_name()))
                    .ok()
            })
            .filter(|sidecar| project.exists(sidecar))
            .collect(),
    };
    Ok(Refs {
        path: path.clone(),
        exists,
        directory,
        uid: graph.uids.uid_of(path).cloned(),
        sidecars,
        class_name: graph
            .declarations
            .by_path(path)
            .and_then(|script| script.class_name.as_ref())
            .map(|named| named.name.clone()),
        references: references_to(graph, path)?,
        suggestions: match exists {
            true => Vec::new(),
            false => graph.suggest_files(path),
        },
    })
}
