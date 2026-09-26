// Acceptance tests for gdview::property. Hint strings are verbatim Godot 4.7.2
// `get_property_list()` output for the declarations named beside them.

use gdview::property::{self, EnumChoice, FieldSchema, PropertyInfo, TypeSchema};
use gdview::variant::VariantType;
use serde_json::{Value, json};

const STORED: u32 = 6; // PROPERTY_USAGE_DEFAULT: storage + editor
const SCRIPT_VARIABLE: u32 = 4102;

fn info(name: &str, code: u32, hint: u32, hint_string: &str, usage: u32) -> PropertyInfo {
    PropertyInfo {
        name: name.into(),
        type_code: code,
        class_name: String::new(),
        hint,
        hint_string: hint_string.into(),
        usage,
        default: Value::Null,
        default_error: None,
    }
}

fn field(info: PropertyInfo) -> FieldSchema {
    property::field(&info).unwrap().expect("a stored field")
}

fn ty(variant_type: VariantType) -> TypeSchema {
    TypeSchema {
        variant_type,
        class_name: None,
        enum_choices: Vec::new(),
    }
}

fn choice(name: &str, value: Value) -> EnumChoice {
    EnumChoice {
        name: name.into(),
        value,
    }
}

#[test]
fn fields_keep_storage_entries_and_drop_script_groups_and_editor_only() {
    let mut script = info("script", 24, 17, "Script", 1048590);
    script.class_name = "Script".into();
    let mut damage = info("damage", 2, 0, "", SCRIPT_VARIABLE);
    damage.default = json!(3);
    let properties = [
        info("Resource", 0, 0, "resource.cpp", 128), // category
        info("resource_local_to_scene", 1, 0, "", STORED),
        script,
        info("editor_only", 2, 0, "", 4),
        info("Group", 0, 0, "group_", 64),
        damage,
        info("_surfaces", 28, 41, "", 10),
    ];
    let fields = property::fields(&properties).unwrap();
    let names: Vec<_> = fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["resource_local_to_scene", "damage", "_surfaces"]);
    assert_eq!(fields[1].default, json!(3));
    assert_eq!(fields[1].variant_type, VariantType::Int);
    assert!(!fields[1].internal);
    assert!(fields[2].internal);
    assert_eq!(fields[2].hint.as_deref(), Some("hint_41"));
}

#[test]
fn typed_arrays_parse_script_and_native_subtype_hint_strings() {
    // @export var tags: Array[String]
    let tags = field(info("tags", 28, 23, "4:", SCRIPT_VARIABLE));
    assert_eq!(tags.element, Some(ty(VariantType::String)));
    assert_eq!(tags.key, None);

    // @export var textures: Array[Texture2D]
    let textures = field(info("textures", 28, 23, "24/17:Texture2D", SCRIPT_VARIABLE));
    let mut texture = ty(VariantType::Object);
    texture.class_name = Some("Texture2D".into());
    assert_eq!(textures.element, Some(texture));

    // Compositor.compositor_effects (native: ARRAY_TYPE)
    let effects = field(info(
        "compositor_effects",
        28,
        31,
        "24/17:CompositorEffect",
        STORED,
    ));
    assert_eq!(
        effects.element.unwrap().class_name.as_deref(),
        Some("CompositorEffect")
    );

    // @export var kinds: Array[Kind]
    let kinds = field(info(
        "kinds",
        28,
        23,
        "2/2:Melee:0,Ranged:5",
        SCRIPT_VARIABLE,
    ));
    let element = kinds.element.unwrap();
    assert_eq!(element.variant_type, VariantType::Int);
    assert_eq!(
        element.enum_choices,
        [choice("Melee", json!(0)), choice("Ranged", json!(5))]
    );

    // @export_file("*.tres") var files: Array[String]; Array[PackedInt32Array]
    let files = field(info("files", 28, 23, "4/13:*.tres", SCRIPT_VARIABLE));
    assert_eq!(files.element, Some(ty(VariantType::String)));
    let packed = field(info("a2", 28, 23, "30:", SCRIPT_VARIABLE));
    assert_eq!(packed.element, Some(ty(VariantType::PackedInt32Array)));

    // A bare class or type name, and an untyped array.
    let bare = field(info("bare", 28, 31, "Texture2D", STORED));
    assert_eq!(
        bare.element.unwrap().class_name.as_deref(),
        Some("Texture2D")
    );
    let named = field(info("named", 28, 31, "int", STORED));
    assert_eq!(named.element, Some(ty(VariantType::Int)));
    let untyped = field(info("arr", 28, 0, "", SCRIPT_VARIABLE));
    assert_eq!(untyped.element, None);
    assert_eq!(untyped.accepts, ["[value, …]"]);
}

