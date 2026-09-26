//! `extension_api.json` → [`ApiIndex`]. Mirrors only the fields the index keeps;
//! unknown fields are ignored so a newer engine still parses.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::{
    API_INDEX_SCHEMA_VERSION, ApiArgument, ApiClass, ApiConstant, ApiEnum, ApiEnumValue, ApiIndex,
    ApiMethod, ApiOperator, ApiProperty, ApiSignal, ApiType, GdscriptBuiltins, Singleton,
};

#[derive(Deserialize)]
struct Dump {
    header: Header,
    #[serde(default)]
    global_constants: Vec<RawConstant>,
    #[serde(default)]
    global_enums: Vec<RawEnum>,
    #[serde(default)]
    utility_functions: Vec<RawMethod>,
    #[serde(default)]
    builtin_classes: Vec<RawBuiltin>,
    #[serde(default)]
    classes: Vec<RawClass>,
    #[serde(default)]
    singletons: Vec<RawSingleton>,
}

#[derive(Deserialize)]
struct Header {
    version_major: u32,
    version_minor: u32,
    version_patch: u32,
    #[serde(default)]
    version_status: String,
    #[serde(default)]
    version_full_name: String,
}

#[derive(Deserialize)]
struct RawConstant {
    name: String,
    #[serde(rename = "type")]
    type_: Option<String>,
    /// A number for classes and globals, GDScript source for builtin types.
    value: serde_json::Value,
    description: Option<String>,
}

#[derive(Deserialize)]
struct RawEnum {
    name: String,
    #[serde(default)]
    is_bitfield: bool,
    #[serde(default)]
    values: Vec<RawEnumValue>,
}

#[derive(Deserialize)]
struct RawEnumValue {
    name: String,
    value: i64,
    description: Option<String>,
}

#[derive(Deserialize)]
struct RawMethod {
    name: String,
    #[serde(default)]
    is_static: bool,
    #[serde(default)]
    is_const: bool,
    #[serde(default)]
    is_virtual: bool,
    #[serde(default)]
    is_required: bool,
    #[serde(default)]
    is_vararg: bool,
    /// Utility functions and builtin types.
    return_type: Option<String>,
    /// Classes.
    return_value: Option<RawReturn>,
    #[serde(default)]
    arguments: Vec<RawArgument>,
    description: Option<String>,
}

#[derive(Deserialize)]
struct RawReturn {
    #[serde(rename = "type")]
    type_: String,
}

#[derive(Deserialize)]
struct RawArgument {
    name: String,
    #[serde(rename = "type")]
    type_: String,
    default_value: Option<String>,
}

#[derive(Deserialize)]
struct RawConstructor {
    #[serde(default)]
    arguments: Vec<RawArgument>,
    description: Option<String>,
}

#[derive(Deserialize)]
struct RawOperator {
    name: String,
    right_type: Option<String>,
    return_type: String,
    description: Option<String>,
}

#[derive(Deserialize)]
struct RawMember {
    name: String,
    #[serde(rename = "type")]
    type_: String,
    description: Option<String>,
}

#[derive(Deserialize)]
struct RawBuiltin {
    name: String,
    brief_description: Option<String>,
    description: Option<String>,
    #[serde(default)]
    constructors: Vec<RawConstructor>,
    #[serde(default)]
    operators: Vec<RawOperator>,
    #[serde(default)]
    methods: Vec<RawMethod>,
    #[serde(default)]
    members: Vec<RawMember>,
    #[serde(default)]
    enums: Vec<RawEnum>,
    #[serde(default)]
    constants: Vec<RawConstant>,
}

#[derive(Deserialize)]
struct RawClass {
    name: String,
    inherits: Option<String>,
    #[serde(default)]
    is_refcounted: bool,
    #[serde(default)]
    is_instantiable: bool,
    #[serde(default)]
    api_type: String,
    brief_description: Option<String>,
    description: Option<String>,
    #[serde(default)]
    methods: Vec<RawMethod>,
    #[serde(default)]
    properties: Vec<RawProperty>,
    #[serde(default)]
    signals: Vec<RawSignal>,
    #[serde(default)]
    enums: Vec<RawEnum>,
    #[serde(default)]
    constants: Vec<RawConstant>,
}

#[derive(Deserialize)]
struct RawProperty {
    name: String,
    #[serde(rename = "type")]
    type_: String,
    getter: Option<String>,
    setter: Option<String>,
    description: Option<String>,
}

#[derive(Deserialize)]
struct RawSignal {
    name: String,
    #[serde(default)]
    arguments: Vec<RawArgument>,
    description: Option<String>,
}

#[derive(Deserialize)]
struct RawSingleton {
    name: String,
    #[serde(rename = "type")]
    type_: String,
}

pub(super) fn parse(text: &str) -> crate::Result<ApiIndex> {
    let dump: Dump = serde_json::from_str(text).map_err(|error| crate::Error::Parse {
        path: None,
        line: error.line(),
        message: format!("extension_api.json: {error}"),
    })?;
    let header = &dump.header;
    let engine_version = header
        .version_full_name
        .strip_prefix("Godot Engine v")
        .map(str::to_owned)
        .unwrap_or_else(|| {
            format!(
                "{}.{}.{}.{}",
                header.version_major,
                header.version_minor,
                header.version_patch,
                header.version_status
            )
        });
    let has_docs = dump
        .classes
        .iter()
        .any(|class| text_of(&class.brief_description).is_some());

    let mut utility_functions: Vec<ApiMethod> =
        dump.utility_functions.into_iter().map(method).collect();
    utility_functions.sort_by(|a, b| a.name.cmp(&b.name));
    let builtin_classes = dump
        .builtin_classes
        .into_iter()
        .map(|raw| (raw.name.clone(), builtin(raw)))
        .collect::<BTreeMap<_, _>>();
    let classes = dump
        .classes
        .into_iter()
        .map(|raw| (raw.name.clone(), class(raw)))
        .collect::<BTreeMap<_, _>>();
    Ok(ApiIndex {
        schema_version: API_INDEX_SCHEMA_VERSION,
        engine_version,
        has_docs,
        classes,
        builtin_classes,
        utility_functions,
        global_enums: dump.global_enums.into_iter().map(enum_).collect(),
        global_constants: dump.global_constants.into_iter().map(constant).collect(),
        singletons: dump
            .singletons
            .into_iter()
            .map(|raw| Singleton {
                name: raw.name,
                class: raw.type_,
            })
            .collect(),
        gdscript: GdscriptBuiltins::default(),
        extension_classes: Vec::new(),
    })
}

