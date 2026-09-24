//! The JSON grammar gdkit uses to move Godot Variants in and out of harnesses.
//! One definition, shared by `resource create` specs, `resource schema` output,
//! `inspect` checkpoints, and `animation inspect`. The GDScript encoder in
//! `gdproject/harness/protocol.gd` must mirror this exactly.
//!
//! Grammar (informal):
//! ```text
//! null | bool | number | string | [value…] | {key: value…}
//! {"$variant": {"type": "<VariantType>", "value": <payload>}}   tagged scalar / container
//! {"$ref": "res://…"}                                              external resource
//! {"$resource": {"class"|"script": …, "properties": {…}}}         nested resource
//! ```
//! Integers outside ±2^53 travel as tagged `{"type":"int","value":"<decimal string>"}`.
//! Floats are always decoded as f64; harnesses that need float32 storage verify
//! the round trip and reject precision loss (that is a harness concern, not this type's).
//!
//! # Tests (tests/variant.rs)
//! - `plain_json_scalars_map_to_untagged_variants`
//! - `tagged_int_accepts_decimal_string_above_2_53_and_rejects_non_canonical`
//! - `tagged_vectors_colors_rects_transforms_require_exact_component_counts`
//! - `packed_arrays_and_typed_arrays_carry_element_type`
//! - `ref_requires_res_path_and_rejects_dot_godot`
//! - `resource_requires_exactly_one_of_class_or_script`
//! - `depth_and_entry_limits_are_enforced`
//! - `to_json_then_from_json_is_identity_for_every_variant_type`
//! - `nan_and_infinity_are_rejected_on_input`

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::respath::ResPath;

#[derive(Clone, Debug, PartialEq)]
pub enum VariantJson {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    /// Any Godot type not representable as plain JSON: StringName, NodePath,
    /// Vector*, Color, Rect2*, AABB, Plane, Quaternion, Basis, Transform*,
    /// Projection, RID-less types, and all Packed*Array.
    Tagged { type_name: VariantType, value: Box<VariantJson> },
    Array(Vec<VariantJson>),
    TypedArray { element: VariantType, items: Vec<VariantJson> },
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

/// Godot's `Variant.Type` names as they appear in JSON, e.g. `"Vector3"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum VariantType {
    Nil, Bool, Int, Float, String, StringName, NodePath,
    Vector2, Vector2i, Vector3, Vector3i, Vector4, Vector4i,
    Rect2, Rect2i, Aabb, Plane, Quaternion, Basis, Transform2D, Transform3D, Projection,
    Color, Rid, Object, Callable, Signal, Dictionary, Array,
    PackedByteArray, PackedInt32Array, PackedInt64Array, PackedFloat32Array, PackedFloat64Array,
    PackedStringArray, PackedVector2Array, PackedVector3Array, PackedVector4Array, PackedColorArray,
}

impl VariantType {
    pub fn name(self) -> &'static str {
        todo!()
    }
    pub fn from_name(name: &str) -> Option<Self> {
        todo!()
    }
    /// Godot's numeric `Variant.Type` value, for harness round trips.
    pub fn code(self) -> u32 {
        todo!()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_depth: usize,
    pub max_entries: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self { max_depth: 32, max_entries: 100_000 }
    }
}

impl VariantJson {
    pub fn from_json(value: &serde_json::Value, limits: &Limits) -> crate::Result<Self> {
        todo!()
    }
    pub fn to_json(&self) -> serde_json::Value {
        todo!()
    }
    /// The Godot type this will become, when statically known.
    pub fn variant_type(&self) -> Option<VariantType> {
        todo!()
    }
}
