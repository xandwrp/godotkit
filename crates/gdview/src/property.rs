//! Godot property metadata, as `Object.get_property_list()` reports it, and the
//! field schema `resource schema` derives from it. The engine side only reports
//! raw entries; everything here is offline and pinned by fixtures.
//!
//! Hint strings (Godot 4.7.2):
//! - Typed arrays: hint `TYPE_STRING` (scripts) or `ARRAY_TYPE` (native classes),
//!   hint string a subtype `"<type>[/<hint>]:<hint_string>"`: `"4:"` is
//!   `Array[String]`, `"24/17:Texture2D"` is `Array[Texture2D]`,
//!   `"2/2:Melee:0,Ranged:5"` an array of a script enum, `"4/13:*.tres"` an
//!   array of file paths.
//! - Typed dictionaries: hint `TYPE_STRING` or `DICTIONARY_TYPE`, two subtypes
//!   joined by `;`: `"4:;2:"` is `Dictionary[String, int]`; type `0` is Variant.
//! - Enums: `"Melee:0,Ranged:5"` (explicit) or `"Linear,Constant,Cubic"`
//!   (implicit: previous value + 1, from 0). Flags: implicit values are
//!   `1 << index`. A `String` field's choices are reported verbatim, each
//!   option's full text as its value.
//! - An enum-typed field's `class_name` is the enum (`WeaponDefinition.Kind`),
//!   flagged by usage `CLASS_IS_ENUM`/`CLASS_IS_BITFIELD`; it is reported as
//!   `enum_name`, and `class_name` is only ever an Object's required class.
//!
//! Fields are the entries with usage `STORAGE` (what a `.tres` holds), minus
//! `script`, which the target sets. `INTERNAL` storage (e.g.
//! `ArrayMesh._surfaces`) is kept and marked `internal`.
//!
//! # Tests (tests/property.rs)
//! - `fields_keep_storage_entries_and_drop_script_groups_and_editor_only`
//! - `typed_arrays_parse_script_and_native_subtype_hint_strings`
//! - `typed_dictionaries_parse_key_and_value_subtypes_with_variant_sides`
//! - `enum_and_flag_choices_follow_godot_implicit_values`
//! - `enum_fields_report_enum_name_not_class_name`
//! - `hints_are_named_and_unknown_codes_kept`
//! - `accepts_documents_every_variant_type_and_untransportable_types_are_unsupported`
//! - `unknown_type_codes_and_malformed_subtypes_are_errors`

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::variant::VariantType;

/// `PropertyUsageFlags` bits this module reads.
pub mod usage {
    pub const STORAGE: u32 = 2;
    pub const INTERNAL: u32 = 8;
    pub const CLASS_IS_BITFIELD: u32 = 512;
    pub const CLASS_IS_ENUM: u32 = 65536;
}

/// `PropertyHint` values this module reads.
pub mod hint {
    pub const NONE: u32 = 0;
    pub const ENUM: u32 = 2;
    pub const ENUM_SUGGESTION: u32 = 3;
    pub const FLAGS: u32 = 6;
    pub const RESOURCE_TYPE: u32 = 17;
    pub const TYPE_STRING: u32 = 23;
    pub const ARRAY_TYPE: u32 = 31;
    pub const NODE_TYPE: u32 = 34;
    pub const DICTIONARY_TYPE: u32 = 38;
}

/// One `get_property_list()` entry plus the instance's current value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PropertyInfo {
    pub name: String,
    /// Godot's numeric `Variant.Type`.
    #[serde(rename = "type")]
    pub type_code: u32,
    #[serde(default)]
    pub class_name: String,
    #[serde(default)]
    pub hint: u32,
    #[serde(default)]
    pub hint_string: String,
    #[serde(default)]
    pub usage: u32,
    /// The value on a fresh instance, in the `gdview::variant` grammar; `null`
    /// when `default_error` says why it could not be encoded.
    #[serde(default)]
    pub default: Value,
    #[serde(default)]
    pub default_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EnumChoice {
    pub name: String,
    /// An integer for int fields; the option text for String fields.
    pub value: Value,
}

