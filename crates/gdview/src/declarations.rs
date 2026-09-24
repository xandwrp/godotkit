//! Declaration index over GDScript sources: what each script declares, without
//! resolving anything. This is the project half of `gdkit api` and the input
//! to `net`. It is deliberately syntactic; type resolution belongs to the engine.
//!
//! # Tests (tests/declarations.rs)
//! - `indexes_class_name_extends_and_members_with_line_numbers`
//! - `records_annotations_per_member_including_rpc_arguments`
//! - `distinguishes_static_private_and_export_members`
//! - `indexes_inner_classes_recursively_with_qualified_names`
//! - `unparseable_scripts_are_reported_not_skipped`
//! - `index_project_uses_file_query_and_is_sorted_by_path`
//! - `by_class_name_detects_duplicate_declarations`

use std::collections::BTreeMap;

use serde::Serialize;

use crate::project::Project;
use crate::respath::ResPath;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ScriptDeclaration {
    pub path: ResPath,
    pub class_name: Option<Named>,
    pub extends: Option<String>,
    pub is_tool: bool,
    pub members: Vec<MemberDeclaration>,
    pub inner_classes: Vec<InnerClassDeclaration>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Named {
    pub name: String,
    pub line: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InnerClassDeclaration {
    /// `Outer.Inner`
    pub qualified_name: String,
    pub line: usize,
    pub extends: Option<String>,
    pub members: Vec<MemberDeclaration>,
    pub inner_classes: Vec<InnerClassDeclaration>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MemberDeclaration {
    pub kind: MemberKind,
    pub name: String,
    pub line: usize,
    pub is_static: bool,
    /// Leading underscore.
    pub is_private: bool,
    pub type_text: Option<String>,
    pub annotations: Vec<AnnotationDeclaration>,
    /// Parsed from `@rpc(...)` when present.
    pub rpc: Option<RpcConfig>,
    /// Function parameters, when `kind == Func`.
    pub parameters: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberKind {
    Signal,
    Const,
    Var,
    Func,
    Enum,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AnnotationDeclaration {
    pub name: String,
    pub arguments: Vec<String>,
}

/// `@rpc` as Godot interprets it. Defaults are Godot's: authority, call_remote, unreliable, channel 0.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RpcConfig {
    pub mode: RpcMode,
    pub call_local: bool,
    pub transfer: TransferMode,
    pub channel: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RpcMode {
    Authority,
    AnyPeer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferMode {
    Unreliable,
    UnreliableOrdered,
    Reliable,
}

impl RpcConfig {
    /// Parses `@rpc` argument texts. Unknown arguments are an error, not ignored.
    pub fn from_arguments(arguments: &[&str]) -> Result<Self, String> {
        todo!()
    }
}

/// Indexes one script from source. Never fails: an unparseable script yields a
/// declaration with `parse_error` set and whatever members were recovered.
pub fn index_script(path: ResPath, source: &str) -> IndexedScript {
    todo!()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct IndexedScript {
    pub declaration: ScriptDeclaration,
    pub parse_error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ProjectDeclarations {
    /// Sorted by path.
    pub scripts: Vec<IndexedScript>,
}

impl ProjectDeclarations {
    pub fn by_path(&self, path: &ResPath) -> Option<&ScriptDeclaration> {
        todo!()
    }
    /// `class_name -> declarations` (a Vec so duplicates are visible).
    pub fn by_class_name(&self) -> BTreeMap<&str, Vec<&ScriptDeclaration>> {
        todo!()
    }
}

/// Indexes every `.gd` the project's default file query returns.
pub fn index_project(project: &Project) -> crate::Result<ProjectDeclarations> {
    todo!()
}
