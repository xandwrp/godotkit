// Acceptance tests for gdview::variant. The engine golden fixture is decoded and
// re-encoded in gdproject/tests/protocol.rs.

use std::collections::BTreeMap;

use gdview::ResPath;
use gdview::variant::{
    Limits, MAX_SAFE_INTEGER, ResourceSpec, ResourceTarget, VariantJson, VariantType,
};
use serde_json::{Value, json};

fn decode(json: Value) -> VariantJson {
    VariantJson::from_json(&json, &Limits::default()).unwrap_or_else(|e| panic!("{json}: {e}"))
}

fn reject(json: Value) -> String {
    match VariantJson::from_json(&json, &Limits::default()) {
        Ok(value) => panic!("accepted {json} as {value:?}"),
        Err(error) => error.to_string(),
    }
}

fn tagged(ty: &str, value: Value) -> Value {
    json!({"$variant": {"type": ty, "value": value}})
}

fn tuple(ty: VariantType, items: Vec<VariantJson>) -> VariantJson {
    VariantJson::Tagged {
        type_name: ty,
        value: Box::new(VariantJson::Array(items)),
    }
}

fn floats(values: &[f64]) -> Vec<VariantJson> {
    values.iter().copied().map(VariantJson::Float).collect()
}

fn ints(values: &[i64]) -> Vec<VariantJson> {
    values.iter().copied().map(VariantJson::Int).collect()
}

/// Canonical round trip: encoding is stable and decodes back to an equal encoding.
fn assert_canonical(json: Value) {
    let value = decode(json.clone());
    assert_eq!(value.to_json(), json, "{value:?}");
}

#[test]
fn plain_json_scalars_map_to_untagged_variants() {
    assert_eq!(decode(json!(null)), VariantJson::Nil);
    assert_eq!(decode(json!(true)), VariantJson::Bool(true));
    assert_eq!(decode(json!(false)), VariantJson::Bool(false));
    assert_eq!(decode(json!(42)), VariantJson::Int(42));
    assert_eq!(decode(json!(-42)), VariantJson::Int(-42));
    assert_eq!(decode(json!(1.25)), VariantJson::Float(1.25));
    assert_eq!(
        decode(json!("héllo\u{1}")),
        VariantJson::String("héllo\u{1}".into())
    );
    assert_eq!(
        decode(json!([1, "two", null])),
        VariantJson::Array(vec![
            VariantJson::Int(1),
            VariantJson::String("two".into()),
            VariantJson::Nil
        ])
    );
    assert_eq!(
        decode(json!({"key": [3]})),
        VariantJson::Dictionary(vec![(
            VariantJson::String("key".into()),
            VariantJson::Array(vec![VariantJson::Int(3)])
        )])
    );
    for value in [
        json!(null),
        json!(true),
        json!(42),
        json!(1.25),
        json!("s"),
        json!([]),
        json!({}),
    ] {
        assert_canonical(value);
    }
}

#[test]
fn bare_numbers_classify_by_value_and_integral_floats_are_tagged() {
    // Godot's parser yields float for every number, so spelling is irrelevant.
    assert_eq!(decode(json!(3.0)), VariantJson::Int(3));
    assert_eq!(decode(json!(-0.0)), VariantJson::Int(0));
    assert_eq!(decode(json!(1e2)), VariantJson::Int(100));
    assert_eq!(
        decode(json!(MAX_SAFE_INTEGER)),
        VariantJson::Int(MAX_SAFE_INTEGER)
    );
    assert_eq!(
        decode(json!(-MAX_SAFE_INTEGER)),
        VariantJson::Int(-MAX_SAFE_INTEGER)
    );
    for ambiguous in [
        json!(MAX_SAFE_INTEGER + 1),
        json!(-MAX_SAFE_INTEGER - 1),
        json!(u64::MAX),
        json!(1e20),
        json!(-9007199254740992.0),
    ] {
        reject(ambiguous);
    }
    // Integral floats and signed zero keep their type through a tag.
    assert_eq!(
        VariantJson::Float(3.0).to_json(),
        tagged("float", json!(3.0))
    );
    assert_eq!(
        VariantJson::Float(1e20).to_json(),
        tagged("float", json!(1e20))
    );
    assert_eq!(VariantJson::Float(0.1).to_json(), json!(0.1));
    assert_eq!(
        VariantJson::Float(-0.0).to_json(),
        tagged("float", json!("-0.0"))
    );
    assert_eq!(
        VariantJson::Float(0.0).to_json(),
        tagged("float", json!(0.0))
    );
    let VariantJson::Float(zero) = decode(tagged("float", json!("-0.0"))) else {
        panic!()
    };
    assert!(zero == 0.0 && zero.is_sign_negative());
    assert_eq!(decode(tagged("float", json!(3))), VariantJson::Float(3.0));
    assert_eq!(decode(tagged("float", json!(2.5))), VariantJson::Float(2.5));
    // A bare int and an integral float are distinct dictionary keys.
    assert_canonical(tagged(
        "Dictionary",
        json!([[1, "int"], [tagged("float", json!(1.0)), "float"]]),
    ));
    assert_canonical(json!([tagged("float", json!(2.0)), 2]));
}