#[test]
fn typed_dictionaries_parse_key_and_value_subtypes_with_variant_sides() {
    // @export var stats: Dictionary[String, int]
    let stats = field(info("stats", 27, 23, "4:;2:", SCRIPT_VARIABLE));
    assert_eq!(stats.key, Some(ty(VariantType::String)));
    assert_eq!(stats.value, Some(ty(VariantType::Int)));
    assert_eq!(stats.element, None);

    // Dictionary[String, Variant] and Dictionary[Variant, int]
    let d1 = field(info("d1", 27, 23, "4:;0:", SCRIPT_VARIABLE));
    assert_eq!(d1.value, Some(ty(VariantType::Nil)));
    let d2 = field(info("d2", 27, 23, "0:;2:", SCRIPT_VARIABLE));
    assert_eq!(d2.key, Some(ty(VariantType::Nil)));

    // Dictionary[StringName, WeaponDefinition]
    let d3 = field(info(
        "d3",
        27,
        23,
        "21:;24/17:WeaponDefinition",
        SCRIPT_VARIABLE,
    ));
    assert_eq!(d3.key, Some(ty(VariantType::StringName)));
    assert_eq!(
        d3.value.unwrap().class_name.as_deref(),
        Some("WeaponDefinition")
    );

    let untyped = field(info("plain", 27, 0, "", SCRIPT_VARIABLE));
    assert_eq!((untyped.key, untyped.value), (None, None));
}

#[test]
fn enum_and_flag_choices_follow_godot_implicit_values() {
    // Gradient.interpolation_mode: implicit values from 0.
    let mode = field(info(
        "interpolation_mode",
        2,
        2,
        "Linear,Constant,Cubic",
        STORED,
    ));
    assert_eq!(
        mode.enum_choices,
        [
            choice("Linear", json!(0)),
            choice("Constant", json!(1)),
            choice("Cubic", json!(2)),
        ]
    );
    assert_eq!(mode.accepts[0], "integer: one of enum_choices' values");

    // Implicit values continue from the previous explicit one.
    let mixed = field(info("mixed", 2, 2, "A, B:5, C", STORED));
    assert_eq!(
        mixed.enum_choices,
        [
            choice("A", json!(0)),
            choice("B", json!(5)),
            choice("C", json!(6)),
        ]
    );

    // StandardMaterial3D.stencil_flags: implicit flags are bits.
    let flags = field(info(
        "stencil_flags",
        2,
        6,
        "Read,Write,Write Depth Fail",
        STORED,
    ));
    assert_eq!(
        flags.enum_choices,
        [
            choice("Read", json!(1)),
            choice("Write", json!(2)),
            choice("Write Depth Fail", json!(4)),
        ]
    );
    assert_eq!(flags.accepts[0], "integer: a sum of enum_choices' values");
    let explicit = field(info("explicit", 2, 6, "a:4,b:16", STORED));
    assert_eq!(explicit.enum_choices[1], choice("b", json!(16)));

    // @export_enum("A", "B:5") var s: String reports the option text verbatim.
    let text = field(info("s", 4, 2, "A,B:5", SCRIPT_VARIABLE));
    assert_eq!(
        text.enum_choices,
        [choice("A", json!("A")), choice("B:5", json!("B:5"))]
    );
    assert_eq!(text.accepts, ["string: one of enum_choices' values"]);

    // Layer hints name no choices; their names live in project settings.
    let layers = field(info("phys", 2, 8, "", SCRIPT_VARIABLE));
    assert!(layers.enum_choices.is_empty());
    assert_eq!(layers.hint.as_deref(), Some("layers_2d_physics"));
}

