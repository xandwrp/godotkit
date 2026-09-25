//! The JSON grammar gdkit uses to move Godot Variants in and out of harnesses.
//! One definition, shared by `resource create` specs, `resource schema` output,
//! `run` checkpoints. The GDScript codec in `gdproject/harness/protocol.gd`
//! mirrors this exactly; an engine-produced golden fixture
//! (`gdproject/tests/fixtures/protocol_golden.json`) is decoded and re-encoded
//! here byte-for-value, and regenerated from the engine by an ignored
//! real-engine test.
//!
//! Grammar (informal):
//! ```text
//! value   := null | true | false | int | float | string | [value…] | {key: value…}
//!          | tagged | {"$ref": "res://…"} | {"$resource": resource}
//! int     := <JSON number with an integral value, |n| ≤ 2^53-1>
//!          | {"$variant": {"type": "int", "value": "<canonical i64 decimal>"}}
//! float   := <JSON number with a fractional part>
//!          | {"$variant": {"type": "float", "value": <JSON number> | "nan" | "inf" | "-inf" | "-0.0"}}
//! tagged  := {"$variant": {"type": "<VariantType>", "value": <payload>}}
//!          | {"$variant": {"type": "Array", "element": "<VariantType>", "value": [value…]}}
//! resource:= {"class": "<ClassName>" | "script": "res://…", "properties"?: {name: value…}}
//! ```
//!
//! **Numbers.** Godot's JSON parser yields float for every number, so a bare
//! number is classified by *value*, not spelling: `3`, `3.0` and `-0.0` are all
//! the int 3/0; `2.5` is a float. Bare integral numbers beyond ±(2^53-1) are
//! rejected as ambiguous (the reader cannot know the exact integer). Encoders
//! therefore write: ints within ±(2^53-1) bare and larger ints as tagged
//! decimal strings; finite non-integral floats bare; integral floats as a tagged
//! number (`{"type":"float","value":3.0}`); and NaN, ±infinity and -0.0 (whose
//! sign `JSON.stringify` drops) as tagged strings `"nan"`, `"inf"`, `"-inf"`,
//! `"-0.0"`. Non-finite floats exist **only** in that tagged string form. Tagged
//! int strings must be canonical: optional `-`, no leading zeros, no `-0`,
//! digits only, within i64.
//!
//! **Components.** Inside a tag that fixes the component type (vectors, colors,
//! rects, planes, quaternions, AABB, bases, transforms, projections and packed
//! numeric arrays) a component is a bare number or a tagged int/float scalar,
//! and integral float components stay bare (`[1.0, 2.0]`). Integer components
//! must be integral and in range (i32 for `*i` types and `PackedInt32Array`,
//! 0..=255 for `PackedByteArray`). Payloads are flat component arrays in Godot
//! constructor order (`Rect2`: position then size; `Plane`: normal then d;
//! bases and transforms column vectors x, y, z then origin; `Projection` four
//! columns). `PackedVector*Array`/`PackedColorArray` items are full tagged
//! values of the element type; `PackedStringArray` items are strings.
//!
//! **Containers.** Dictionaries whose keys are all `String` and none of
//! `$variant`, `$ref`, `$resource` are plain JSON objects; any other dictionary
//! is `{"type": "Dictionary", "value": [[key, value]…]}` with duplicate keys
//! rejected (`String` and `StringName` keys collide, as in Godot). Typed arrays
//! carry `element` (not Nil, Object, RID, Callable or Signal) and every item
//! must be of that type. An object with a reserved key must be exactly that one
//! tag; tag objects accept no unknown fields. `$ref` and resource `script`
//! paths are valid `res://` paths, never `.godot` segments, control
//! characters, or built-in `::` sub-resource paths. Rust does not preserve plain
//! object key order (serde_json maps are sorted).
//!
//! **Limits.** Both sides enforce the same [`Limits`]: every visited value
//! (root, array item, dictionary value, tagged-dictionary key, tuple or packed
//! component, typed array item, resource property) counts one entry against a
//! budget for the whole value, and has depth parent+1 from a root at depth 0.
//! The GDScript encoder additionally tracks the containers and inline resources
//! on the current path by identity and rejects cycles immediately; the depth
//! limit and entry budget bound everything else, including shared subgraphs.
//!
//! Godot's parser loses precision on some subnormal floats (below ~2.2e-308);
//! those do not round-trip through the engine exactly.
//!
//! # Tests (tests/variant.rs)
//! - `plain_json_scalars_map_to_untagged_variants`
//! - `bare_numbers_classify_by_value_and_integral_floats_are_tagged`
//! - `tagged_int_accepts_decimal_string_above_2_53_and_rejects_non_canonical`
//! - `tagged_vectors_colors_rects_transforms_require_exact_component_counts`
//! - `packed_arrays_and_typed_arrays_carry_element_type`
//! - `dictionaries_escape_reserved_and_non_string_keys_and_reject_duplicates`
//! - `ref_requires_res_path_and_rejects_dot_godot`
//! - `resource_requires_exactly_one_of_class_or_script`
//! - `tags_reject_unknown_types_and_fields`
//! - `depth_and_entry_limits_are_enforced`
//! - `to_json_then_from_json_is_identity_for_every_variant_type`
//! - `non_finite_floats_are_accepted_only_in_tagged_string_form`
//! - `variant_type_names_and_codes_match_godot`
//!
//! The engine golden is decoded and re-encoded by
//! `gdproject/tests/protocol.rs::engine_golden_cases_decode_and_reencode_identically`.