#[test]
fn tagged_int_accepts_decimal_string_above_2_53_and_rejects_non_canonical() {
    for (text, value) in [
        ("9007199254740992", 9007199254740992),
        ("9223372036854775807", i64::MAX),
        ("-9223372036854775808", i64::MIN),
        ("0", 0),
        ("-17", -17),
    ] {
        assert_eq!(
            decode(tagged("int", json!(text))),
            VariantJson::Int(value),
            "{text}"
        );
    }
    assert_eq!(
        VariantJson::Int(MAX_SAFE_INTEGER + 1).to_json(),
        tagged("int", json!("9007199254740992"))
    );
    assert_eq!(
        VariantJson::Int(i64::MIN).to_json(),
        tagged("int", json!("-9223372036854775808"))
    );
    assert_eq!(
        VariantJson::Int(MAX_SAFE_INTEGER).to_json(),
        json!(MAX_SAFE_INTEGER)
    );
    for bad in [
        json!("12abc"),
        json!("007"),
        json!("-0"),
        json!("+1"),
        json!(""),
        json!("-"),
        json!(" 1"),
        json!("1.0"),
        json!("1e3"),
        json!("٣"),
        json!("9223372036854775808"),
        json!("-9223372036854775809"),
        json!(12),
        json!(null),
    ] {
        let message = reject(tagged("int", bad.clone()));
        assert!(message.contains("canonical decimal"), "{bad}: {message}");
    }
}

#[test]
fn tagged_vectors_colors_rects_transforms_require_exact_component_counts() {
    for (ty, count, integer) in [
        // (type, component count, integer components)
        (VariantType::Vector2, 2_usize, false),
        (VariantType::Vector2i, 2, true),
        (VariantType::Rect2, 4, false),
        (VariantType::Rect2i, 4, true),
        (VariantType::Vector3, 3, false),
        (VariantType::Vector3i, 3, true),
        (VariantType::Transform2D, 6, false),
        (VariantType::Vector4, 4, false),
        (VariantType::Vector4i, 4, true),
        (VariantType::Plane, 4, false),
        (VariantType::Quaternion, 4, false),
        (VariantType::Aabb, 6, false),
        (VariantType::Basis, 9, false),
        (VariantType::Transform3D, 12, false),
        (VariantType::Projection, 16, false),
        (VariantType::Color, 4, false),
    ] {
        let name = ty.name();
        let components: Vec<i64> = (1..=count as i64).collect();
        let value = decode(tagged(name, json!(components)));
        let expected = if integer {
            ints(&components)
        } else {
            floats(&components.iter().map(|&c| c as f64).collect::<Vec<_>>())
        };
        assert_eq!(value, tuple(ty, expected), "{name}");
        assert_eq!(value.variant_type(), Some(ty));
        for wrong in [count - 1, count + 1] {
            let message = reject(tagged(name, json!(vec![0; wrong])));
            assert!(message.contains(&format!("{count} numbers")), "{message}");
        }
        reject(tagged(name, json!("1, 2")));
        reject(tagged(name, json!(vec![json!("1"); count])));
    }
    // Float components stay bare even when integral; specials are tagged.
    let vector = tuple(VariantType::Vector2, floats(&[1.0, -0.0]));
    assert_eq!(
        vector.to_json(),
        tagged("Vector2", json!([1.0, tagged("float", json!("-0.0"))]))
    );
    assert_canonical(tagged(
        "Vector3",
        json!([
            tagged("float", json!("nan")),
            tagged("float", json!("inf")),
            0.5
        ]),
    ));
    // Integer components are integral and in i32 range.
    assert_eq!(
        decode(tagged("Vector2i", json!([2147483647, -2147483648]))),
        tuple(VariantType::Vector2i, ints(&[2147483647, -2147483648]))
    );
    reject(tagged("Vector2i", json!([1.5, 2])));
    reject(tagged("Vector2i", json!([2147483648_i64, 2])));
    reject(tagged("Rect2i", json!([0, 0, 0, -2147483649_i64])));
    reject(tagged("Vector2i", json!([tagged("float", json!(1.0)), 2])));
    reject(tagged("Vector2", json!([tagged("int", json!("1")), 2])));
}

