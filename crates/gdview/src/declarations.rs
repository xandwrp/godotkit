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

use crate::files::FileQuery;
use crate::project::Project;
use crate::respath::ResPath;

mod index;

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
    /// Parsed from `@rpc(...)` when present.
    pub rpc: Option<RpcConfig>,
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
        let required = self.parameters.iter().filter(|p| p.default.is_none() && !p.is_variadic).count();
        let variadic = self.parameters.iter().any(|p| p.is_variadic);
        (required, (!variadic).then_some(self.parameters.len()))
    }
}

impl Named {
    /// For an `extends` written as a path, the path without quotes.
    pub fn quoted_path(&self) -> Option<&str> {
        let text = self.name.as_str();
        ["\"", "'"].iter().find_map(|quote| text.strip_prefix(quote)?.strip_suffix(quote))
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
    /// Parses `@rpc` argument texts, quoted or not. Unknown arguments are an
    /// error, not ignored. A later argument of the same kind wins, as in Godot.
    pub fn from_arguments(arguments: &[&str]) -> Result<Self, String> {
        let mut config = RpcConfig { mode: RpcMode::Authority, call_local: false, transfer: TransferMode::Unreliable, channel: 0 };
        for argument in arguments {
            let text = argument.trim().trim_matches(|c| c == '"' || c == '\'');
            match text {
                "authority" => config.mode = RpcMode::Authority,
                "any_peer" => config.mode = RpcMode::AnyPeer,
                "call_remote" => config.call_local = false,
                "call_local" => config.call_local = true,
                "unreliable" => config.transfer = TransferMode::Unreliable,
                "unreliable_ordered" => config.transfer = TransferMode::UnreliableOrdered,
                "reliable" => config.transfer = TransferMode::Reliable,
                _ => match text.parse() {
                    Ok(channel) => config.channel = channel,
                    Err(_) => return Err(format!("unknown @rpc argument {argument}")),
                },
            }
        }
        Ok(config)
    }
}

/// Indexes one script from source. Never fails: an unparseable script yields a
/// declaration with `parse_error` set and whatever members were recovered.
pub fn index_script(path: ResPath, source: &str) -> IndexedScript {
    index::script(path, source)
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
                classes.entry(class_name.name.as_str()).or_default().push(&script.declaration);
            }
        }
        classes
    }
}

/// Indexes every `.gd` the project's default file query returns.
pub fn index_project(project: &Project) -> crate::Result<ProjectDeclarations> {
    let mut scripts = Vec::new();
    for file in project.files(&FileQuery::with_extensions(["gd"]))? {
        let Ok(path) = project.localize(&file) else { continue };
        let source = std::fs::read(&file).map_err(|source| crate::Error::Io { path: file.clone(), source })?;
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