use std::collections::{BTreeMap, HashSet};
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value, json};

use crate::respath::ResPath;

/// 2^53 - 1: the largest integer every JSON reader, including Godot's
/// float-only parser, represents exactly.
pub const MAX_SAFE_INTEGER: i64 = (1 << 53) - 1;

const RESERVED_KEYS: [&str; 3] = ["$variant", "$ref", "$resource"];

#[derive(Clone, Debug, PartialEq)]
pub enum VariantJson {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    /// Any Godot type not representable as plain JSON: StringName, NodePath
    /// (`value` is [`VariantJson::String`]), Vector*, Color, Rect2*, AABB, Plane,
    /// Quaternion, Basis, Transform*, Projection (`value` is an
    /// [`VariantJson::Array`] of `Float` or `Int` components) and all
    /// Packed*Array (`value` is an `Array` of `Int`, `Float`, `String` or
    /// element-typed `Tagged` items). `int`, `float`, `Array` and `Dictionary`
    /// tags decode to their dedicated variants instead.
    Tagged {
        type_name: VariantType,
        value: Box<VariantJson>,
    },
    Array(Vec<VariantJson>),
    TypedArray {
        element: VariantType,
        items: Vec<VariantJson>,
    },
    Dictionary(Vec<(VariantJson, VariantJson)>),
    Ref(ResPath),
    Resource(ResourceSpec),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResourceSpec {
    pub target: ResourceTarget,
    pub properties: BTreeMap<String, VariantJson>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceTarget {
    Class(String),
    Script(ResPath),
}

/// Godot's `Variant.Type`, named as Godot's `type_string` spells it (and as it
/// appears in JSON), e.g. `"Vector3"`, `"AABB"`, `"int"`. Discriminants are
/// Godot 4's numeric type codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "&'static str")]
#[non_exhaustive]
pub enum VariantType {
    Nil = 0,
    Bool = 1,
    Int = 2,
    Float = 3,
    String = 4,
    Vector2 = 5,
    Vector2i = 6,
    Rect2 = 7,
    Rect2i = 8,
    Vector3 = 9,
    Vector3i = 10,
    Transform2D = 11,
    Vector4 = 12,
    Vector4i = 13,
    Plane = 14,
    Quaternion = 15,
    Aabb = 16,
    Basis = 17,
    Transform3D = 18,
    Projection = 19,
    Color = 20,
    StringName = 21,
    NodePath = 22,
    Rid = 23,
    Object = 24,
    Callable = 25,
    Signal = 26,
    Dictionary = 27,
    Array = 28,
    PackedByteArray = 29,
    PackedInt32Array = 30,
    PackedInt64Array = 31,
    PackedFloat32Array = 32,
    PackedFloat64Array = 33,
    PackedStringArray = 34,
    PackedVector2Array = 35,
    PackedVector3Array = 36,
    PackedColorArray = 37,
    PackedVector4Array = 38,
}

impl VariantType {
    /// Every type, in code order (`ALL[code]`).
    pub const ALL: [VariantType; 39] = {
        use VariantType::*;
        [
            Nil,
            Bool,
            Int,
            Float,
            String,
            Vector2,
            Vector2i,
            Rect2,
            Rect2i,
            Vector3,
            Vector3i,
            Transform2D,
            Vector4,
            Vector4i,
            Plane,
            Quaternion,
            Aabb,
            Basis,
            Transform3D,
            Projection,
            Color,
            StringName,
            NodePath,
            Rid,
            Object,
            Callable,
            Signal,
            Dictionary,
            Array,
            PackedByteArray,
            PackedInt32Array,
            PackedInt64Array,
            PackedFloat32Array,
            PackedFloat64Array,
            PackedStringArray,
            PackedVector2Array,
            PackedVector3Array,
            PackedColorArray,
            PackedVector4Array,
        ]
    };

    pub fn name(self) -> &'static str {
        use VariantType::*;
        match self {
            Nil => "Nil",
            Bool => "bool",
            Int => "int",
            Float => "float",
            String => "String",
            Vector2 => "Vector2",
            Vector2i => "Vector2i",
            Rect2 => "Rect2",
            Rect2i => "Rect2i",
            Vector3 => "Vector3",
            Vector3i => "Vector3i",
            Transform2D => "Transform2D",
            Vector4 => "Vector4",
            Vector4i => "Vector4i",
            Plane => "Plane",
            Quaternion => "Quaternion",
            Aabb => "AABB",
            Basis => "Basis",
            Transform3D => "Transform3D",
            Projection => "Projection",
            Color => "Color",
            StringName => "StringName",
            NodePath => "NodePath",
            Rid => "RID",
            Object => "Object",
            Callable => "Callable",
            Signal => "Signal",
            Dictionary => "Dictionary",
            Array => "Array",
            PackedByteArray => "PackedByteArray",
            PackedInt32Array => "PackedInt32Array",
            PackedInt64Array => "PackedInt64Array",
            PackedFloat32Array => "PackedFloat32Array",
            PackedFloat64Array => "PackedFloat64Array",
            PackedStringArray => "PackedStringArray",
            PackedVector2Array => "PackedVector2Array",
            PackedVector3Array => "PackedVector3Array",
            PackedColorArray => "PackedColorArray",
            PackedVector4Array => "PackedVector4Array",
        }
    }

    /// Exact, case-sensitive inverse of [`VariantType::name`].
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|ty| ty.name() == name)
    }

    /// Godot's numeric `Variant.Type` value, for harness round trips.
    pub fn code(self) -> u32 {
        self as u32
    }

    pub fn from_code(code: u32) -> Option<Self> {
        Self::ALL.get(usize::try_from(code).ok()?).copied()
    }

    /// Tuple payload shape: component count and component kind.
    fn tuple(self) -> Option<(usize, Kind)> {
        use VariantType::*;
        Some(match self {
            Vector2 => (2, Kind::Float),
            Vector2i => (2, Kind::I32),
            Rect2 => (4, Kind::Float),
            Rect2i => (4, Kind::I32),
            Vector3 => (3, Kind::Float),
            Vector3i => (3, Kind::I32),
            Transform2D => (6, Kind::Float),
            Vector4 => (4, Kind::Float),
            Vector4i => (4, Kind::I32),
            Plane => (4, Kind::Float),
            Quaternion => (4, Kind::Float),
            Aabb => (6, Kind::Float),
            Basis => (9, Kind::Float),
            Transform3D => (12, Kind::Float),
            Projection => (16, Kind::Float),
            Color => (4, Kind::Float),
            _ => return None,
        })
    }

    fn packed_element(self) -> Option<Kind> {
        use VariantType::*;
        Some(match self {
            PackedByteArray => Kind::U8,
            PackedInt32Array => Kind::I32,
            PackedInt64Array => Kind::I64,
            PackedFloat32Array | PackedFloat64Array => Kind::Float,
            PackedStringArray => Kind::Of(String),
            PackedVector2Array => Kind::Of(Vector2),
            PackedVector3Array => Kind::Of(Vector3),
            PackedColorArray => Kind::Of(Color),
            PackedVector4Array => Kind::Of(Vector4),
            _ => return None,
        })
    }

    fn typed_array_element(self) -> bool {
        use VariantType::*;
        !matches!(self, Nil | Object | Rid | Callable | Signal)
    }
}