#[test]
fn enum_fields_report_enum_name_not_class_name() {
    // @export var kind: Kind
    let mut kind = info("kind", 2, 2, "Melee:0,Ranged:5", 69638);
    kind.class_name = "WeaponDefinition.Kind".into();
    let kind = field(kind);
    assert_eq!(kind.enum_name.as_deref(), Some("WeaponDefinition.Kind"));
    assert_eq!(kind.class_name, None);

    // @export var icon: Texture2D, and a RESOURCE_TYPE hint without class_name.
    let mut icon = info("icon", 24, 17, "Texture2D", SCRIPT_VARIABLE);
    icon.class_name = "Texture2D".into();
    let icon = field(icon);
    assert_eq!(icon.class_name.as_deref(), Some("Texture2D"));
    assert_eq!(icon.enum_name, None);
    let hinted = field(info("next_pass", 24, 17, "Material", STORED));
    assert_eq!(hinted.class_name.as_deref(), Some("Material"));
}

#[test]
fn hints_are_named_and_unknown_codes_kept() {
    let spread = field(info("spread", 3, 1, "0.0,10.0,0.5", SCRIPT_VARIABLE));
    assert_eq!(spread.hint.as_deref(), Some("range"));
    assert_eq!(spread.hint_string.as_deref(), Some("0.0,10.0,0.5"));
    let file = field(info("f", 4, 13, "*.png", SCRIPT_VARIABLE));
    assert_eq!(file.hint.as_deref(), Some("file"));
    let plain = field(info("plain", 4, 0, "", SCRIPT_VARIABLE));
    assert_eq!((plain.hint, plain.hint_string), (None, None));
    let future = field(info("future", 4, 99, "", STORED));
    assert_eq!(future.hint.as_deref(), Some("hint_99"));
}

#[test]
fn accepts_documents_every_variant_type_and_untransportable_types_are_unsupported() {
    for variant_type in VariantType::ALL {
        let field = field(info("f", variant_type.code(), 0, "", STORED));
        match variant_type {
            VariantType::Rid | VariantType::Callable | VariantType::Signal => {
                assert!(field.accepts.is_empty(), "{variant_type}");
                assert!(
                    field
                        .unsupported
                        .as_deref()
                        .is_some_and(|why| why.starts_with(variant_type.name())),
                    "{variant_type}"
                );
            }
            _ => {
                assert!(!field.accepts.is_empty(), "{variant_type}");
                assert_eq!(field.unsupported, None, "{variant_type}");
            }
        }
    }
    let vector = field(info("offset", 9, 0, "", SCRIPT_VARIABLE));
    assert_eq!(
        vector.accepts,
        [r#"{"$variant":{"type":"Vector3","value":[3 numbers]}}"#]
    );
    let vector_i = field(info("cell", 6, 0, "", SCRIPT_VARIABLE));
    assert_eq!(
        vector_i.accepts,
        [r#"{"$variant":{"type":"Vector2i","value":[2 integers]}}"#]
    );
    let object = field(info("sub", 24, 17, "WeaponDefinition", SCRIPT_VARIABLE));
    assert_eq!(object.accepts.len(), 3);
    assert!(object.accepts[1].contains("$ref"));
    let typed = field(info("stats", 27, 23, "4:;2:", SCRIPT_VARIABLE));
    assert!(typed.accepts[1].contains("`key`"));
}

#[test]
fn unknown_type_codes_and_malformed_subtypes_are_errors() {
    for bad in [
        info("f", 39, 0, "", STORED),
        info("f", 28, 23, "x:", STORED),
        info("f", 28, 23, "4/x:", STORED),
        info("f", 28, 23, "99:", STORED),
        info("f", 27, 23, "4:", STORED),
        info("f", 27, 23, "4:;", STORED),
    ] {
        let error = property::field(&bad).unwrap_err().to_string();
        assert!(
            error.contains("Variant type code") || error.contains("malformed type hint"),
            "{bad:?}: {error}"
        );
    }
    // Unstored entries are never parsed.
    assert_eq!(property::field(&info("f", 99, 0, "", 4)).unwrap(), None);
}