#[test]
fn packed_arrays_and_typed_arrays_carry_element_type() {
    assert_eq!(
        decode(tagged("PackedByteArray", json!([0, 255]))),
        tuple(VariantType::PackedByteArray, ints(&[0, 255]))
    );
    reject(tagged("PackedByteArray", json!([256])));
    reject(tagged("PackedByteArray", json!([-1])));
    reject(tagged("PackedInt32Array", json!([2147483648_i64])));
    assert_canonical(tagged(
        "PackedInt64Array",
        json!([
            tagged("int", json!("-9223372036854775808")),
            tagged("int", json!("9223372036854775807")),
            5
        ]),
    ));
    assert_canonical(tagged(
        "PackedFloat64Array",
        json!([
            1.0,
            tagged("float", json!("-0.0")),
            tagged("float", json!("inf"))
        ]),
    ));
    assert_eq!(
        decode(tagged("PackedFloat32Array", json!([1, 0.25]))),
        tuple(VariantType::PackedFloat32Array, floats(&[1.0, 0.25]))
    );
    assert_canonical(tagged("PackedStringArray", json!(["a", "b\u{1f}"])));
    reject(tagged("PackedStringArray", json!([1])));
    let vector = tagged("Vector2", json!([1.0, 1.0]));
    assert_canonical(tagged("PackedVector2Array", json!([vector])));
    reject(tagged("PackedVector2Array", json!([[1.0, 1.0]])));
    reject(tagged("PackedVector3Array", json!([vector])));
    reject(tagged("PackedColorArray", json!({})));

    let typed =
        decode(json!({"$variant": {"type": "Array", "element": "Vector2", "value": [vector]}}));
    assert_eq!(
        typed,
        VariantJson::TypedArray {
            element: VariantType::Vector2,
            items: vec![tuple(VariantType::Vector2, floats(&[1.0, 1.0]))],
        }
    );
    assert_eq!(typed.variant_type(), Some(VariantType::Array));
    assert_canonical(json!({"$variant": {
        "type": "Array", "element": "float", "value": [tagged("float", json!(1.0)), 2.5]
    }}));
    assert_canonical(
        json!({"$variant": {"type": "Array", "element": "Array", "value": [[1], []]}}),
    );
    // Items must have exactly the element type: a bare 1.0 is an int.
    reject(json!({"$variant": {"type": "Array", "element": "float", "value": [1.0]}}));
    reject(json!({"$variant": {"type": "Array", "element": "int", "value": [1.5]}}));
    for element in ["Object", "Nil", "RID", "Callable", "Signal", "Aabb", ""] {
        reject(json!({"$variant": {"type": "Array", "element": element, "value": []}}));
    }
    reject(json!({"$variant": {"type": "Array", "value": []}}));
}