impl fmt::Display for VariantType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl From<VariantType> for &'static str {
    fn from(ty: VariantType) -> Self {
        ty.name()
    }
}

impl TryFrom<String> for VariantType {
    type Error = String;

    fn try_from(name: String) -> Result<Self, String> {
        Self::from_name(&name).ok_or_else(|| format!("unknown Variant type {name:?}"))
    }
}

/// Component kinds for payloads whose element type the tag fixes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Float,
    I32,
    I64,
    U8,
    Of(VariantType),
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_depth: usize,
    pub max_entries: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_depth: 32,
            max_entries: 100_000,
        }
    }
}

impl VariantJson {
    /// Validates and decodes one value of the grammar. Errors name the JSON
    /// pointer of the offending value.
    pub fn from_json(value: &serde_json::Value, limits: &Limits) -> crate::Result<Self> {
        Decoder { limits, entries: 0 }.value(value, 0, Loc::Root)
    }

    /// Canonical encoding; `from_json(&v.to_json())` reproduces `v`.
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            VariantJson::Nil => Value::Null,
            VariantJson::Bool(value) => Value::Bool(*value),
            VariantJson::Int(value) => int_json(*value),
            VariantJson::Float(value) => float_json(*value, false),
            VariantJson::String(value) => Value::String(value.clone()),
            VariantJson::Tagged { type_name, value } => {
                let payload = match value.as_ref() {
                    VariantJson::Array(items) => {
                        Value::Array(items.iter().map(component_json).collect())
                    }
                    other => other.to_json(),
                };
                tag(*type_name, payload)
            }
            VariantJson::Array(items) => {
                Value::Array(items.iter().map(VariantJson::to_json).collect())
            }
            VariantJson::TypedArray { element, items } => json!({"$variant": {
                "type": VariantType::Array.name(),
                "element": element.name(),
                "value": items.iter().map(VariantJson::to_json).collect::<Vec<_>>(),
            }}),
            VariantJson::Dictionary(pairs) => {
                let plain = pairs.iter().all(|(key, _)| {
                    matches!(key, VariantJson::String(key) if !RESERVED_KEYS.contains(&key.as_str()))
                });
                if plain {
                    let mut object = Map::new();
                    for (key, value) in pairs {
                        if let VariantJson::String(key) = key {
                            object.insert(key.clone(), value.to_json());
                        }
                    }
                    Value::Object(object)
                } else {
                    let pairs = pairs
                        .iter()
                        .map(|(key, value)| json!([key.to_json(), value.to_json()]))
                        .collect();
                    tag(VariantType::Dictionary, Value::Array(pairs))
                }
            }
            VariantJson::Ref(path) => json!({"$ref": path.as_str()}),
            VariantJson::Resource(spec) => {
                let mut object = Map::new();
                match &spec.target {
                    ResourceTarget::Class(class) => {
                        object.insert("class".into(), Value::String(class.clone()))
                    }
                    ResourceTarget::Script(script) => {
                        object.insert("script".into(), Value::String(script.to_string()))
                    }
                };
                let properties = spec
                    .properties
                    .iter()
                    .map(|(name, value)| (name.clone(), value.to_json()))
                    .collect();
                object.insert("properties".into(), Value::Object(properties));
                json!({"$resource": object})
            }
        }
    }

    /// The Godot type this will become, when statically known.
    pub fn variant_type(&self) -> Option<VariantType> {
        Some(match self {
            VariantJson::Nil => VariantType::Nil,
            VariantJson::Bool(_) => VariantType::Bool,
            VariantJson::Int(_) => VariantType::Int,
            VariantJson::Float(_) => VariantType::Float,
            VariantJson::String(_) => VariantType::String,
            VariantJson::Tagged { type_name, .. } => *type_name,
            VariantJson::Array(_) | VariantJson::TypedArray { .. } => VariantType::Array,
            VariantJson::Dictionary(_) => VariantType::Dictionary,
            VariantJson::Ref(_) | VariantJson::Resource(_) => VariantType::Object,
        })
    }
}

