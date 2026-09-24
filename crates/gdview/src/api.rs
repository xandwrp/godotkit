//! The reflected native API as a model: classes, members, inheritance, search.
//! gdview owns the shape and the lookup logic; gdproject fills it from the
//! engine and caches it. This module never talks to Godot.
//!
//! # Tests (tests/api.rs)
//! - `lineage_walks_parent_chain_and_stops_at_missing_parent`
//! - `lookup_member_finds_inherited_members_and_reports_declaring_class`
//! - `search_matches_class_and_member_names_case_insensitively_and_ranks_prefix_first`
//! - `format_signature_renders_static_vararg_defaults_and_return_type`
//! - `suggest_names_returns_close_matches_within_edit_distance`
//! - `index_json_round_trips_and_carries_schema_version`

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const API_INDEX_SCHEMA_VERSION: u32 = 3;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApiIndex {
    pub schema_version: u32,
    pub engine_version: String,
    pub capabilities: Capabilities,
    pub classes: BTreeMap<String, ApiClass>,
}

/// Which reflection features the producing engine supported. Consumers must
/// not treat an absent member as "does not exist" when the capability is false.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub method_defaults: bool,
    pub property_hints: bool,
    pub extension_classes: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApiClass {
    pub name: String,
    pub parent: Option<String>,
    pub is_extension: bool,
    pub instantiable: bool,
    pub methods: Vec<ApiMethod>,
    pub properties: Vec<ApiProperty>,
    pub signals: Vec<ApiSignal>,
    pub enums: Vec<ApiEnum>,
    pub constants: Vec<ApiConstant>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApiMethod {
    pub name: String,
    pub is_static: bool,
    pub is_const: bool,
    pub is_virtual: bool,
    pub is_vararg: bool,
    pub return_type: ApiType,
    pub arguments: Vec<ApiArgument>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApiArgument {
    pub name: String,
    pub type_: ApiType,
    /// Rendered default as GDScript source, when known.
    pub default: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApiProperty {
    pub name: String,
    pub type_: ApiType,
    pub getter: Option<String>,
    pub setter: Option<String>,
    pub hint: Option<PropertyHint>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PropertyHint {
    pub kind: String,
    pub hint_string: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApiSignal {
    pub name: String,
    pub arguments: Vec<ApiArgument>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApiEnum {
    pub name: String,
    pub is_bitfield: bool,
    pub values: Vec<(String, i64)>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApiConstant {
    pub name: String,
    pub value: i64,
}

/// A type as reflected. `Variant` means untyped.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ApiType {
    Variant,
    Builtin { name: crate::variant::VariantType },
    Class { name: String },
    Enum { name: String },
    TypedArray { element: Box<ApiType> },
}

impl ApiType {
    pub fn display(&self) -> String {
        todo!()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberKind {
    Method,
    Property,
    Signal,
    Enum,
    Constant,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MemberHit<'a> {
    pub declaring_class: &'a ApiClass,
    pub kind: MemberKind,
    pub name: &'a str,
}

impl ApiIndex {
    pub fn class(&self, name: &str) -> Option<&ApiClass> {
        self.classes.get(name)
    }
    /// `[Self, Parent, Grandparent, …]`. Stops silently at an unknown parent.
    pub fn lineage(&self, name: &str) -> Vec<&ApiClass> {
        todo!()
    }
    /// Finds a member on the class or any ancestor.
    pub fn lookup_member(&self, class: &str, member: &str) -> Option<MemberHit<'_>> {
        todo!()
    }
    /// Case-insensitive search over class and member names, prefix matches first.
    pub fn search(&self, term: &str, limit: usize) -> Vec<SearchHit<'_>> {
        todo!()
    }
    /// Close names for "did you mean".
    pub fn suggest(&self, class: &str, member: &str, limit: usize) -> Vec<String> {
        todo!()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SearchHit<'a> {
    pub class: &'a ApiClass,
    pub member: Option<(MemberKind, &'a str)>,
}

pub fn format_signature(class: &ApiClass, method: &ApiMethod) -> String {
    todo!()
}
