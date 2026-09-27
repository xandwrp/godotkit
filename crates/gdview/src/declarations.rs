//! Declaration index over GDScript sources: what each script declares, without
//! resolving anything. This is the project half of `gdkit api` and the input
//! to `net`. It is deliberately syntactic; type resolution belongs to the engine.
//!
//! # Tests (tests/declarations.rs)
//! - `indexes_class_name_extends_and_members_with_line_numbers`
//! - `res_and_uid_string_literals_are_resource_uses_once`
//! - `records_annotations_per_member_including_rpc_arguments`
//! - `distinguishes_static_private_and_export_members`
//! - `indexes_inner_classes_recursively_with_qualified_names`
//! - `unparseable_scripts_are_reported_not_skipped`
//! - `index_project_uses_file_query_and_is_sorted_by_path`
//! - `by_class_name_detects_duplicate_declarations`
//! - `rpc_config_enforces_categories_and_channel_position`
//! - `rpc_config_preserves_signed_channels_and_decodes_string_literals`
//! - `rpc_config_keeps_unresolved_expressions_explicit`
//! - `rpc_annotation_errors_survive_indexing_including_inner_classes`

use std::collections::BTreeMap;

use serde::Serialize;

use crate::files::FileQuery;
use crate::project::Project;
use crate::respath::ResPath;

mod index;
mod rpc;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ScriptDeclaration {
    pub path: ResPath,
    pub class_name: Option<Named>,
    /// Base as written: `Node`, `"res://base.gd"` (quotes kept), `Outer.Inner`.
    pub extends: Option<Named>,
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
    pub extends: Option<Named>,
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
    /// Parsed from a literal `@rpc(...)` when present and understood.
    pub rpc: Option<RpcConfig>,
    /// Invalid or statically unresolved RPC annotation. This is not a syntax
    /// error: the original annotation arguments remain available to consumers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rpc_error: Option<String>,
    /// Parameters, when `kind` is `Func` or `Signal`.
    pub parameters: Vec<ParameterDeclaration>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ParameterDeclaration {
    pub name: String,
    pub type_text: Option<String>,
    /// Default value source text.
    pub default: Option<String>,
    /// `...rest`
    pub is_variadic: bool,
}

impl MemberDeclaration {
    /// `(required, Some(max))`, or `(required, None)` when variadic.
    pub fn arity(&self) -> (usize, Option<usize>) {
        let required = self
            .parameters
            .iter()
            .filter(|p| p.default.is_none() && !p.is_variadic)
            .count();
        let variadic = self.parameters.iter().any(|p| p.is_variadic);
        (required, (!variadic).then_some(self.parameters.len()))
    }
}

impl Named {
    /// For an `extends` written as a path, the path without quotes.
    pub fn quoted_path(&self) -> Option<&str> {
        let text = self.name.as_str();
        ["\"", "'"]
            .iter()
            .find_map(|quote| text.strip_prefix(quote)?.strip_suffix(quote))
    }
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

/// Source RPC configuration, not a guarantee that a transport accepts it.
/// Defaults are Godot's: authority, call_remote, unreliable, channel 0.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RpcConfig {
    pub mode: RpcMode,
    pub call_local: bool,
    pub transfer: TransferMode,
    /// Signed because Godot preserves negative annotation channels; transport
    /// constraints are not validated by this source index.
    pub channel: i64,
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

/// Indexes one script from source. Never fails: an unparseable script yields a
/// declaration with `parse_error` set and whatever members were recovered.
pub fn index_script(path: ResPath, source: &str) -> IndexedScript {
    index_parsed(path, &crate::syntax::parse(source))
}

/// `index_script` for a caller that already holds the parse, so it is not repeated.
pub fn index_parsed(path: ResPath, parsed: &crate::syntax::Parsed) -> IndexedScript {
    index::script(path, parsed)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct IndexedScript {
    pub declaration: ScriptDeclaration,
    /// First syntax error, `line N: message`.
    pub parse_error: Option<String>,
    /// `preload`, `load`, and `extends` string literals, in source order.
    pub resource_uses: Vec<ResourceUse>,
    /// `$A/B`, `%Unique`, and `get_node("A/B")` with a literal path, in source order.
    pub node_path_uses: Vec<NodePathUse>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ResourceUse {
    pub kind: ResourceUseKind,
    /// As written: `res://…`, `uid://…`, or relative to the script's directory.
    pub path: String,
    pub line: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceUseKind {
    Preload,
    Load,
    Extends,
    /// Any other `res://` or `uid://` string literal, such as a path passed to
    /// `change_scene_to_file` or kept in a constant. Not necessarily a file.
    String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NodePathUse {
    /// As `get_node` receives it: `A/B`, `%Unique/C`, `../X`, `/root/Y`.
    pub path: String,
    pub line: usize,
    /// In the initializer of a top-level `@onready var`, so it is resolved
    /// against the scene before any of the script's own code runs.
    pub onready: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ProjectDeclarations {
    /// Sorted by path.
    pub scripts: Vec<IndexedScript>,
}

impl ProjectDeclarations {
    pub fn by_path(&self, path: &ResPath) -> Option<&ScriptDeclaration> {
        self.indexed(path).map(|script| &script.declaration)
    }
    pub fn indexed(&self, path: &ResPath) -> Option<&IndexedScript> {
        self.scripts
            .binary_search_by(|script| script.declaration.path.cmp(path))
            .ok()
            .map(|index| &self.scripts[index])
    }
    /// `class_name -> declarations` (a Vec so duplicates are visible).
    pub fn by_class_name(&self) -> BTreeMap<&str, Vec<&ScriptDeclaration>> {
        let mut classes: BTreeMap<&str, Vec<&ScriptDeclaration>> = BTreeMap::new();
        for script in &self.scripts {
            if let Some(class_name) = &script.declaration.class_name {
                classes
                    .entry(class_name.name.as_str())
                    .or_default()
                    .push(&script.declaration);
            }
        }
        classes
    }
}

/// Indexes every `.gd` the project's default file query returns.
pub fn index_project(project: &Project) -> crate::Result<ProjectDeclarations> {
    let mut scripts = Vec::new();
    for file in project.files(&FileQuery::with_extensions(["gd"]))? {
        let Ok(path) = project.localize(&file) else {
            continue;
        };
        let source = std::fs::read(&file).map_err(|source| crate::Error::Io {
            path: file.clone(),
            source,
        })?;
        let script = match String::from_utf8(source) {
            Ok(source) => index_script(path, &source),
            Err(_) => IndexedScript {
                parse_error: Some("line 1: not valid UTF-8".into()),
                ..index_script(path, "")
            },
        };
        scripts.push(script);
    }
    scripts.sort_by(|a, b| a.declaration.path.cmp(&b.declaration.path));
    Ok(ProjectDeclarations { scripts })
}