fn tag(ty: VariantType, value: Value) -> Value {
    json!({"$variant": {"type": ty.name(), "value": value}})
}

fn int_json(value: i64) -> Value {
    if (-MAX_SAFE_INTEGER..=MAX_SAFE_INTEGER).contains(&value) {
        Value::from(value)
    } else {
        tag(VariantType::Int, Value::String(value.to_string()))
    }
}

/// `component` floats sit where the tag fixes the type, so integral values stay bare.
fn float_json(value: f64, component: bool) -> Value {
    let special = if value.is_nan() {
        "nan"
    } else if value == f64::INFINITY {
        "inf"
    } else if value == f64::NEG_INFINITY {
        "-inf"
    } else if value == 0.0 && value.is_sign_negative() {
        "-0.0"
    } else {
        let number = Value::Number(Number::from_f64(value).expect("finite float"));
        return if component || value.fract() != 0.0 {
            number
        } else {
            tag(VariantType::Float, number)
        };
    };
    tag(VariantType::Float, Value::String(special.into()))
}

fn component_json(value: &VariantJson) -> Value {
    match value {
        VariantJson::Float(value) => float_json(*value, true),
        other => other.to_json(),
    }
}

/// A JSON pointer to the value being decoded, rendered only on error.
#[derive(Clone, Copy)]
enum Loc<'a> {
    Root,
    Index(&'a Loc<'a>, usize),
    Key(&'a Loc<'a>, &'a str),
}

