//! The engine API as a model, mirroring what `godot --dump-extension-api-with-docs`
//! writes. gdview owns the shape and the lookups; gdproject produces the file and
//! parses it. This module never talks to Godot.
//!
//! Why the dump and not ClassDB reflection: the dump is official, complete, and
//! includes what ClassDB cannot reflect and agents get wrong most: builtin Variant
//! methods (`String.split`, `Array.filter`), utility functions (`lerp`, `clamp`),
//! global enums, singletons, and one-line descriptions.
//!
//! # Tests (tests/api.rs)
//! - `parses_a_real_extension_api_json_fixture` (fixture frozen from 4.7 by an engine-backed test)
//! - `lineage_walks_parent_chain_and_stops_at_missing_parent`
//! - `lookup_member_finds_inherited_members_and_reports_declaring_class`
//! - `lookup_covers_builtin_classes_utility_functions_and_global_enums`
//! - `search_matches_class_and_member_names_case_insensitively_and_ranks_prefix_first`
//! - `format_signature_renders_static_vararg_defaults_and_return_type`
//! - `suggest_names_returns_close_matches_within_edit_distance`
//! - `descriptions_are_present_when_the_dump_had_docs_and_absent_otherwise`

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Schema of gdview's normalized index (not the raw dump's `version` block).
pub const API_INDEX_SCHEMA_VERSION: u32 = 4;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApiIndex {
    pub schema_version: u32,
    pub engine_version: String,
    /// Whether descriptions were available (`--dump-extension-api-with-docs`).
    pub has_docs: bool,
    pub classes: BTreeMap<String, ApiClass>,
    pub builtin_classes: BTreeMap<String, ApiClass>,
    pub utility_functions: Vec<ApiMethod>,
    pub global_enums: Vec<ApiEnum>,
    pub singletons: Vec<Singleton>,
    /// Project GDExtension classes, when the producer could obtain them.
    pub extension_classes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Singleton {
    pub name: String,
    pub class: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApiClass {
    pub name: String,
    pub parent: Option<String>,
    pub is_builtin: bool,
    pub is_refcounted: bool,
    pub instantiable: bool,
    pub api_type: String,
    pub brief: Option<String>,
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
    pub brief: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApiArgument {
    pub name: String,
    pub type_: ApiType,
    /// Default as GDScript source, when present.
    pub default: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApiProperty {
    pub name: String,
    pub type_: ApiType,
    pub getter: Option<String>,
    pub setter: Option<String>,
    pub brief: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApiSignal {
    pub name: String,
    pub arguments: Vec<ApiArgument>,
    pub brief: Option<String>,
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
    pub value: String,
}

/// A type as the dump spells it (`int`, `Node`, `typedarray::Node`, `enum::Node.ProcessMode`, `bitfield::…`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ApiType {
    Variant,
    Void,
    Builtin { name: crate::variant::VariantType },
    Class { name: String },
    Enum { name: String, bitfield: bool },
    TypedArray { element: Box<ApiType> },
}

impl ApiType {
    pub fn parse(spelling: &str) -> ApiType {
        todo!()
    }
    pub fn display(&self) -> String {
        todo!()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
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
    /// Parses the raw `extension_api.json` text into the normalized index.
    pub fn from_extension_api_json(text: &str) -> crate::Result<Self> {
        todo!()
    }
    /// Native or builtin class.
    pub fn class(&self, name: &str) -> Option<&ApiClass> {
        self.classes
            .get(name)
            .or_else(|| self.builtin_classes.get(name))
    }
    /// `[Self, Parent, Grandparent, …]`. Stops silently at an unknown parent.
    pub fn lineage(&self, name: &str) -> Vec<&ApiClass> {
        todo!()
    }
    /// Finds a member on the class or any ancestor.
    pub fn lookup_member(&self, class: &str, member: &str) -> Option<MemberHit<'_>> {
        todo!()
    }
    pub fn utility_function(&self, name: &str) -> Option<&ApiMethod> {
        self.utility_functions.iter().find(|f| f.name == name)
    }
    pub fn global_enum(&self, name: &str) -> Option<&ApiEnum> {
        self.global_enums.iter().find(|e| e.name == name)
    }
    /// Case-insensitive search over class, member, utility, and enum names; prefix matches first.
    pub fn search(&self, term: &str, limit: usize) -> Vec<SearchHit<'_>> {
        todo!()
    }
    /// Close names for "did you mean". `class = None` suggests classes and utilities.
    pub fn suggest(&self, class: Option<&str>, name: &str, limit: usize) -> Vec<String> {
        todo!()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SearchHit<'a> {
    Class(&'a ApiClass),
    Member {
        class: &'a ApiClass,
        kind: MemberKind,
        name: &'a str,
    },
    Utility(&'a ApiMethod),
    GlobalEnum(&'a ApiEnum),
}

pub fn format_signature(class: Option<&ApiClass>, method: &ApiMethod) -> String {
    todo!()
}
