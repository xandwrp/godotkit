//! The engine API as a model, built from what the configured editor reports
//! about itself. gdview owns the shape, the parsers, and the lookups; gdproject
//! runs the engine and hands the text over. This module never talks to Godot.
//!
//! Sources, both from the engine binary:
//! - `--dump-extension-api-with-docs` ([`ApiIndex::from_extension_api_json`]):
//!   classes, builtin Variant types (`String.split`, `Vector3(x, y, z)`), utility
//!   functions (`lerp`), global enums and constants, singletons, and descriptions.
//! - `--doctool` class XML ([`doc_xml`], merged by [`ApiIndex::merge_doctool`]):
//!   `@GDScript` (`range`, `preload`, `@export`) and property defaults, which the
//!   dump lacks. The engine writes no descriptions there, so `@GDScript` entries
//!   stay undocumented. The same XML parser reads `--gdscript-docs` output for
//!   project scripts.
//!
//! Descriptions are kept as the engine's BBCode; [`bbcode::doc_text`] renders them.
//!
//! # Tests (tests/api.rs)
//! - `parses_a_real_extension_api_json_fixture` (fixture frozen from 4.7.2 by `real_engine_refresh_api_fixtures`)
//! - `type_spellings_parse_and_display_as_gdscript`
//! - `lineage_walks_parent_chain_and_stops_at_missing_parent_or_cycle`
//! - `lookup_member_finds_inherited_members_and_reports_declaring_class`
//! - `lookup_covers_builtin_classes_utility_functions_and_global_enums`
//! - `doctool_merge_adds_gdscript_builtins_and_property_defaults`
//! - `search_matches_class_and_member_names_case_insensitively_and_ranks_prefix_first`
//! - `format_signature_renders_static_vararg_defaults_and_return_type`
//! - `suggest_names_returns_close_matches_within_edit_distance`
//! - `descriptions_are_present_when_the_dump_had_docs_and_absent_otherwise`
//! - `script_classes_layer_over_native_ones_and_keys_normalize`
//! - `index_round_trips_through_json`
//!
//! Engine (`#[ignore]`, `GDKIT_TEST_GODOT`): `real_engine_refresh_api_fixtures`.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::variant::VariantType;

pub mod answer;
pub mod bbcode;
pub mod doc_xml;
mod dump;

/// Schema of gdview's normalized index (not the raw dump's `version` block).
pub const API_INDEX_SCHEMA_VERSION: u32 = 5;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ApiIndex {
    pub schema_version: u32,
    /// `4.7.2.stable.arch_linux`, from the dump's header.
    pub engine_version: String,
    /// Whether descriptions were available (`--dump-extension-api-with-docs`).
    pub has_docs: bool,
    pub classes: BTreeMap<String, ApiClass>,
    pub builtin_classes: BTreeMap<String, ApiClass>,
    pub utility_functions: Vec<ApiMethod>,
    pub global_enums: Vec<ApiEnum>,
    pub global_constants: Vec<ApiConstant>,
    pub singletons: Vec<Singleton>,
    /// `@GDScript`, from `--doctool`. Empty until [`ApiIndex::merge_doctool`].
    pub gdscript: GdscriptBuiltins,
    /// Project GDExtension classes, when the producer could obtain them.
    pub extension_classes: Vec<String>,
}