#[test]
fn dictionaries_escape_reserved_and_non_string_keys_and_reject_duplicates() {
    let escaped = VariantJson::Dictionary(vec![(
        VariantJson::String("$ref".into()),
        VariantJson::String("literal".into()),
    )]);
    assert_eq!(
        escaped.to_json(),
        tagged("Dictionary", json!([["$ref", "literal"]]))
    );
    assert_eq!(decode(escaped.to_json()), escaped);
    assert_canonical(tagged(
        "Dictionary",
        json!([[
            tagged("Vector2i", json!([1, 2])),
            tagged("StringName", json!("value"))
        ]]),
    ));
    assert_canonical(tagged(
        "Dictionary",
        json!([[tagged("StringName", json!("key")), 1]]),
    ));
    // A tagged dictionary with plain keys is valid but not canonical.
    assert_eq!(
        decode(tagged("Dictionary", json!([["a", 1]]))).to_json(),
        json!({"a": 1})
    );
    for duplicate in [
        json!([["a", 1], ["a", 2]]),
        json!([["a", 1], [tagged("StringName", json!("a")), 2]]),
        json!([[1, 1], [1.0, 2]]),
        json!([
            [tagged("float", json!("nan")), 1],
            [tagged("float", json!("nan")), 2]
        ]),
    ] {
        let message = reject(tagged("Dictionary", duplicate));
        assert!(message.contains("duplicate"), "{message}");
    }
    for malformed in [
        json!({}),
        json!([["a"]]),
        json!([["a", 1, 2]]),
        json!(["a"]),
    ] {
        reject(tagged("Dictionary", malformed));
    }
}

#[test]
fn ref_requires_res_path_and_rejects_dot_godot() {
    assert_eq!(
        decode(json!({"$ref": "res://assets/test.tres"})),
        VariantJson::Ref(ResPath::parse("res://assets/test.tres").unwrap())
    );
    assert_canonical(json!({"$ref": "res://a b/ü.tres"}));
    for bad in [
        json!("res://.godot/cache.tres"),
        json!("res://addons/.godot/x.tres"),
        json!("res://"),
        json!("res://../x.tres"),
        json!("res://a//b.tres"),
        json!("res://a\\b.tres"),
        json!("user://save.tres"),
        json!("/tmp/x.tres"),
        json!("res://scene.tscn::GDScript_abc"),
        json!("res://line\nbreak.tres"),
        json!(1),
        json!(null),
    ] {
        let message = reject(json!({"$ref": bad}));
        assert!(message.contains("resource reference"), "{message}");
    }
    reject(json!({"$ref": "res://x.tres", "extra": 1}));
    assert_eq!(
        decode(json!({"$ref": "res://x.tres"})).variant_type(),
        Some(VariantType::Object)
    );
}

#[test]
fn resource_requires_exactly_one_of_class_or_script() {
    let class = decode(json!({"$resource": {"class": "Resource", "properties": {
        "resource_name": "test", "value": 1.5
    }}}));
    assert_eq!(
        class,
        VariantJson::Resource(ResourceSpec {
            target: ResourceTarget::Class("Resource".into()),
            properties: BTreeMap::from([
                ("resource_name".into(), VariantJson::String("test".into())),
                ("value".into(), VariantJson::Float(1.5)),
            ]),
        })
    );
    assert_eq!(class.variant_type(), Some(VariantType::Object));
    assert_canonical(
        json!({"$resource": {"script": "res://item.gd", "properties": {
            "nested": {"$resource": {"class": "Resource", "properties": {}}},
            "size": tagged("float", json!(2.0))
        }}}),
    );
    // Properties are optional on input and always written on output.
    assert_eq!(
        decode(json!({"$resource": {"class": "Resource"}})).to_json(),
        json!({"$resource": {"class": "Resource", "properties": {}}})
    );
    for bad in [
        json!({}),
        json!({"class": "Resource", "script": "res://item.gd"}),
        json!({"class": ""}),
        json!({"class": 1}),
        json!({"script": "res://scene.tscn::GDScript_abc"}),
        json!({"script": "res://.godot/x.gd"}),
        json!({"class": "Resource", "properties": []}),
        json!({"class": "Resource", "extra": 1}),
        json!([]),
    ] {
        reject(json!({"$resource": bad}));
    }
    let message = reject(json!({"$resource": {}}));
    assert!(
        message.contains("exactly one of class or script"),
        "{message}"
    );
    reject(json!({"$resource": {"class": "Resource"}, "x": 1}));
}