impl fmt::Display for Loc<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Loc::Root => Ok(()),
            Loc::Index(parent, index) => write!(f, "{parent}/{index}"),
            Loc::Key(parent, key) => {
                write!(f, "{parent}/{}", key.replace('~', "~0").replace('/', "~1"))
            }
        }
    }
}

fn error(at: Loc<'_>, message: impl fmt::Display) -> crate::Error {
    let at = at.to_string();
    let at = if at.is_empty() { "/" } else { &at };
    crate::Error::Variant(format!("at {at}: {message}"))
}

enum Scalar {
    Int(i64),
    Float(f64),
}

struct Decoder<'l> {
    limits: &'l Limits,
    entries: usize,
}

impl Decoder<'_> {
    fn visit(&mut self, depth: usize, at: Loc<'_>) -> crate::Result<()> {
        self.entries += 1;
        if self.entries > self.limits.max_entries {
            return Err(error(
                at,
                format_args!("value exceeds {} entries", self.limits.max_entries),
            ));
        }
        if depth > self.limits.max_depth {
            return Err(error(
                at,
                format_args!("nesting exceeds depth {}", self.limits.max_depth),
            ));
        }
        Ok(())
    }

    fn value(&mut self, json: &Value, depth: usize, at: Loc<'_>) -> crate::Result<VariantJson> {
        self.visit(depth, at)?;
        let object = match json {
            Value::Null => return Ok(VariantJson::Nil),
            Value::Bool(value) => return Ok(VariantJson::Bool(*value)),
            Value::String(value) => return Ok(VariantJson::String(value.clone())),
            Value::Number(number) => {
                return Ok(match bare_number(number, false, at)? {
                    Scalar::Int(value) => VariantJson::Int(value),
                    Scalar::Float(value) => VariantJson::Float(value),
                });
            }
            Value::Array(items) => {
                let mut result = Vec::with_capacity(items.len());
                for (index, item) in items.iter().enumerate() {
                    result.push(self.value(item, depth + 1, Loc::Index(&at, index))?);
                }
                return Ok(VariantJson::Array(result));
            }
            Value::Object(object) => object,
        };
        if let Some(path) = object.get("$ref") {
            let path = match path {
                Value::String(path) if object.len() == 1 => transportable_path(path),
                _ => None,
            };
            return path
                .map(VariantJson::Ref)
                .ok_or_else(|| error(at, "invalid resource reference"));
        }
        if let Some(spec) = object.get("$resource") {
            return self.resource(object.len(), spec, depth, Loc::Key(&at, "$resource"));
        }
        if let Some(tag) = object.get("$variant") {
            let at = Loc::Key(&at, "$variant");
            return match tag {
                Value::Object(tag) if object.len() == 1 => self.tagged(tag, depth, at),
                _ => Err(error(at, "invalid Variant tag")),
            };
        }
        let mut pairs = Vec::with_capacity(object.len());
        for (key, value) in object {
            let value = self.value(value, depth + 1, Loc::Key(&at, key))?;
            pairs.push((VariantJson::String(key.clone()), value));
        }
        Ok(VariantJson::Dictionary(pairs))
    }

    fn resource(
        &mut self,
        siblings: usize,
        spec: &Value,
        depth: usize,
        at: Loc<'_>,
    ) -> crate::Result<VariantJson> {
        let spec = match spec {
            Value::Object(spec) if siblings == 1 => spec,
            _ => return Err(error(at, "invalid resource spec")),
        };
        if let Some(key) = spec
            .keys()
            .find(|key| !matches!(key.as_str(), "class" | "script" | "properties"))
        {
            return Err(error(
                at,
                format_args!("unknown resource spec field {key:?}"),
            ));
        }
        let target = match (spec.get("class"), spec.get("script")) {
            (Some(Value::String(class)), None) if !class.is_empty() => {
                ResourceTarget::Class(class.clone())
            }
            (Some(_), None) => return Err(error(at, "resource class must be a class name")),
            (None, Some(script)) => script
                .as_str()
                .and_then(transportable_path)
                .map(ResourceTarget::Script)
                .ok_or_else(|| error(at, "resource script must be a saved res:// script path"))?,
            _ => {
                return Err(error(
                    at,
                    "resource requires exactly one of class or script",
                ));
            }
        };
        let mut properties = BTreeMap::new();
        match spec.get("properties") {
            None => {}
            Some(Value::Object(values)) => {
                let at = Loc::Key(&at, "properties");
                for (name, value) in values {
                    let value = self.value(value, depth + 1, Loc::Key(&at, name))?;
                    properties.insert(name.clone(), value);
                }
            }
            Some(_) => return Err(error(at, "resource properties must be an object")),
        }
        Ok(VariantJson::Resource(ResourceSpec { target, properties }))
    }

    fn tagged(
        &mut self,
        tag: &Map<String, Value>,
        depth: usize,
        at: Loc<'_>,
    ) -> crate::Result<VariantJson> {
        let name = tag.get("type").and_then(Value::as_str);
        let ty = name.and_then(VariantType::from_name);
        if let Some(key) = tag.keys().find(|key| {
            !matches!(key.as_str(), "type" | "value")
                && !(key.as_str() == "element" && ty == Some(VariantType::Array))
        }) {
            return Err(error(at, format_args!("unknown Variant tag field {key:?}")));
        }
        let Some(value) = tag.get("value") else {
            return Err(error(at, "Variant tag requires a value"));
        };
        let value_at = Loc::Key(&at, "value");
        let unsupported = || {
            error(
                at,
                format_args!(
                    "unknown or unsupported Variant tag {}",
                    tag.get("type").unwrap_or(&Value::Null)
                ),
            )
        };
        let Some(ty) = ty else {
            return Err(unsupported());
        };
        match ty {
            VariantType::Int => return tagged_int(value, value_at).map(VariantJson::Int),
            VariantType::Float => return tagged_float(value, value_at).map(VariantJson::Float),
            VariantType::StringName | VariantType::NodePath => {
                return match value {
                    Value::String(text) => Ok(VariantJson::Tagged {
                        type_name: ty,
                        value: Box::new(VariantJson::String(text.clone())),
                    }),
                    _ => Err(error(value_at, format_args!("{ty} value must be a string"))),
                };
            }
            VariantType::Dictionary => return self.dictionary(value, depth, value_at),
            VariantType::Array => {
                let element = tag
                    .get("element")
                    .and_then(Value::as_str)
                    .and_then(VariantType::from_name)
                    .filter(|element| element.typed_array_element())
                    .ok_or_else(|| error(at, "unsupported typed array element"))?;
                let Value::Array(values) = value else {
                    return Err(error(value_at, "typed array value must be an array"));
                };
                let mut items = Vec::with_capacity(values.len());
                for (index, item) in values.iter().enumerate() {
                    let item_at = Loc::Index(&value_at, index);
                    let item = self.value(item, depth + 1, item_at)?;
                    if item.variant_type() != Some(element) {
                        return Err(error(
                            item_at,
                            format_args!("typed array item is not {element}"),
                        ));
                    }
                    items.push(item);
                }
                return Ok(VariantJson::TypedArray { element, items });
            }
            _ => {}
        }
        let (count, kind) = if let Some((count, kind)) = ty.tuple() {
            (Some(count), kind)
        } else if let Some(kind) = ty.packed_element() {
            (None, kind)
        } else {
            return Err(unsupported());
        };
        let values = match (value, count) {
            (Value::Array(values), Some(count)) if values.len() == count => values,
            (Value::Array(values), None) => values,
            (_, Some(count)) => {
                return Err(error(
                    value_at,
                    format_args!("{ty} value must be an array of {count} numbers"),
                ));
            }
            (_, None) => return Err(error(value_at, format_args!("{ty} value must be an array"))),
        };
        let mut items = Vec::with_capacity(values.len());
        for (index, item) in values.iter().enumerate() {
            items.push(self.element(item, kind, depth + 1, Loc::Index(&value_at, index))?);
        }
        Ok(VariantJson::Tagged {
            type_name: ty,
            value: Box::new(VariantJson::Array(items)),
        })
    }

    fn dictionary(
        &mut self,
        value: &Value,
        depth: usize,
        at: Loc<'_>,
    ) -> crate::Result<VariantJson> {
        let malformed = || {
            error(
                at,
                "Dictionary value must be an array of [key, value] pairs",
            )
        };
        let Value::Array(values) = value else {
            return Err(malformed());
        };
        let mut seen = HashSet::new();
        let mut pairs = Vec::with_capacity(values.len());
        for (index, pair) in values.iter().enumerate() {
            let pair_at = Loc::Index(&at, index);
            let Some([key, value]) = pair
                .as_array()
                .map(Vec::as_slice)
                .and_then(|pair| <&[Value; 2]>::try_from(pair).ok())
            else {
                return Err(malformed());
            };
            let key = self.value(key, depth + 1, Loc::Index(&pair_at, 0))?;
            // Godot treats String and StringName keys with equal text as one key.
            let identity = match &key {
                VariantJson::String(text) => Value::String(text.clone()),
                VariantJson::Tagged {
                    type_name: VariantType::StringName,
                    value,
                } => value.to_json(),
                other => other.to_json(),
            };
            if !seen.insert(identity.to_string()) {
                return Err(error(pair_at, "duplicate dictionary key"));
            }
            let value = self.value(value, depth + 1, Loc::Index(&pair_at, 1))?;
            pairs.push((key, value));
        }
        Ok(VariantJson::Dictionary(pairs))
    }

    fn element(
        &mut self,
        json: &Value,
        kind: Kind,
        depth: usize,
        at: Loc<'_>,
    ) -> crate::Result<VariantJson> {
        if let Kind::Of(ty) = kind {
            let item = self.value(json, depth, at)?;
            if item.variant_type() != Some(ty) {
                return Err(error(at, format_args!("expected {ty} element")));
            }
            return Ok(item);
        }
        self.visit(depth, at)?;
        let integer = kind != Kind::Float;
        let scalar = if integer { "int" } else { "float" };
        let number = match json {
            Value::Number(number) => bare_number(number, integer, at)?,
            Value::Object(object) if object.len() == 1 => {
                let tag = object
                    .get("$variant")
                    .and_then(Value::as_object)
                    .filter(|tag| tag.len() == 2 && tag.get("type") == Some(&json!(scalar)))
                    .and_then(|tag| tag.get("value"))
                    .ok_or_else(|| error(at, format_args!("expected a {scalar} component")))?;
                let at = Loc::Key(&at, "$variant");
                if integer {
                    Scalar::Int(tagged_int(tag, at)?)
                } else {
                    Scalar::Float(tagged_float(tag, at)?)
                }
            }
            _ => return Err(error(at, format_args!("expected a {scalar} component"))),
        };
        let value = match (number, kind) {
            (Scalar::Int(value), Kind::Float) => return Ok(VariantJson::Float(value as f64)),
            (Scalar::Float(value), _) => return Ok(VariantJson::Float(value)),
            (Scalar::Int(value), _) => value,
        };
        let in_range = match kind {
            Kind::I32 => i32::try_from(value).is_ok(),
            Kind::U8 => u8::try_from(value).is_ok(),
            _ => true,
        };
        if !in_range {
            return Err(error(at, format_args!("component {value} is out of range")));
        }
        Ok(VariantJson::Int(value))
    }
}