/// The declared type of a container element, key, or value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TypeSchema {
    /// `Nil` means any Variant.
    pub variant_type: VariantType,
    pub class_name: Option<String>,
    pub enum_choices: Vec<EnumChoice>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FieldSchema {
    pub name: String,
    /// `Nil` means any Variant.
    pub variant_type: VariantType,
    /// The class an Object field requires.
    pub class_name: Option<String>,
    /// The enum or bitfield an int field is declared as, e.g. `WeaponDefinition.Kind`.
    pub enum_name: Option<String>,
    /// Element type of a typed Array.
    pub element: Option<TypeSchema>,
    /// Key and value types of a typed Dictionary.
    pub key: Option<TypeSchema>,
    pub value: Option<TypeSchema>,
    /// The value on a fresh instance, encoded; `null` when it could not be.
    pub default: Value,
    /// `PropertyHint` name without its prefix, lowercase (`range`, `enum`, `file`).
    pub hint: Option<String>,
    pub hint_string: Option<String>,
    pub enum_choices: Vec<EnumChoice>,
    /// Engine-internal storage; settable, but not meant to be written by hand.
    pub internal: bool,
    /// The JSON shapes a spec may give, as documentation for generators.
    pub accepts: Vec<String>,
    /// Why no spec can set this field, when none can.
    pub unsupported: Option<String>,
}

/// The fields a resource spec can name, in property-list order.
pub fn fields(properties: &[PropertyInfo]) -> crate::Result<Vec<FieldSchema>> {
    let mut fields = Vec::new();
    for info in properties {
        if let Some(field) = field(info)? {
            fields.push(field);
        }
    }
    Ok(fields)
}

/// `None` for entries that are not stored or are the `script` property.
pub fn field(info: &PropertyInfo) -> crate::Result<Option<FieldSchema>> {
    if info.usage & usage::STORAGE == 0 || info.name == "script" {
        return Ok(None);
    }
    let variant_type = variant_type(info.type_code)?;
    let is_enum = info.usage & (usage::CLASS_IS_ENUM | usage::CLASS_IS_BITFIELD) != 0;
    let class_name = match variant_type {
        VariantType::Object if !info.class_name.is_empty() => Some(info.class_name.clone()),
        VariantType::Object
            if matches!(info.hint, hint::RESOURCE_TYPE | hint::NODE_TYPE)
                && !info.hint_string.is_empty() =>
        {
            Some(info.hint_string.clone())
        }
        _ => None,
    };
    let enum_name = (is_enum && !info.class_name.is_empty()).then(|| info.class_name.clone());
    let typed = matches!(
        info.hint,
        hint::TYPE_STRING | hint::ARRAY_TYPE | hint::DICTIONARY_TYPE
    ) && !info.hint_string.is_empty();
    let element = match variant_type {
        VariantType::Array if typed => Some(subtype(&info.hint_string)?),
        _ => None,
    };
    let (key, value) = match variant_type {
        VariantType::Dictionary if typed => {
            let (key, value) = info.hint_string.split_once(';').ok_or_else(|| {
                malformed(&info.hint_string, "a typed Dictionary needs `key;value`")
            })?;
            (Some(subtype(key)?), Some(subtype(value)?))
        }
        _ => (None, None),
    };
    let enum_choices = choices(variant_type, info.hint, &info.hint_string);
    let unsupported = unsupported(variant_type);
    let accepts = if unsupported.is_some() {
        Vec::new()
    } else {
        accepts(
            variant_type,
            info.hint,
            element.as_ref(),
            key.as_ref().zip(value.as_ref()),
        )
    };
    Ok(Some(FieldSchema {
        name: info.name.clone(),
        variant_type,
        class_name,
        enum_name,
        element,
        key,
        value,
        default: info.default.clone(),
        hint: hint_name(info.hint),
        hint_string: (!info.hint_string.is_empty()).then(|| info.hint_string.clone()),
        enum_choices,
        internal: info.usage & usage::INTERNAL != 0,
        accepts,
        unsupported,
    }))
}