#[test]
fn tags_reject_unknown_types_and_fields() {
    for ty in [
        "Aabb",
        "aabb",
        "Rid",
        "Int",
        "Bool",
        "bool",
        "Nil",
        "String",
        "RID",
        "Object",
        "Callable",
        "Signal",
        "FutureType",
    ] {
        reject(tagged(ty, json!(null)));
    }
    reject(json!({"$variant": {"value": 1}}));
    reject(json!({"$variant": {"type": "StringName"}}));
    reject(json!({"$variant": {"type": "int", "value": "1", "element": "int"}}));
    reject(json!({"$variant": {"type": "int", "value": "1", "extra": 1}}));
    reject(json!({"$variant": {"type": "int", "value": "1"}, "x": 1}));
    reject(json!({"$variant": "int"}));
    reject(tagged("StringName", json!(1)));
    reject(tagged("NodePath", json!(["a"])));
    let message = reject(json!({"a": [0, {"b": tagged("Vector2", json!([1]))}]}));
    assert!(message.contains("/a/1/b/$variant/value"), "{message}");
}

#[test]
fn depth_and_entry_limits_are_enforced() {
    let limits = Limits::default();
    assert_eq!((limits.max_depth, limits.max_entries), (32, 100_000));
    let nested = |levels: usize| (0..levels).fold(json!(0), |inner, _| json!([inner]));
    // Root at depth 0; 32 levels of nesting put the innermost value at depth 32.
    assert!(VariantJson::from_json(&nested(32), &limits).is_ok());
    let message = VariantJson::from_json(&nested(33), &limits)
        .unwrap_err()
        .to_string();
    assert!(message.contains("depth 32"), "{message}");
    // Tuple components count as one level below their tag.
    let deep_vector = (0..32).fold(tagged("Vector2", json!([1.0, 2.0])), |inner, _| {
        json!([inner])
    });
    assert!(VariantJson::from_json(&deep_vector, &limits).is_err());

    // Entries count every visited value, including the root and components.
    let small = Limits {
        max_depth: 32,
        max_entries: 4,
    };
    assert!(VariantJson::from_json(&json!([1, 2, 3]), &small).is_ok());
    assert!(VariantJson::from_json(&json!([1, 2, 3, 4]), &small).is_err());
    assert!(VariantJson::from_json(&tagged("Vector3", json!([1, 2, 3])), &small).is_ok());
    assert!(VariantJson::from_json(&tagged("Vector4", json!([1, 2, 3, 4])), &small).is_err());
    assert!(VariantJson::from_json(&json!({"a": 1, "b": 2, "c": 3}), &small).is_ok());
    // Tagged dictionary keys are values too.
    assert!(VariantJson::from_json(&tagged("Dictionary", json!([[1, 2]])), &small).is_ok());
    assert!(
        VariantJson::from_json(&tagged("Dictionary", json!([[1, 2], [3, 4]])), &small).is_err()
    );
    // The budget spans the whole value, not each container.
    let wide = json!(vec![vec![0; 400]; 400]);
    let message = VariantJson::from_json(&wide, &limits)
        .unwrap_err()
        .to_string();
    assert!(message.contains("100000 entries"), "{message}");
    let packed = tagged("PackedByteArray", json!(vec![0; 100_000]));
    assert!(VariantJson::from_json(&packed, &limits).is_err());
    let packed = tagged("PackedByteArray", json!(vec![0; 99_999]));
    assert!(VariantJson::from_json(&packed, &limits).is_ok());
}