/// Classifies a bare number by value, as Godot's float-only parser must.
fn bare_number(number: &Number, integer: bool, at: Loc<'_>) -> crate::Result<Scalar> {
    let unsafe_integer = || error(at, "integers beyond ±(2^53-1) must use the tagged int form");
    if let Some(value) = number.as_i64() {
        return if (-MAX_SAFE_INTEGER..=MAX_SAFE_INTEGER).contains(&value) {
            Ok(Scalar::Int(value))
        } else {
            Err(unsafe_integer())
        };
    }
    if number.is_u64() {
        return Err(unsafe_integer());
    }
    let value = number.as_f64().expect("serde_json numbers are finite");
    if value.fract() != 0.0 {
        return if integer {
            Err(error(
                at,
                format_args!("expected an integer, found {value}"),
            ))
        } else {
            Ok(Scalar::Float(value))
        };
    }
    if value.abs() > MAX_SAFE_INTEGER as f64 {
        return Err(error(
            at,
            "integral numbers beyond ±(2^53-1) are ambiguous; use a tagged int or float",
        ));
    }
    Ok(Scalar::Int(value as i64))
}

fn tagged_int(value: &Value, at: Loc<'_>) -> crate::Result<i64> {
    value
        .as_str()
        .filter(|text| canonical_int(text))
        .and_then(|text| text.parse().ok())
        .ok_or_else(|| {
            error(
                at,
                "tagged int value must be a canonical decimal string in the int64 range",
            )
        })
}