/// Empty descriptions are absent ones.
fn text_of(text: &Option<String>) -> Option<String> {
    text.as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn method(raw: RawMethod) -> ApiMethod {
    let return_type = match (&raw.return_value, &raw.return_type) {
        (Some(value), _) => ApiType::parse(&value.type_),
        (None, Some(spelling)) => ApiType::parse(spelling),
        (None, None) => ApiType::Void,
    };
    ApiMethod {
        description: text_of(&raw.description),
        name: raw.name,
        is_static: raw.is_static,
        is_const: raw.is_const,
        is_virtual: raw.is_virtual,
        is_required: raw.is_required,
        is_vararg: raw.is_vararg,
        return_type,
        arguments: raw.arguments.into_iter().map(argument).collect(),
        ..ApiMethod::default()
    }
}

fn argument(raw: RawArgument) -> ApiArgument {
    ApiArgument {
        type_: ApiType::parse(&raw.type_),
        name: raw.name,
        default: raw.default_value,
    }
}

fn enum_(raw: RawEnum) -> ApiEnum {
    ApiEnum {
        name: raw.name,
        is_bitfield: raw.is_bitfield,
        values: raw
            .values
            .into_iter()
            .map(|value| ApiEnumValue {
                description: text_of(&value.description),
                name: value.name,
                value: value.value,
                ..ApiEnumValue::default()
            })
            .collect(),
        ..ApiEnum::default()
    }
}

fn constant(raw: RawConstant) -> ApiConstant {
    ApiConstant {
        description: text_of(&raw.description),
        type_: raw.type_.as_deref().map(ApiType::parse),
        value: match raw.value {
            serde_json::Value::String(text) => text,
            other => other.to_string(),
        },
        name: raw.name,
        ..ApiConstant::default()
    }
}

fn builtin(raw: RawBuiltin) -> ApiClass {
    let constructors = raw
        .constructors
        .into_iter()
        .map(|constructor| ApiMethod {
            name: raw.name.clone(),
            is_static: false,
            is_const: false,
            is_virtual: false,
            is_required: false,
            is_vararg: false,
            return_type: ApiType::parse(&raw.name),
            arguments: constructor.arguments.into_iter().map(argument).collect(),
            description: text_of(&constructor.description),
            ..ApiMethod::default()
        })
        .collect();
    ApiClass {
        brief: text_of(&raw.brief_description),
        description: text_of(&raw.description),
        parent: None,
        is_builtin: true,
        is_refcounted: false,
        instantiable: true,
        api_type: "builtin".into(),
        constructors,
        operators: raw
            .operators
            .into_iter()
            .map(|operator| ApiOperator {
                description: text_of(&operator.description),
                right_type: operator.right_type.as_deref().map(ApiType::parse),
                return_type: ApiType::parse(&operator.return_type),
                operator: operator.name,
            })
            .collect(),
        methods: raw.methods.into_iter().map(method).collect(),
        properties: raw
            .members
            .into_iter()
            .map(|member| ApiProperty {
                description: text_of(&member.description),
                type_: ApiType::parse(&member.type_),
                name: member.name,
                getter: None,
                setter: None,
                default: None,
                ..ApiProperty::default()
            })
            .collect(),
        signals: Vec::new(),
        enums: raw.enums.into_iter().map(enum_).collect(),
        constants: raw.constants.into_iter().map(constant).collect(),
        name: raw.name,
        ..ApiClass::default()
    }
}

fn class(raw: RawClass) -> ApiClass {
    ApiClass {
        brief: text_of(&raw.brief_description),
        description: text_of(&raw.description),
        name: raw.name,
        parent: raw.inherits,
        is_builtin: false,
        is_refcounted: raw.is_refcounted,
        instantiable: raw.is_instantiable,
        api_type: raw.api_type,
        constructors: Vec::new(),
        operators: Vec::new(),
        methods: raw.methods.into_iter().map(method).collect(),
        properties: raw
            .properties
            .into_iter()
            .map(|property| ApiProperty {
                description: text_of(&property.description),
                type_: ApiType::parse(&property.type_),
                name: property.name,
                getter: property.getter.filter(|name| !name.is_empty()),
                setter: property.setter.filter(|name| !name.is_empty()),
                default: None,
                ..ApiProperty::default()
            })
            .collect(),
        signals: raw
            .signals
            .into_iter()
            .map(|signal| ApiSignal {
                description: text_of(&signal.description),
                name: signal.name,
                arguments: signal.arguments.into_iter().map(argument).collect(),
                ..ApiSignal::default()
            })
            .collect(),
        enums: raw.enums.into_iter().map(enum_).collect(),
        constants: raw.constants.into_iter().map(constant).collect(),
        ..ApiClass::default()
    }
}
