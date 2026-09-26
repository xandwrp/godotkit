//! Autoload declarations in initialization order.
//!
//! # Tests (tests/settings.rs covers parsing; tests/autoload.rs)
//! - `resolve_lists_zero_based_order_kind_and_path`
//! - `scripts_vs_scenes_are_distinguished_by_extension`

use serde::Serialize;

use crate::project::Project;
use crate::respath::{ResPath, Uid};
use crate::uid::UidMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Autoload {
    pub name: String,
    pub target: AutoloadTarget,
    /// `*` prefix in project.godot: registered as a global singleton.
    pub singleton: bool,
}

/// What an autoload loads: Godot writes a `res://` path, or since 4.4 often a `uid://`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum AutoloadTarget {
    Path(ResPath),
    Uid(Uid),
}

impl Autoload {
    /// The loaded file, resolving a `uid://` through `uids`.
    pub fn path<'a>(&'a self, uids: &'a UidMap) -> Option<&'a ResPath> {
        match &self.target {
            AutoloadTarget::Path(path) => Some(path),
            AutoloadTarget::Uid(uid) => uids.resolve(uid),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Autoloads(pub Vec<Autoload>);

impl Autoloads {
    pub fn iter(&self) -> impl Iterator<Item = &Autoload> {
        self.0.iter()
    }
    pub fn by_name(&self, name: &str) -> Option<&Autoload> {
        self.0.iter().find(|autoload| autoload.name == name)
    }
}

impl Autoloads {
    /// Each autoload with its load order, file, and kind, `uid://`s resolved.
    pub fn resolve(&self, project: &Project, uids: &UidMap) -> Vec<ResolvedAutoload> {
        self.iter()
            .enumerate()
            .map(|(order, autoload)| {
                let path = autoload.path(uids).cloned();
                let kind = match &path {
                    None => AutoloadKind::Unresolved,
                    Some(path) => AutoloadKind::of(path),
                };
                ResolvedAutoload {
                    order,
                    name: autoload.name.clone(),
                    singleton: autoload.singleton,
                    kind,
                    exists: path.as_ref().is_some_and(|path| project.exists(path)),
                    uid: match &autoload.target {
                        AutoloadTarget::Uid(uid) => Some(uid.clone()),
                        AutoloadTarget::Path(_) => None,
                    },
                    path,
                }
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ResolvedAutoload {
    /// Zero-based. Godot adds autoloads to the tree, and runs their `_init`/`_ready`, in this order.
    pub order: usize,
    pub name: String,
    /// Reachable by name from every script.
    pub singleton: bool,
    pub kind: AutoloadKind,
    /// The file it loads; `None` when its `uid://` resolves to no file.
    pub path: Option<ResPath>,
    /// As written in project.godot, when it names a uid.
    pub uid: Option<Uid>,
    pub exists: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoloadKind {
    /// `.gd` or `.cs`: Godot instances a `Node` with the script attached.
    Script,
    /// `.tscn` or `.scn`: Godot instances the scene.
    Scene,
    /// Any other extension, such as a script in a language an extension adds.
    Other,
    /// A `uid://` that no project file claims.
    Unresolved,
}

impl AutoloadKind {
    /// By extension, case-insensitively, as the engine decides.
    pub fn of(path: &ResPath) -> Self {
        match path.extension().map(str::to_ascii_lowercase).as_deref() {
            Some("gd" | "cs") => Self::Script,
            Some("tscn" | "scn") => Self::Scene,
            _ => Self::Other,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Script => "script",
            Self::Scene => "scene",
            Self::Other => "other",
            Self::Unresolved => "unresolved",
        }
    }
}