/// Optional '-', no leading zeros (and no "-0"), ASCII digits only.
fn canonical_int(text: &str) -> bool {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(digits) => (true, digits),
        None => (false, text),
    };
    !digits.is_empty()
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && !(digits.starts_with('0') && (digits.len() > 1 || negative))
}

fn tagged_float(value: &Value, at: Loc<'_>) -> crate::Result<f64> {
    match value {
        Value::String(text) => match text.as_str() {
            "nan" => Ok(f64::NAN),
            "inf" => Ok(f64::INFINITY),
            "-inf" => Ok(f64::NEG_INFINITY),
            "-0.0" => Ok(-0.0),
            _ => Err(error(
                at,
                "tagged float strings are nan, inf, -inf and -0.0",
            )),
        },
        Value::Number(number) => Ok(number.as_f64().expect("serde_json numbers are finite")),
        _ => Err(error(
            at,
            "tagged float value must be a finite number or nan, inf, -inf, -0.0",
        )),
    }
}

/// A `res://` path a `$ref` or resource `script` may name, matching
/// `protocol.gd`'s `transportable_res_path`.
fn transportable_path(text: &str) -> Option<ResPath> {
    let path = ResPath::parse(text).ok()?;
    let relative = path.relative();
    let valid = !relative.is_empty()
        && !relative.contains("::")
        && !relative.chars().any(|c| u32::from(c) < 32)
        && relative.split('/').all(|segment| segment != ".godot");
    valid.then_some(path)
}
