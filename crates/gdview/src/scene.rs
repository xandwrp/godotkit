//! Text scene (`.tscn`) and text resource (`.tres`) files as data.
//!
//! This replaces every ad-hoc `.tscn` scan in the old code (v0.1 net.rs and friends).
//! Node properties, sub-resources, ext-resources, connections, and line numbers
//! are all first-class here so nobody re-parses text downstream.
//!
//! # Tests (tests/scene.rs)
//! - `parses_header_ext_sub_nodes_connections_and_editable_paths`
//! - `node_properties_keep_typed_values` (ints, floats, strings, bools, arrays, dicts, `Vector2(..)`, `NodePath(..)`, `ExtResource(..)`, `SubResource(..)`)
//! - `every_entry_carries_its_line_number`
//! - `instance_and_instance_placeholder_are_distinguished`
//! - `groups_and_unique_names_are_read`
//! - `parent_of_root_is_none_and_paths_resolve_from_root_name`
//! - `malformed_sections_error_with_line_number`
//! - `tres_parses_with_the_same_grammar`
//! - (tests/scene_expand.rs) `expand_resolves_instances_and_inherited_bases`
//! - `expand_caps_depth_and_reports_where_it_stopped`
//! - `expand_rejects_instance_cycles`
//! - `expand_reports_missing_scene_files_as_unresolved_not_error`
//! - `compact_tree_renders_scripts_types_connections_and_groups`

use std::collections::BTreeMap;

use serde::Serialize;

use crate::respath::{NodePath, ResPath, Uid};

mod text;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SceneFile {
    pub kind: FileKind,
    pub format: u32,
    pub uid: Option<Uid>,
    pub load_steps: Option<u32>,
    pub ext_resources: Vec<ExtResource>,
    pub sub_resources: Vec<SubResource>,
    /// Document order. First node is the root (parent is `None`).
    pub nodes: Vec<SceneNode>,
    pub connections: Vec<Connection>,
    pub editable_instances: Vec<NodePath>,
    /// `[resource]` block for `.tres`.
    pub resource: Option<Properties>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    Scene,
    Resource,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ExtResource {
    pub id: String,
    pub type_name: String,
    pub path: ResPath,
    pub uid: Option<Uid>,
    pub line: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SubResource {
    pub id: String,
    pub type_name: String,
    pub properties: Properties,
    pub line: usize,
}

pub type Properties = BTreeMap<String, Value>;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SceneNode {
    pub name: String,
    pub parent: Option<NodePath>,
    pub type_name: Option<String>,
    pub instance: Option<ResourceRef>,
    pub instance_placeholder: Option<ResPath>,
    pub script: Option<ResourceRef>,
    pub groups: Vec<String>,
    pub unique_name_in_owner: bool,
    /// Every `key = value` line, including `script`, kept raw for consumers that want it.
    pub properties: Properties,
    pub line: usize,
}

impl SceneNode {
    /// Path relative to the scene root, as `get_node` from the root would take
    /// it: `.` for the root, `Child/Leaf` below it.
    pub fn path(&self) -> NodePath {
        match self.parent.as_ref().map(|parent| parent.0.as_str()) {
            None => NodePath(".".into()),
            Some(parent) => NodePath(parent.into()).join(&self.name),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Connection {
    pub signal: String,
    pub from: NodePath,
    pub to: NodePath,
    pub method: String,
    pub flags: Option<u32>,
    pub binds: Vec<Value>,
    /// Trailing signal arguments dropped before the call (`Callable.unbind`).
    pub unbinds: u32,
    pub line: usize,
}

/// Reference to an ext or sub resource by id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "id")]
pub enum ResourceRef {
    Ext(String),
    Sub(String),
}

/// A text-format value. Constructors like `Vector2(1, 2)` are kept symbolic.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "value")]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    StringName(String),
    Array(Vec<Value>),
    Dict(Vec<(Value, Value)>),
    /// `Vector2(1, 2)`, `NodePath("A/B")`, `ExtResource("1_abc")`, `PackedStringArray("a")`, ...
    Call { name: String, args: Vec<Value> },
}

impl Value {
    /// Contents of a `String` or `StringName`.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(text) | Value::StringName(text) => Some(text),
            _ => None,
        }
    }
    /// `ExtResource("id")` / `SubResource("id")`; format-2 integer ids are accepted.
    pub fn as_resource_ref(&self) -> Option<ResourceRef> {
        let Value::Call { name, args } = self else { return None };
        let id = match args.as_slice() {
            [Value::Str(id)] => id.clone(),
            [Value::Int(id)] => id.to_string(),
            _ => return None,
        };
        match name.as_str() {
            "ExtResource" => Some(ResourceRef::Ext(id)),
            "SubResource" => Some(ResourceRef::Sub(id)),
            _ => None,
        }
    }
    fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(value) => Some(*value),
            _ => None,
        }
    }
}