#[test]
fn to_json_then_from_json_is_identity_for_every_variant_type() {
    use VariantType as T;
    let string = |text: &str| VariantJson::String(text.into());
    let samples = [
        VariantJson::Nil,
        VariantJson::Bool(true),
        VariantJson::Int(-7),
        VariantJson::Int(i64::MAX),
        VariantJson::Int(i64::MIN),
        VariantJson::Float(1.25),
        VariantJson::Float(-3.0),
        VariantJson::Float(1e300),
        VariantJson::Float(f64::MIN_POSITIVE),
        VariantJson::Float(-0.0),
        VariantJson::Float(f64::NAN),
        VariantJson::Float(f64::NEG_INFINITY),
        string("line\nquote\"\u{0}\u{7f}"),
        VariantJson::Tagged {
            type_name: T::StringName,
            value: Box::new(string("name")),
        },
        VariantJson::Tagged {
            type_name: T::NodePath,
            value: Box::new(string("child:property")),
        },
        tuple(T::Vector2, floats(&[1.0, 2.5])),
        tuple(T::Vector2i, ints(&[-1, 2])),
        tuple(T::Rect2, floats(&[1.0, 2.0, 3.0, 4.0])),
        tuple(T::Rect2i, ints(&[1, 2, 3, 4])),
        tuple(T::Vector3, floats(&[1.0, f64::NAN, 3.0])),
        tuple(T::Vector3i, ints(&[1, 2, 3])),
        tuple(T::Transform2D, floats(&[1.0, 0.0, 0.0, 1.0, 5.0, 6.0])),
        tuple(T::Vector4, floats(&[1.0, 2.0, 3.0, 4.0])),
        tuple(T::Vector4i, ints(&[1, 2, 3, 4])),
        tuple(T::Plane, floats(&[0.0, 1.0, 0.0, 2.0])),
        tuple(T::Quaternion, floats(&[0.0, 0.0, 0.0, 1.0])),
        tuple(T::Aabb, floats(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0])),
        tuple(
            T::Basis,
            floats(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, -0.0]),
        ),
        tuple(T::Transform3D, floats(&[1.0; 12])),
        tuple(T::Projection, floats(&[0.5; 16])),
        tuple(T::Color, floats(&[0.25, 0.5, 0.75, 1.0])),
        VariantJson::Dictionary(vec![
            (VariantJson::Int(1), string("int")),
            (VariantJson::Float(1.0), string("float")),
        ]),
        VariantJson::Dictionary(vec![(string("key"), VariantJson::Array(vec![]))]),
        VariantJson::Array(vec![VariantJson::Float(2.0), VariantJson::Int(2)]),
        VariantJson::TypedArray {
            element: T::Float,
            items: floats(&[1.0, 2.5]),
        },
        tuple(T::PackedByteArray, ints(&[0, 255])),
        tuple(T::PackedInt32Array, ints(&[-1, 2147483647])),
        tuple(T::PackedInt64Array, ints(&[i64::MIN, 0])),
        tuple(T::PackedFloat32Array, floats(&[0.25, 1.0])),
        tuple(T::PackedFloat64Array, floats(&[f64::INFINITY, -0.0])),
        tuple(T::PackedStringArray, vec![string("a"), string("b")]),
        tuple(
            T::PackedVector2Array,
            vec![tuple(T::Vector2, floats(&[1.0, 1.0]))],
        ),
        tuple(
            T::PackedVector3Array,
            vec![tuple(T::Vector3, floats(&[1.0, 1.0, 1.0]))],
        ),
        tuple(
            T::PackedColorArray,
            vec![tuple(T::Color, floats(&[1.0, 0.0, 0.0, 1.0]))],
        ),
        tuple(
            T::PackedVector4Array,
            vec![tuple(T::Vector4, floats(&[1.0; 4]))],
        ),
        VariantJson::Ref(ResPath::parse("res://probe.tres").unwrap()),
        VariantJson::Resource(ResourceSpec {
            target: ResourceTarget::Script(ResPath::parse("res://item.gd").unwrap()),
            properties: BTreeMap::from([("answer".into(), VariantJson::Int(42))]),
        }),
    ];
    let mut covered = std::collections::HashSet::new();
    for sample in &samples {
        let json = sample.to_json();
        let decoded = VariantJson::from_json(&json, &Limits::default())
            .unwrap_or_else(|e| panic!("{json}: {e}"));
        // NaN != NaN, so compare canonical encodings, which also pin -0.0.
        assert_eq!(decoded.to_json(), json, "{sample:?}");
        assert_eq!(decoded.variant_type(), sample.variant_type());
        covered.insert(sample.variant_type().unwrap());
    }
    let transportable = VariantType::ALL
        .into_iter()
        .filter(|ty| !matches!(ty, T::Rid | T::Callable | T::Signal))
        .count();
    assert_eq!(covered.len(), transportable);
}