/// GDScript's own functions, annotations, and constants. Signatures only.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GdscriptBuiltins {
    pub functions: Vec<ApiMethod>,
    pub annotations: Vec<ApiMethod>,
    pub constants: Vec<ApiConstant>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Singleton {
    pub name: String,
    pub class: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ApiClass {
    pub name: String,
    pub parent: Option<String>,
    pub is_builtin: bool,
    pub is_refcounted: bool,
    pub instantiable: bool,
    /// `core`, `editor`, `extension`, `editor_extension`; `builtin` for Variant types.
    pub api_type: String,
    pub brief: Option<String>,
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experimental: Option<String>,
    /// Where a project script class comes from; `None` for engine classes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<ScriptOrigin>,
    /// Builtin types only; each is named after the type.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constructors: Vec<ApiMethod>,
    /// Builtin types only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operators: Vec<ApiOperator>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub methods: Vec<ApiMethod>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub properties: Vec<ApiProperty>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signals: Vec<ApiSignal>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enums: Vec<ApiEnum>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constants: Vec<ApiConstant>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ApiMethod {
    pub name: String,
    pub is_static: bool,
    pub is_const: bool,
    pub is_virtual: bool,
    /// A virtual method that implementations must override.
    pub is_required: bool,
    pub is_vararg: bool,
    pub return_type: ApiType,
    pub arguments: Vec<ApiArgument>,
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experimental: Option<String>,
    /// Declaration line, for project scripts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ApiArgument {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: ApiType,
    /// Default as GDScript source, as the engine spells it (`0` for enums).
    pub default: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ApiProperty {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: ApiType,
    pub getter: Option<String>,
    pub setter: Option<String>,
    /// From `--doctool`; the dump has no defaults.
    pub default: Option<String>,
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experimental: Option<String>,
    /// Declaration line, for project scripts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ApiSignal {
    pub name: String,
    pub arguments: Vec<ApiArgument>,
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experimental: Option<String>,
    /// Declaration line, for project scripts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ApiEnum {
    pub name: String,
    pub is_bitfield: bool,
    pub values: Vec<ApiEnumValue>,
    /// Declaration line, for project scripts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ApiEnumValue {
    pub name: String,
    pub value: i64,
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experimental: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ApiConstant {
    pub name: String,
    /// Builtin-type constants only (`Vector3.ZERO: Vector3`); others are `int`.
    #[serde(rename = "type")]
    pub type_: Option<ApiType>,
    /// GDScript source: `10`, `Vector3(0, 0, 0)`.
    pub value: String,
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experimental: Option<String>,
    /// Declaration line, for project scripts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ApiOperator {
    /// `==`, `+`, `unary-`, `in`.
    pub operator: String,
    /// `None` for unary operators.
    pub right_type: Option<ApiType>,
    pub return_type: ApiType,
    pub description: Option<String>,
}

/// A project script class's file, and whether the engine documented it
/// (`--gdscript-docs`) or gdkit read it from source because the engine could not.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScriptOrigin {
    /// `res://player.gd`
    pub path: String,
    /// Line of `class_name` (or of `class` for an inner class).
    pub line: Option<usize>,
    pub from_engine: bool,
}

/// A type as the engine spells it, normalized.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ApiType {
    #[default]
    Variant,
    Void,
    Builtin {
        name: VariantType,
    },
    Class {
        name: String,
    },
    /// `Node.ProcessMode`, or a global enum (`Key`, `Variant.Type`).
    Enum {
        name: String,
        bitfield: bool,
    },
    TypedArray {
        element: Box<ApiType>,
    },
    TypedDictionary {
        key: Box<ApiType>,
        value: Box<ApiType>,
    },
    /// A resource property hint: any of `classes` except `except`
    /// (`Texture2D,-AnimatedTexture`).
    OneOf {
        classes: Vec<String>,
        except: Vec<String>,
    },
    /// Pointers and native structures (`const void*`, `AudioFrame*`); GDExtension-only.
    Native {
        spelling: String,
    },
}

impl ApiType {
    /// Parses the dump's spelling: `int`, `Node`, `enum::Node.ProcessMode`,
    /// `bitfield::…`, `typedarray::Node`, `typedarray::24/17:Font`,
    /// `typeddictionary::int;String`, `BaseMaterial3D,ShaderMaterial`, `void*`.
    pub fn parse(spelling: &str) -> ApiType {
        let spelling = spelling.trim();
        if let Some(name) = spelling.strip_prefix("enum::") {
            return ApiType::Enum {
                name: name.to_owned(),
                bitfield: false,
            };
        }
        if let Some(name) = spelling.strip_prefix("bitfield::") {
            return ApiType::Enum {
                name: name.to_owned(),
                bitfield: true,
            };
        }
        if let Some(element) = spelling.strip_prefix("typedarray::") {
            return ApiType::TypedArray {
                element: Box::new(Self::parse_hinted(element)),
            };
        }
        if let Some(pair) = spelling.strip_prefix("typeddictionary::")
            && let Some((key, value)) = pair.split_once(';')
        {
            return ApiType::TypedDictionary {
                key: Box::new(Self::parse_hinted(key)),
                value: Box::new(Self::parse_hinted(value)),
            };
        }
        if spelling.contains('*') {
            return ApiType::Native {
                spelling: spelling.to_owned(),
            };
        }
        if spelling.contains(',') {
            let (except, classes): (Vec<&str>, Vec<&str>) = spelling
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .partition(|name| name.starts_with('-'));
            return ApiType::OneOf {
                classes: classes.into_iter().map(str::to_owned).collect(),
                except: except
                    .into_iter()
                    .map(|name| name.trim_start_matches('-').to_owned())
                    .collect(),
            };
        }
        match spelling {
            "" | "void" => ApiType::Void,
            "Variant" | "Nil" => ApiType::Variant,
            // `Object` is a class with members, not just a Variant slot.
            "Object" => ApiType::Class {
                name: "Object".into(),
            },
            _ => match VariantType::from_name(spelling) {
                Some(name) => ApiType::Builtin { name },
                None => ApiType::Class {
                    name: spelling.to_owned(),
                },
            },
        }
    }

    /// Element spellings may carry a property hint: `24/17:Font` (a class), `27/0:` (a Variant type code).
    fn parse_hinted(spelling: &str) -> ApiType {
        let Some((hint, rest)) = spelling.split_once(':') else {
            return Self::parse(spelling);
        };
        let Some((code, _)) = hint.split_once('/') else {
            return Self::parse(spelling);
        };
        if !rest.is_empty() {
            return Self::parse(rest);
        }
        match code.parse().ok().and_then(VariantType::from_code) {
            Some(VariantType::Nil) | None => ApiType::Variant,
            Some(ty) => Self::parse(ty.name()),
        }
    }

    /// Parses a class-reference XML spelling: `int` with `enum="Node.ProcessMode"`,
    /// `Node[]`, `Array[Node]`, `Dictionary[int, String]`.
    pub fn parse_doc(spelling: &str, enum_name: Option<&str>, bitfield: bool) -> ApiType {
        if let Some(name) = enum_name.filter(|name| !name.is_empty()) {
            // An array of enums is spelled `Node.ProcessMode[]`; keep the enum.
            return match name.strip_suffix("[]") {
                Some(name) => ApiType::TypedArray {
                    element: Box::new(ApiType::Enum {
                        name: name.to_owned(),
                        bitfield,
                    }),
                },
                None => ApiType::Enum {
                    name: name.to_owned(),
                    bitfield,
                },
            };
        }
        let spelling = spelling.trim();
        if let Some(element) = spelling.strip_suffix("[]") {
            return ApiType::TypedArray {
                element: Box::new(Self::parse_doc(element, None, false)),
            };
        }
        if let Some(element) = spelling
            .strip_prefix("Array[")
            .and_then(|rest| rest.strip_suffix(']'))
        {
            return ApiType::TypedArray {
                element: Box::new(Self::parse_doc(element, None, false)),
            };
        }
        if let Some((key, value)) = spelling
            .strip_prefix("Dictionary[")
            .and_then(|rest| rest.strip_suffix(']'))
            .and_then(split_top_level_comma)
        {
            return ApiType::TypedDictionary {
                key: Box::new(Self::parse_doc(key, None, false)),
                value: Box::new(Self::parse_doc(value, None, false)),
            };
        }
        Self::parse(spelling)
    }

    /// As GDScript writes it: `Array[Node]`, `Dictionary[int, String]`, `Node.ProcessMode`.
    pub fn display(&self) -> String {
        match self {
            ApiType::Variant => "Variant".into(),
            ApiType::Void => "void".into(),
            ApiType::Builtin { name } => name.name().into(),
            ApiType::Class { name } | ApiType::Enum { name, .. } => name.clone(),
            ApiType::TypedArray { element } => format!("Array[{}]", element.display()),
            ApiType::TypedDictionary { key, value } => {
                format!("Dictionary[{}, {}]", key.display(), value.display())
            }
            ApiType::OneOf { classes, except } => {
                let mut text = classes.join(" | ");
                if !except.is_empty() {
                    text.push_str(&format!(" (except {})", except.join(", ")));
                }
                text
            }
            ApiType::Native { spelling } => spelling.clone(),
        }
    }
}

fn split_top_level_comma(text: &str) -> Option<(&str, &str)> {
    let mut depth = 0usize;
    for (at, ch) in text.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => return Some((text[..at].trim(), text[at + 1..].trim())),
            _ => {}
        }
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberKind {
    Constructor,
    Method,
    Property,
    Signal,
    Enum,
    EnumValue,
    Constant,
    Operator,
}

/// One declared member of a class.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Member<'a> {
    Method(&'a ApiMethod),
    Property(&'a ApiProperty),
    Signal(&'a ApiSignal),
    Enum(&'a ApiEnum),
    EnumValue {
        owner: &'a ApiEnum,
        value: &'a ApiEnumValue,
    },
    Constant(&'a ApiConstant),
}

impl<'a> Member<'a> {
    pub fn kind(&self) -> MemberKind {
        match self {
            Member::Method(_) => MemberKind::Method,
            Member::Property(_) => MemberKind::Property,
            Member::Signal(_) => MemberKind::Signal,
            Member::Enum(_) => MemberKind::Enum,
            Member::EnumValue { .. } => MemberKind::EnumValue,
            Member::Constant(_) => MemberKind::Constant,
        }
    }

    pub fn name(&self) -> &'a str {
        match self {
            Member::Method(method) => &method.name,
            Member::Property(property) => &property.name,
            Member::Signal(signal) => &signal.name,
            Member::Enum(enum_) => &enum_.name,
            Member::EnumValue { value, .. } => &value.name,
            Member::Constant(constant) => &constant.name,
        }
    }

    pub fn description(&self) -> Option<&'a str> {
        match self {
            Member::Method(method) => method.description.as_deref(),
            Member::Property(property) => property.description.as_deref(),
            Member::Signal(signal) => signal.description.as_deref(),
            Member::Enum(_) => None,
            Member::EnumValue { value, .. } => value.description.as_deref(),
            Member::Constant(constant) => constant.description.as_deref(),
        }
    }
}

impl ApiClass {
    /// Every declared member, in lookup priority: methods, properties, signals,
    /// constants, enums, enum values.
    pub fn members(&self) -> impl Iterator<Item = Member<'_>> {
        let methods = self.methods.iter().map(Member::Method);
        let properties = self.properties.iter().map(Member::Property);
        let signals = self.signals.iter().map(Member::Signal);
        let constants = self.constants.iter().map(Member::Constant);
        let enums = self.enums.iter().map(Member::Enum);
        let values = self.enums.iter().flat_map(|owner| {
            owner
                .values
                .iter()
                .map(move |value| Member::EnumValue { owner, value })
        });
        methods
            .chain(properties)
            .chain(signals)
            .chain(constants)
            .chain(enums)
            .chain(values)
    }

    /// A member declared on this class itself.
    pub fn member(&self, name: &str) -> Option<Member<'_>> {
        self.members().find(|member| member.name() == name)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MemberHit<'a> {
    pub declaring_class: &'a ApiClass,
    pub member: Member<'a>,
}

/// A name that needs no class: `lerp`, `range`, `@export`, `Key`, `KEY_A`, `PI`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Global<'a> {
    Utility(&'a ApiMethod),
    GdscriptFunction(&'a ApiMethod),
    Annotation(&'a ApiMethod),
    Enum(&'a ApiEnum),
    EnumValue {
        owner: &'a ApiEnum,
        value: &'a ApiEnumValue,
    },
    Constant(&'a ApiConstant),
    GdscriptConstant(&'a ApiConstant),
}

impl<'a> Global<'a> {
    pub fn name(&self) -> &'a str {
        match self {
            Global::Utility(method)
            | Global::GdscriptFunction(method)
            | Global::Annotation(method) => &method.name,
            Global::Enum(enum_) => &enum_.name,
            Global::EnumValue { value, .. } => &value.name,
            Global::Constant(constant) | Global::GdscriptConstant(constant) => &constant.name,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SearchHit<'a> {
    Class(&'a ApiClass),
    Member {
        class: &'a ApiClass,
        member: Member<'a>,
    },
    Global(Global<'a>),
}

impl<'a> SearchHit<'a> {
    pub fn name(&self) -> &'a str {
        match self {
            SearchHit::Class(class) => &class.name,
            SearchHit::Member { member, .. } => member.name(),
            SearchHit::Global(global) => global.name(),
        }
    }

    /// The declaring class, for members.
    pub fn owner(&self) -> Option<&'a str> {
        match self {
            SearchHit::Member { class, .. } => Some(&class.name),
            _ => None,
        }
    }
}

impl ApiIndex {
    /// Parses the raw `extension_api.json` text into the normalized index.
    pub fn from_extension_api_json(text: &str) -> crate::Result<Self> {
        dump::parse(text)
    }

    /// Adds what only `--doctool` has: `@GDScript`, property defaults, and the
    /// enum behind `int` properties. Descriptions are left alone; doctool writes none.
    pub fn merge_doctool(&mut self, classes: Vec<doc_xml::DocClass>) {
        for doc in classes {
            match doc.class.name.as_str() {
                "@GDScript" => {
                    self.gdscript = GdscriptBuiltins {
                        functions: doc.class.methods,
                        annotations: doc.annotations,
                        constants: doc.class.constants,
                    };
                }
                "@GlobalScope" => {}
                name => {
                    let Some(class) = self
                        .classes
                        .get_mut(name)
                        .or_else(|| self.builtin_classes.get_mut(name))
                    else {
                        continue;
                    };
                    for documented in doc.class.properties {
                        if let Some(property) = class
                            .properties
                            .iter_mut()
                            .find(|property| property.name == documented.name)
                        {
                            property.default = documented.default.or(property.default.take());
                            // The dump types enum properties as plain `int`; the XML names the enum.
                            if matches!(documented.type_, ApiType::Enum { .. })
                                && property.type_
                                    == (ApiType::Builtin {
                                        name: VariantType::Int,
                                    })
                            {
                                property.type_ = documented.type_;
                            }
                        }
                    }
                }
            }
        }
    }

    /// Adds project script classes (`api_type: "script"`) so lookups, lineage,
    /// and search cover them. A script class named like an engine class is not
    /// added (Godot refuses such a `class_name`); the names are returned.
    pub fn add_scripts(&mut self, scripts: Vec<ApiClass>) -> Vec<String> {
        let mut shadowed = Vec::new();
        for mut class in scripts {
            if self
                .classes
                .get(&class.name)
                .is_some_and(|c| c.script.is_none())
                || self.builtin_classes.contains_key(&class.name)
            {
                shadowed.push(class.name);
                continue;
            }
            class.api_type = "script".into();
            self.classes.insert(class.name.clone(), class);
        }
        shadowed
    }

    /// Native or builtin class.
    pub fn class(&self, name: &str) -> Option<&ApiClass> {
        self.classes
            .get(name)
            .or_else(|| self.builtin_classes.get(name))
    }

    /// `[Self, Parent, Grandparent, …]`. Stops silently at an unknown parent or a cycle.
    pub fn lineage(&self, name: &str) -> Vec<&ApiClass> {
        let mut lineage: Vec<&ApiClass> = Vec::new();
        let mut next = self.class(name);
        while let Some(class) = next {
            if lineage.iter().any(|seen| seen.name == class.name) {
                break;
            }
            lineage.push(class);
            next = class
                .parent
                .as_deref()
                .and_then(|parent| self.class(parent));
        }
        lineage
    }

    /// Finds a member on the class or its nearest ancestor that declares it.
    pub fn lookup_member(&self, class: &str, member: &str) -> Option<MemberHit<'_>> {
        self.lineage(class).into_iter().find_map(|declaring_class| {
            declaring_class.member(member).map(|member| MemberHit {
                declaring_class,
                member,
            })
        })
    }

    pub fn utility_function(&self, name: &str) -> Option<&ApiMethod> {
        self.utility_functions.iter().find(|f| f.name == name)
    }

    pub fn global_enum(&self, name: &str) -> Option<&ApiEnum> {
        self.global_enums.iter().find(|e| e.name == name)
    }

    /// Everything reachable without a class, in lookup priority: utility
    /// functions, `@GDScript` functions and annotations, global enums, their
    /// values, global constants, `@GDScript` constants.
    pub fn globals(&self) -> impl Iterator<Item = Global<'_>> {
        let utilities = self.utility_functions.iter().map(Global::Utility);
        let functions = self.gdscript.functions.iter().map(Global::GdscriptFunction);
        let annotations = self.gdscript.annotations.iter().map(Global::Annotation);
        let enums = self.global_enums.iter().map(Global::Enum);
        let values = self.global_enums.iter().flat_map(|owner| {
            owner
                .values
                .iter()
                .map(move |value| Global::EnumValue { owner, value })
        });
        let constants = self.global_constants.iter().map(Global::Constant);
        let gdscript_constants = self.gdscript.constants.iter().map(Global::GdscriptConstant);
        utilities
            .chain(functions)
            .chain(annotations)
            .chain(enums)
            .chain(values)
            .chain(constants)
            .chain(gdscript_constants)
    }

    /// A global by exact name. Annotations match with or without the `@`.
    pub fn global(&self, name: &str) -> Option<Global<'_>> {
        self.globals()
            .find(|global| global.name() == name)
            .or_else(|| {
                let at = format!("@{name}");
                self.globals()
                    .find(|global| matches!(global, Global::Annotation(_)) && global.name() == at)
            })
    }

    /// An enum by the name a type spells it with: `Node.ProcessMode`, `Key`, `Variant.Type`.
    pub fn enum_named(&self, name: &str) -> Option<&ApiEnum> {
        self.global_enum(name).or_else(|| {
            let (class, enum_name) = name.rsplit_once('.')?;
            self.class(class)?
                .enums
                .iter()
                .find(|e| e.name == enum_name)
        })
    }

    /// Case-insensitive search over class, member, and global names, ignoring
    /// underscores and `@` (`process_mode` finds `ProcessMode`). Exact matches
    /// first (as typed, then normalized), then prefixes, then substrings;
    /// classes before globals before members.
    pub fn search(&self, term: &str, limit: usize) -> Vec<SearchHit<'_>> {
        let query = normalize(term);
        if query.is_empty() || limit == 0 {
            return Vec::new();
        }
        let classes = self.classes.values().chain(self.builtin_classes.values());
        let hits = classes
            .clone()
            .map(SearchHit::Class)
            .chain(self.globals().map(SearchHit::Global))
            .chain(classes.flat_map(|class| {
                class
                    .members()
                    .map(move |member| SearchHit::Member { class, member })
            }));
        let mut ranked = Vec::new();
        for hit in hits {
            let normalized = normalize(hit.name());
            let quality: u8 = if hit.name() == term {
                0
            } else if normalized == query {
                1
            } else if normalized.starts_with(&query) {
                2
            } else if normalized.contains(&query) {
                3
            } else {
                continue;
            };
            let kind: u8 = match hit {
                SearchHit::Class(_) => 0,
                SearchHit::Global(_) => 1,
                SearchHit::Member { .. } => 2,
            };
            let key = (
                quality,
                kind,
                normalized.len(),
                hit.name(),
                hit.owner().unwrap_or(""),
            );
            ranked.push((key, hit));
        }
        ranked.sort_by(|a, b| a.0.cmp(&b.0));
        ranked.into_iter().take(limit).map(|(_, hit)| hit).collect()
    }

    /// Close names for "did you mean". With a class, its members and its
    /// ancestors'; without, classes and globals. Edit-distance matches come
    /// first, then names containing every `_`-separated word of `name`
    /// (`move_slide` → `move_and_slide`).
    pub fn suggest(&self, class: Option<&str>, name: &str, limit: usize) -> Vec<String> {
        let candidates: BTreeSet<&str> = match class {
            Some(class) => self
                .lineage(class)
                .into_iter()
                .flat_map(ApiClass::members)
                .map(|member| member.name())
                .collect(),
            None => self
                .classes
                .keys()
                .chain(self.builtin_classes.keys())
                .map(String::as_str)
                .chain(self.globals().map(|global| global.name()))
                .collect(),
        };
        suggest_from(name, candidates, limit)
    }

    /// `static Owner.name(arg: Type = default, ...) -> Return const`. Enum
    /// defaults are shown by name when one value matches (`Node.INTERNAL_MODE_DISABLED`).
    pub fn signature(&self, owner: Option<&str>, method: &ApiMethod) -> String {
        let mut text = String::new();
        if method.is_static {
            text.push_str("static ");
        }
        if method.is_virtual {
            text.push_str(if method.is_required {
                "virtual required "
            } else {
                "virtual "
            });
        }
        if let Some(owner) = owner {
            text.push_str(owner);
            text.push('.');
        }
        text.push_str(&method.name);
        text.push('(');
        let mut parameters: Vec<String> = method
            .arguments
            .iter()
            .map(|argument| self.parameter(argument))
            .collect();
        if method.is_vararg {
            parameters.push("...".into());
        }
        text.push_str(&parameters.join(", "));
        text.push(')');
        // Annotations have no return value to speak of.
        if !method.name.starts_with('@') {
            text.push_str(" -> ");
            text.push_str(&method.return_type.display());
        }
        if method.is_const {
            text.push_str(" const");
        }
        text
    }

    /// `name: Type = default`
    pub fn parameter(&self, argument: &ApiArgument) -> String {
        let mut text = format!("{}: {}", argument.name, argument.type_.display());
        if let Some(default) = &argument.default {
            text.push_str(" = ");
            text.push_str(&self.default_display(&argument.type_, default));
        }
        text
    }

    /// A default as GDScript source, with enum integers named when one value matches.
    pub fn default_display(&self, type_: &ApiType, default: &str) -> String {
        let ApiType::Enum { name, .. } = type_ else {
            return default.to_owned();
        };
        let Ok(number) = default.trim().parse::<i64>() else {
            return default.to_owned();
        };
        let Some(enum_) = self.enum_named(name) else {
            return default.to_owned();
        };
        let mut matches = enum_.values.iter().filter(|value| value.value == number);
        match (matches.next(), matches.next()) {
            (Some(value), None) => match name.rsplit_once('.') {
                // Class enums are qualified by class; global ones (`Variant.Type` too) are bare.
                Some((class, _)) if self.global_enum(name).is_none() => {
                    format!("{class}.{}", value.name)
                }
                _ => value.name.clone(),
            },
            _ => default.to_owned(),
        }
    }

    /// The singleton instance name for a class (`Input` for `Input`).
    pub fn singleton_for(&self, class: &str) -> Option<&Singleton> {
        self.singletons.iter().find(|s| s.class == class)
    }
}

/// The name a project script class answers to: its `class_name`, else its
/// path. `--gdscript-docs` spells unnamed scripts `"dir/file.gd"` (inner
/// classes `"dir/file.gd".Inner`); those become `res://dir/file.gd(.Inner)`.
pub fn script_key(documented_name: &str) -> String {
    let Some(rest) = documented_name.strip_prefix('"') else {
        return documented_name.to_owned();
    };
    match rest.split_once('"') {
        Some((path, inner)) => {
            let path = path.strip_prefix("res://").unwrap_or(path);
            format!("res://{path}{inner}")
        }
        None => documented_name.to_owned(),
    }
}

/// [`crate::similar::similar`], then names containing every word of `name`.
pub fn suggest_from<'n>(
    name: &str,
    candidates: impl IntoIterator<Item = &'n str>,
    limit: usize,
) -> Vec<String> {
    let candidates: BTreeSet<&str> = candidates.into_iter().collect();
    let mut found = crate::similar::similar(name, candidates.iter().copied(), limit);
    if found.len() >= limit {
        return found;
    }
    let words: Vec<String> = name
        .to_lowercase()
        .split('_')
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect();
    if words.is_empty() {
        return found;
    }
    let query = name.to_lowercase();
    let mut containing: Vec<(usize, &str)> = candidates
        .iter()
        .copied()
        .filter(|candidate| *candidate != name && !found.iter().any(|f| f == candidate))
        .filter(|candidate| {
            let lower = candidate.to_lowercase();
            words.iter().all(|word| lower.contains(word.as_str()))
        })
        .map(|candidate| {
            (
                crate::similar::edit_distance(&query, &candidate.to_lowercase()),
                candidate,
            )
        })
        .collect();
    containing.sort_unstable();
    found.extend(
        containing
            .into_iter()
            .take(limit - found.len())
            .map(|(_, candidate)| candidate.to_owned()),
    );
    found
}

fn normalize(name: &str) -> String {
    name.chars()
        .filter(|ch| *ch != '_' && *ch != '@')
        .flat_map(char::to_lowercase)
        .collect()
}