/// Parses a `.tscn` or `.tres` text.
pub fn parse(source: &str) -> crate::Result<SceneFile> {
    text::parse(source)
}

impl SceneFile {
    pub fn root(&self) -> Option<&SceneNode> {
        self.nodes.first()
    }
    pub fn ext_resource(&self, id: &str) -> Option<&ExtResource> {
        self.ext_resources.iter().find(|resource| resource.id == id)
    }
    pub fn sub_resource(&self, id: &str) -> Option<&SubResource> {
        self.sub_resources.iter().find(|resource| resource.id == id)
    }
    pub fn resolve(&self, reference: &ResourceRef) -> Option<Resolved<'_>> {
        match reference {
            ResourceRef::Ext(id) => self.ext_resource(id).map(Resolved::Ext),
            ResourceRef::Sub(id) => self.sub_resource(id).map(Resolved::Sub),
        }
    }
    /// `.` is the root; `./A` and `A` are the same node.
    pub fn node(&self, path: &NodePath) -> Option<&SceneNode> {
        let wanted = normalize(&path.0);
        self.nodes.iter().find(|node| normalize(&node.path().0) == wanted)
    }
    pub fn children_of(&self, path: &NodePath) -> impl Iterator<Item = &SceneNode> {
        let wanted = normalize(&path.0);
        self.nodes
            .iter()
            .filter(move |node| node.parent.as_ref().is_some_and(|parent| normalize(&parent.0) == wanted))
    }
    /// The `.tscn` an inherited scene extends: a root with `instance=` and no `type`.
    pub fn inherited_base(&self) -> Option<&ExtResource> {
        let root = self.root()?;
        match (&root.instance, &root.type_name) {
            (Some(reference @ ResourceRef::Ext(_)), None) => match self.resolve(reference)? {
                Resolved::Ext(resource) => Some(resource),
                Resolved::Sub(_) => None,
            },
            _ => None,
        }
    }
}

fn normalize(path: &str) -> &str {
    match path.trim_start_matches("./") {
        "" => ".",
        path => path,
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Resolved<'a> {
    Ext(&'a ExtResource),
    Sub(&'a SubResource),
}

/// Source of scenes for expansion. Implemented for `Project`; tests use a map.
pub trait SceneSource {
    fn load(&self, path: &ResPath) -> crate::Result<Option<SceneFile>>;
}

impl SceneSource for crate::Project {
    fn load(&self, path: &ResPath) -> crate::Result<Option<SceneFile>> {
        let os_path = self.globalize(path);
        if !os_path.is_file() {
            return Ok(None);
        }
        let source = self.read_to_string(path)?;
        parse(&source).map(Some).map_err(|error| error.with_path(os_path))
    }
}

#[derive(Clone, Debug)]
pub struct ExpandOptions {
    /// Instance depth cap. 1 means "this scene only".
    pub max_depth: usize,
}

impl Default for ExpandOptions {
    fn default() -> Self {
        Self { max_depth: 64 }
    }
}

/// A scene with instances and inherited bases folded in.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ExpandedScene {
    pub path: Option<ResPath>,
    pub nodes: Vec<ExpandedNode>,
    /// Instances that could not be loaded, with the reason. Not an error.
    pub unresolved: Vec<Unresolved>,
    /// True if `max_depth` stopped expansion somewhere.
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ExpandedNode {
    pub path: NodePath,
    pub name: String,
    pub type_name: Option<String>,
    pub script: Option<ResPath>,
    /// Scene file this node was declared in (differs from the top scene inside instances).
    pub origin: Option<ResPath>,
    pub instance_of: Option<ResPath>,
    pub groups: Vec<String>,
    pub connections: Vec<Connection>,
    pub depth: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Unresolved {
    pub at: NodePath,
    pub scene: ResPath,
    pub reason: String,
}

/// Expands `scene` using `source` for instances and `[gd_scene] base` inheritance.
/// Cycles are an `Error::Expansion`; missing files are `unresolved`.
pub fn expand(
    scene: &SceneFile,
    path: Option<&ResPath>,
    source: &dyn SceneSource,
    options: &ExpandOptions,
) -> crate::Result<ExpandedScene> {
    todo!()
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TreeOptions {
    pub connections: bool,
    pub groups: bool,
}

/// One line per node, indented by depth. Deterministic; the CLI prints it verbatim.
pub fn compact_tree(scene: &ExpandedScene, options: TreeOptions) -> String {
    todo!()
}