#[test]
fn non_finite_floats_are_accepted_only_in_tagged_string_form() {
    let special = |text: &str| decode(tagged("float", json!(text)));
    assert!(matches!(special("nan"), VariantJson::Float(value) if value.is_nan()));
    assert_eq!(special("inf"), VariantJson::Float(f64::INFINITY));
    assert_eq!(special("-inf"), VariantJson::Float(f64::NEG_INFINITY));
    assert_eq!(
        VariantJson::Float(f64::NAN).to_json(),
        tagged("float", json!("nan"))
    );
    assert_eq!(
        VariantJson::Float(f64::INFINITY).to_json(),
        tagged("float", json!("inf"))
    );
    for bad in [
        "NaN",
        "Infinity",
        "-Infinity",
        "+inf",
        "INF",
        "0",
        "1.5",
        "-0",
        "",
    ] {
        reject(tagged("float", json!(bad)));
    }
    reject(tagged("float", json!(null)));
    reject(tagged("float", json!([1.0])));
    // Bare NaN/Infinity are not JSON, and out-of-range literals do not parse.
    for text in ["NaN", "Infinity", "-Infinity", "1e400", "[1e400]"] {
        assert!(serde_json::from_str::<Value>(text).is_err(), "{text}");
    }
    // Strings are strings, never floats.
    assert_eq!(decode(json!("nan")), VariantJson::String("nan".into()));
    assert!(matches!(
        decode(tagged("Vector2", json!([tagged("float", json!("nan")), 1]))),
        VariantJson::Tagged { .. }
    ));
    reject(tagged("Vector2", json!(["nan", 1])));
}

#[test]
fn variant_type_names_and_codes_match_godot() {
    // Godot 4.7 `type_string(code)` for every code below TYPE_MAX.
    let godot = [
        "Nil",
        "bool",
        "int",
        "float",
        "String",
        "Vector2",
        "Vector2i",
        "Rect2",
        "Rect2i",
        "Vector3",
        "Vector3i",
        "Transform2D",
        "Vector4",
        "Vector4i",
        "Plane",
        "Quaternion",
        "AABB",
        "Basis",
        "Transform3D",
        "Projection",
        "Color",
        "StringName",
        "NodePath",
        "RID",
        "Object",
        "Callable",
        "Signal",
        "Dictionary",
        "Array",
        "PackedByteArray",
        "PackedInt32Array",
        "PackedInt64Array",
        "PackedFloat32Array",
        "PackedFloat64Array",
        "PackedStringArray",
        "PackedVector2Array",
        "PackedVector3Array",
        "PackedColorArray",
        "PackedVector4Array",
    ];
    assert_eq!(VariantType::ALL.len(), godot.len());
    for (code, name) in godot.into_iter().enumerate() {
        let ty = VariantType::from_name(name).unwrap_or_else(|| panic!("{name}"));
        assert_eq!(ty.name(), name);
        assert_eq!(ty.code() as usize, code, "{name}");
        assert_eq!(VariantType::from_code(code as u32), Some(ty));
        assert_eq!(ty.to_string(), name);
        assert_eq!(serde_json::to_value(ty).unwrap(), json!(name));
        assert_eq!(
            serde_json::from_value::<VariantType>(json!(name)).unwrap(),
            ty
        );
    }
    assert_eq!(VariantType::from_code(39), None);
    for wrong in ["Aabb", "Rid", "Bool", "Int", "vector2", ""] {
        assert_eq!(VariantType::from_name(wrong), None, "{wrong}");
        assert!(serde_json::from_value::<VariantType>(json!(wrong)).is_err());
    }
}