fn variant_type(code: u32) -> crate::Result<VariantType> {
    VariantType::from_code(code)
        .ok_or_else(|| crate::Error::Variant(format!("unknown Variant type code {code}")))
}

fn malformed(text: &str, why: &str) -> crate::Error {
    crate::Error::Variant(format!("malformed type hint {text:?}: {why}"))
}

/// `"<type>[/<hint>]:<hint_string>"`, or a bare class or type name (older
/// native `ARRAY_TYPE` hints).
fn subtype(text: &str) -> crate::Result<TypeSchema> {
    let Some((head, rest)) = text.split_once(':') else {
        return Ok(match VariantType::from_name(text) {
            Some(variant_type) => TypeSchema {
                variant_type,
                class_name: None,
                enum_choices: Vec::new(),
            },
            None if !text.is_empty() => TypeSchema {
                variant_type: VariantType::Object,
                class_name: Some(text.to_owned()),
                enum_choices: Vec::new(),
            },
            None => return Err(malformed(text, "empty subtype")),
        });
    };
    let (code, sub_hint) = match head.split_once('/') {
        Some((code, sub_hint)) => (code, Some(sub_hint)),
        None => (head, None),
    };
    let code = code
        .parse()
        .map_err(|_| malformed(text, "the type is not a number"))?;
    let sub_hint = sub_hint
        .map(str::parse)
        .transpose()
        .map_err(|_| malformed(text, "the hint is not a number"))?
        .unwrap_or(hint::NONE);
    let variant_type = variant_type(code)?;
    let class_name = (variant_type == VariantType::Object
        && matches!(sub_hint, hint::RESOURCE_TYPE | hint::NODE_TYPE)
        && !rest.is_empty())
    .then(|| rest.to_owned());
    Ok(TypeSchema {
        variant_type,
        class_name,
        enum_choices: choices(variant_type, sub_hint, rest),
    })
}

fn choices(variant_type: VariantType, hint: u32, hint_string: &str) -> Vec<EnumChoice> {
    let flags = hint == hint::FLAGS;
    if !matches!(hint, hint::ENUM | hint::ENUM_SUGGESTION | hint::FLAGS) || hint_string.is_empty() {
        return Vec::new();
    }
    let options = hint_string
        .split(',')
        .map(str::trim)
        .filter(|option| !option.is_empty());
    if variant_type != VariantType::Int {
        return options
            .map(|option| EnumChoice {
                name: option.to_owned(),
                value: Value::String(option.to_owned()),
            })
            .collect();
    }
    let mut next = 0i64;
    options
        .enumerate()
        .map(|(index, option)| {
            let explicit = option
                .rsplit_once(':')
                .and_then(|(name, value)| Some((name.trim(), value.trim().parse::<i64>().ok()?)));
            let (name, value) = match explicit {
                Some((name, value)) => (name, value),
                None if flags => (option, 1i64.checked_shl(index as u32).unwrap_or(0)),
                None => (option, next),
            };
            next = value.wrapping_add(1);
            EnumChoice {
                name: name.to_owned(),
                value: Value::from(value),
            }
        })
        .collect()
}

fn hint_name(code: u32) -> Option<String> {
    let name = match code {
        0 => return None,
        1 => "range",
        2 => "enum",
        3 => "enum_suggestion",
        4 => "exp_easing",
        5 => "link",
        6 => "flags",
        7 => "layers_2d_render",
        8 => "layers_2d_physics",
        9 => "layers_2d_navigation",
        10 => "layers_3d_render",
        11 => "layers_3d_physics",
        12 => "layers_3d_navigation",
        13 => "file",
        14 => "dir",
        15 => "global_file",
        16 => "global_dir",
        17 => "resource_type",
        18 => "multiline_text",
        19 => "expression",
        20 => "placeholder_text",
        21 => "color_no_alpha",
        22 => "object_id",
        23 => "type_string",
        24 => "node_path_to_edited_node",
        25 => "object_too_big",
        26 => "node_path_valid_types",
        27 => "save_file",
        28 => "global_save_file",
        29 => "int_is_objectid",
        30 => "int_is_pointer",
        31 => "array_type",
        32 => "locale_id",
        33 => "localizable_string",
        34 => "node_type",
        35 => "hide_quaternion_edit",
        36 => "password",
        37 => "layers_avoidance",
        38 => "dictionary_type",
        39 => "tool_button",
        40 => "oneshot",
        42 => "group_enable",
        43 => "input_name",
        44 => "file_path",
        other => return Some(format!("hint_{other}")),
    };
    Some(name.to_owned())
}

fn unsupported(variant_type: VariantType) -> Option<String> {
    matches!(
        variant_type,
        VariantType::Rid | VariantType::Callable | VariantType::Signal
    )
    .then(|| {
        format!(
            "{} values exist only inside a running engine and cannot be written to a resource",
            variant_type.name()
        )
    })
}

fn tagged(variant_type: VariantType, payload: &str) -> String {
    format!(
        r#"{{"$variant":{{"type":"{}","value":{payload}}}}}"#,
        variant_type.name()
    )
}

fn accepts(
    variant_type: VariantType,
    hint: u32,
    element: Option<&TypeSchema>,
    entry: Option<(&TypeSchema, &TypeSchema)>,
) -> Vec<String> {
    use VariantType as T;
    match variant_type {
        T::Nil => vec!["any value".into()],
        T::Bool => vec!["true or false".into()],
        T::Int => vec![
            match hint {
                hint::ENUM | hint::ENUM_SUGGESTION => "integer: one of enum_choices' values",
                hint::FLAGS => "integer: a sum of enum_choices' values",
                _ => "integer",
            }
            .into(),
            format!("{} beyond ±(2^53-1)", tagged(T::Int, r#""<decimal>""#)),
        ],
        T::Float => vec![
            "number (integers are exact floats)".into(),
            tagged(T::Float, r#""nan"|"inf"|"-inf"|"-0.0""#),
        ],
        T::String if matches!(hint, hint::ENUM | hint::ENUM_SUGGESTION) => {
            vec!["string: one of enum_choices' values".into()]
        }
        T::String => vec!["string".into()],
        T::StringName | T::NodePath => vec!["string".into(), tagged(variant_type, "string")],
        T::Object => vec![
            "null".into(),
            r#"{"$ref":"res://…"}"#.into(),
            r#"{"$resource":{"class":"<Class>"|"script":"res://…","properties":{…}}}"#.into(),
        ],
        T::Array => vec![match element {
            Some(_) => "[item, …] with every item as `element` accepts".into(),
            None => "[value, …]".into(),
        }],
        T::Dictionary => match entry {
            Some(_) => vec![
                "{\"<key>\": value, …} when `key` is String or StringName".into(),
                format!(
                    "{} with keys as `key` and values as `value` accept",
                    tagged(T::Dictionary, "[[key, value], …]")
                ),
            ],
            None => vec![
                "{\"<string key>\": value, …}".into(),
                tagged(T::Dictionary, "[[key, value], …]"),
            ],
        },
        T::PackedByteArray => vec![tagged(variant_type, "[integers 0..=255]")],
        T::PackedInt32Array | T::PackedInt64Array => vec![tagged(variant_type, "[integers]")],
        T::PackedFloat32Array | T::PackedFloat64Array => vec![tagged(variant_type, "[numbers]")],
        T::PackedStringArray => vec![tagged(variant_type, "[strings]")],
        T::PackedVector2Array => vec![tagged(variant_type, "[tagged Vector2 values]")],
        T::PackedVector3Array => vec![tagged(variant_type, "[tagged Vector3 values]")],
        T::PackedVector4Array => vec![tagged(variant_type, "[tagged Vector4 values]")],
        T::PackedColorArray => vec![tagged(variant_type, "[tagged Color values]")],
        other => match other.components() {
            Some((count, integer)) => vec![tagged(
                other,
                &format!("[{count} {}]", if integer { "integers" } else { "numbers" }),
            )],
            None => Vec::new(),
        },
    }
}
