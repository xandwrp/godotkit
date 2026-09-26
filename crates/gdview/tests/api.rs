// Acceptance tests for gdview::api. Offline unless prefixed real_engine_.
//
// Fixtures under tests/fixtures/api are trimmed from Godot 4.7.2 by
// `real_engine_refresh_api_fixtures` (a few classes, first-line descriptions);
// rerun it after an engine upgrade.

use std::path::{Path, PathBuf};

use gdview::api::answer::{self, Answer};
use gdview::api::{
    ApiClass, ApiIndex, ApiMethod, ApiType, Global, Member, MemberKind, ScriptOrigin, SearchHit,
    bbcode::doc_text, doc_xml,
};
use gdview::variant::VariantType;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/api")
}

fn dump_text() -> String {
    std::fs::read_to_string(fixtures().join("extension_api.json")).unwrap()
}

fn doctool() -> Vec<doc_xml::DocClass> {
    ["@GDScript.xml", "Node.xml", "CharacterBody3D.xml"]
        .iter()
        .map(|name| {
            let text = std::fs::read_to_string(fixtures().join("doctool").join(name)).unwrap();
            doc_xml::parse_class(&text).unwrap()
        })
        .collect()
}

fn index() -> ApiIndex {
    let mut index = ApiIndex::from_extension_api_json(&dump_text()).unwrap();
    index.merge_doctool(doctool());
    index
}

#[test]
fn parses_a_real_extension_api_json_fixture() {
    let index = ApiIndex::from_extension_api_json(&dump_text()).unwrap();
    assert_eq!(index.schema_version, gdview::api::API_INDEX_SCHEMA_VERSION);
    assert_eq!(index.engine_version, "4.7.2.stable.arch_linux");
    assert!(index.has_docs);
    let body = &index.classes["CharacterBody3D"];
    assert_eq!(body.parent.as_deref(), Some("PhysicsBody3D"));
    assert!(body.instantiable && !body.is_refcounted);
    assert_eq!(body.api_type, "core");
    let slide = body
        .methods
        .iter()
        .find(|m| m.name == "move_and_slide")
        .unwrap();
    assert_eq!(slide.return_type, ApiType::parse("bool"));
    assert!(slide.arguments.is_empty());

    let vector3 = &index.builtin_classes["Vector3"];
    assert!(vector3.is_builtin);
    assert!(vector3.constructors.iter().any(|c| c.arguments.len() == 3));
    let zero = vector3.constants.iter().find(|c| c.name == "ZERO").unwrap();
    assert_eq!(zero.value, "Vector3(0, 0, 0)");
    assert_eq!(zero.type_, Some(ApiType::parse("Vector3")));
    assert!(vector3.operators.iter().any(|o| o.operator == "unary-"));
    assert!(vector3.properties.iter().any(|p| p.name == "x"));

    let lerp = index.utility_function("lerp").unwrap();
    assert_eq!(lerp.arguments.len(), 3);
    assert!(index.utility_function("str").unwrap().is_vararg);
    assert!(index.global_enum("Side").is_some());
    assert!(
        index
            .global_constants
            .iter()
            .any(|c| c.name == "UINT8_MAX" && c.value == "255")
    );
    assert_eq!(index.singleton_for("Input").unwrap().name, "Input");

    let node = &index.classes["Node"];
    let ready = node.methods.iter().find(|m| m.name == "_ready").unwrap();
    assert!(ready.is_virtual && !ready.is_required);
    let threads = node
        .enums
        .iter()
        .find(|e| e.name == "ProcessThreadMessages")
        .unwrap();
    assert!(threads.is_bitfield);
    // Before the doctool merge there are no defaults and no @GDScript.
    assert!(node.properties.iter().all(|p| p.default.is_none()));
    assert!(index.gdscript.functions.is_empty());
}

#[test]
fn type_spellings_parse_and_display_as_gdscript() {
    let cases = [
        ("int", "int"),
        ("void", "void"),
        ("Variant", "Variant"),
        ("Object", "Object"),
        ("AABB", "AABB"),
        ("Node", "Node"),
        ("enum::Node.ProcessMode", "Node.ProcessMode"),
        ("bitfield::Control.SizeFlags", "Control.SizeFlags"),
        ("enum::Key", "Key"),
        ("typedarray::Node", "Array[Node]"),
        ("typedarray::24/17:Font", "Array[Font]"),
        ("typedarray::27/0:", "Array[Dictionary]"),
        ("typeddictionary::int;String", "Dictionary[int, String]"),
        (
            "BaseMaterial3D,ShaderMaterial",
            "BaseMaterial3D | ShaderMaterial",
        ),
        (
            "Texture2D,-AnimatedTexture,-AtlasTexture",
            "Texture2D (except AnimatedTexture, AtlasTexture)",
        ),
        ("const uint8_t **", "const uint8_t **"),
    ];
    for (spelling, shown) in cases {
        assert_eq!(ApiType::parse(spelling).display(), shown, "{spelling}");
    }
    assert_eq!(
        ApiType::parse("int"),
        ApiType::Builtin {
            name: VariantType::Int
        }
    );
    assert_eq!(
        ApiType::parse("Object"),
        ApiType::Class {
            name: "Object".into()
        }
    );
    assert_eq!(
        ApiType::parse("bitfield::Control.SizeFlags"),
        ApiType::Enum {
            name: "Control.SizeFlags".into(),
            bitfield: true
        }
    );
    let doc = [
        ("Node[]", None, "Array[Node]"),
        ("Array[Node]", None, "Array[Node]"),
        (
            "Dictionary[int, Array[String]]",
            None,
            "Dictionary[int, Array[String]]",
        ),
        ("int", Some("Node.ProcessMode"), "Node.ProcessMode"),
        (
            "int[]",
            Some("Node.ProcessMode[]"),
            "Array[Node.ProcessMode]",
        ),
    ];
    for (spelling, enum_name, shown) in doc {
        assert_eq!(
            ApiType::parse_doc(spelling, enum_name, false).display(),
            shown,
            "{spelling}"
        );
    }
}

#[test]
fn lineage_walks_parent_chain_and_stops_at_missing_parent_or_cycle() {
    let index = index();
    let names: Vec<_> = index
        .lineage("CharacterBody3D")
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    // Node's parent Object is in the fixture; CollisionObject3D's chain is complete.
    assert_eq!(
        names,
        [
            "CharacterBody3D",
            "PhysicsBody3D",
            "CollisionObject3D",
            "Node3D",
            "Node",
            "Object"
        ]
    );
    assert!(index.lineage("Nope").is_empty());

    let mut cyclic = index.clone();
    cyclic.classes.get_mut("Object").unwrap().parent = Some("Node".into());
    assert_eq!(cyclic.lineage("Node").len(), 2);
    cyclic.classes.get_mut("Object").unwrap().parent = Some("Missing".into());
    assert_eq!(cyclic.lineage("Node").len(), 2);
}

#[test]
fn lookup_member_finds_inherited_members_and_reports_declaring_class() {
    let index = index();
    let hit = index
        .lookup_member("CharacterBody3D", "move_and_slide")
        .unwrap();
    assert_eq!(hit.declaring_class.name, "CharacterBody3D");
    assert_eq!(hit.member.kind(), MemberKind::Method);

    let hit = index
        .lookup_member("CharacterBody3D", "queue_free")
        .unwrap();
    assert_eq!(hit.declaring_class.name, "Node");
    let hit = index.lookup_member("CharacterBody3D", "position").unwrap();
    assert_eq!(hit.declaring_class.name, "Node3D");
    assert_eq!(hit.member.kind(), MemberKind::Property);
    let hit = index.lookup_member("CharacterBody3D", "ready").unwrap();
    assert_eq!(hit.member.kind(), MemberKind::Signal);
    let hit = index
        .lookup_member("CharacterBody3D", "ProcessMode")
        .unwrap();
    assert_eq!(hit.member.kind(), MemberKind::Enum);
    let hit = index
        .lookup_member("CharacterBody3D", "PROCESS_MODE_ALWAYS")
        .unwrap();
    let Member::EnumValue { owner, value } = hit.member else {
        panic!("{hit:?}")
    };
    assert_eq!((owner.name.as_str(), value.value), ("ProcessMode", 3));
    let hit = index.lookup_member("Node", "NOTIFICATION_READY").unwrap();
    assert_eq!(hit.member.kind(), MemberKind::Constant);
    assert!(
        index
            .lookup_member("CharacterBody3D", "move_and_slid")
            .is_none()
    );
    assert!(index.lookup_member("Nope", "queue_free").is_none());
}

#[test]
fn lookup_covers_builtin_classes_utility_functions_and_global_enums() {
    let index = index();
    let split = index.lookup_member("String", "split").unwrap();
    assert!(split.declaring_class.is_builtin);
    assert!(index.lookup_member("Vector3", "ZERO").is_some());

    assert!(matches!(index.global("lerp"), Some(Global::Utility(_))));
    assert!(matches!(
        index.global("range"),
        Some(Global::GdscriptFunction(_))
    ));
    assert!(matches!(
        index.global("@export"),
        Some(Global::Annotation(_))
    ));
    assert!(matches!(
        index.global("export"),
        Some(Global::Annotation(_))
    ));
    assert!(matches!(index.global("Side"), Some(Global::Enum(_))));
    assert!(matches!(
        index.global("Variant.Type"),
        Some(Global::Enum(_))
    ));
    assert!(matches!(
        index.global("SIDE_LEFT"),
        Some(Global::EnumValue { .. })
    ));
    assert!(matches!(
        index.global("UINT8_MAX"),
        Some(Global::Constant(_))
    ));
    assert!(matches!(
        index.global("PI"),
        Some(Global::GdscriptConstant(_))
    ));
    assert!(index.global("nope").is_none());

    assert_eq!(
        index.enum_named("Node.ProcessMode").unwrap().values.len(),
        5
    );
    assert!(index.enum_named("Variant.Type").is_some());
    assert!(index.enum_named("Side").is_some());
}

#[test]
fn doctool_merge_adds_gdscript_builtins_and_property_defaults() {
    let index = index();
    let range = index
        .gdscript
        .functions
        .iter()
        .find(|f| f.name == "range")
        .unwrap();
    assert!(range.is_vararg);
    assert_eq!(range.description, None);
    assert!(index.gdscript.functions.iter().any(|f| f.name == "preload"));
    assert!(
        index
            .gdscript
            .annotations
            .iter()
            .any(|a| a.name == "@export_range")
    );
    assert!(index.gdscript.constants.iter().any(|c| c.name == "TAU"));

    let velocity = index.classes["CharacterBody3D"]
        .properties
        .iter()
        .find(|p| p.name == "velocity")
        .unwrap();
    assert_eq!(velocity.default.as_deref(), Some("Vector3(0, 0, 0)"));
    assert!(
        velocity.description.is_some(),
        "the dump's description survives"
    );
    let mode = index.classes["Node"]
        .properties
        .iter()
        .find(|p| p.name == "process_mode")
        .unwrap();
    assert_eq!(mode.default.as_deref(), Some("0"));
    assert_eq!(
        mode.type_.display(),
        "Node.ProcessMode",
        "the XML names the enum"
    );
    assert_eq!(
        index.default_display(&mode.type_, "0"),
        "Node.PROCESS_MODE_INHERIT"
    );
}

#[test]
fn search_matches_class_and_member_names_case_insensitively_and_ranks_prefix_first() {
    let index = index();
    let hits = index.search("characterbody", 5);
    assert!(matches!(hits[0], SearchHit::Class(class) if class.name == "CharacterBody3D"));

    // Underscores are ignored: the enum, the property, and the enum values all match.
    let hits = index.search("process_mode", 50);
    let names: Vec<_> = hits.iter().map(|hit| hit.name()).collect();
    assert_eq!(&names[..2], ["process_mode", "ProcessMode"]);
    assert!(names.contains(&"PROCESS_MODE_ALWAYS"));

    let hits = index.search("SLIDE", 10);
    assert!(
        hits.iter()
            .any(|hit| hit.name() == "move_and_slide" && hit.owner() == Some("CharacterBody3D"))
    );
    // Prefix matches come before substring matches.
    let hits = index.search("queue", 10);
    let first_substring = hits
        .iter()
        .position(|hit| !hit.name().to_lowercase().starts_with("queue"));
    let last_prefix = hits
        .iter()
        .rposition(|hit| hit.name().to_lowercase().starts_with("queue"));
    if let (Some(substring), Some(prefix)) = (first_substring, last_prefix) {
        assert!(prefix < substring);
    }
    assert_eq!(index.search("queue", 2).len(), 2);
    assert!(index.search("", 10).is_empty());
    assert!(index.search("zzzzqqq", 10).is_empty());
    assert_eq!(index.search("export_range", 1)[0].name(), "@export_range");
    // Deterministic.
    assert_eq!(index.search("node", 30), index.search("node", 30));
}

#[test]
fn format_signature_renders_static_vararg_defaults_and_return_type() {
    let index = index();
    let node = &index.classes["Node"];
    let add_child = node.methods.iter().find(|m| m.name == "add_child").unwrap();
    assert_eq!(
        index.signature(Some("Node"), add_child),
        "Node.add_child(node: Node, force_readable_name: bool = false, internal: Node.InternalMode = Node.INTERNAL_MODE_DISABLED) -> void"
    );
    let get_child = node.methods.iter().find(|m| m.name == "get_child").unwrap();
    assert_eq!(
        index.signature(None, get_child),
        "get_child(idx: int, include_internal: bool = false) -> Node const"
    );
    let ready = node.methods.iter().find(|m| m.name == "_ready").unwrap();
    assert_eq!(index.signature(None, ready), "virtual _ready() -> void");
    let str_ = index.utility_function("str").unwrap();
    assert_eq!(
        index.signature(None, str_),
        "str(arg1: Variant, ...) -> String"
    );
    let range = index
        .gdscript
        .functions
        .iter()
        .find(|f| f.name == "range")
        .unwrap();
    assert_eq!(index.signature(None, range), "range(...) -> Array");
    let export_range = index
        .gdscript
        .annotations
        .iter()
        .find(|a| a.name == "@export_range")
        .unwrap();
    assert!(
        index
            .signature(None, export_range)
            .starts_with("@export_range(min: float, max: float")
    );
    assert!(!index.signature(None, export_range).contains("->"));
    let vector3 = &index.builtin_classes["Vector3"];
    let from_components = vector3
        .constructors
        .iter()
        .find(|c| c.arguments.len() == 3)
        .unwrap();
    assert_eq!(
        index.signature(None, from_components),
        "Vector3(x: float, y: float, z: float) -> Vector3"
    );
    let static_method = index.builtin_classes["String"]
        .methods
        .iter()
        .find(|m| m.name == "num")
        .unwrap();
    assert!(
        index
            .signature(Some("String"), static_method)
            .starts_with("static String.num(")
    );
}

#[test]
fn suggest_names_returns_close_matches_within_edit_distance() {
    let index = index();
    assert_eq!(
        index.suggest(Some("CharacterBody3D"), "move_and_slid", 3)[0],
        "move_and_slide"
    );
    // Inherited members are candidates too.
    assert!(
        index
            .suggest(Some("CharacterBody3D"), "queue_fre", 3)
            .contains(&"queue_free".to_owned())
    );
    // Words of the query, in any class member name.
    assert!(
        index
            .suggest(Some("CharacterBody3D"), "move_slide", 5)
            .contains(&"move_and_slide".to_owned())
    );
    assert_eq!(
        index.suggest(None, "CharacterBody", 3)[0],
        "CharacterBody3D"
    );
    assert_eq!(index.suggest(None, "lerpp", 3)[0], "lerp");
    assert!(
        index
            .suggest(None, "Vectr3", 3)
            .contains(&"Vector3".to_owned())
    );
    assert!(index.suggest(None, "qqqqqqqqq", 3).is_empty());
    assert!(index.suggest(Some("Node"), "zzzzzzzz", 3).is_empty());
}

#[test]
fn descriptions_are_present_when_the_dump_had_docs_and_absent_otherwise() {
    let index = index();
    let node = &index.classes["Node"];
    assert!(node.brief.is_some() && node.description.is_some());
    let slide = index.classes["CharacterBody3D"]
        .member("move_and_slide")
        .unwrap();
    let rendered = doc_text(slide.description().unwrap(), Some("CharacterBody3D"));
    assert!(!rendered.text.contains("[method"), "{}", rendered.text);
    assert!(
        rendered
            .see_also
            .contains(&"CharacterBody3D.velocity".to_owned())
    );

    let mut raw: serde_json::Value = serde_json::from_str(&dump_text()).unwrap();
    strip_descriptions(&mut raw);
    let bare = ApiIndex::from_extension_api_json(&raw.to_string()).unwrap();
    assert!(!bare.has_docs);
    assert!(bare.classes["Node"].brief.is_none());
    assert!(
        bare.classes["Node"]
            .methods
            .iter()
            .all(|m| m.description.is_none())
    );
}

#[test]
fn script_classes_layer_over_native_ones_and_keys_normalize() {
    assert_eq!(gdview::api::script_key("Player"), "Player");
    assert_eq!(
        gdview::api::script_key("\"tools/retarget.gd\""),
        "res://tools/retarget.gd"
    );
    assert_eq!(
        gdview::api::script_key("\"tools/retarget.gd\"._Rig"),
        "res://tools/retarget.gd._Rig"
    );
    assert_eq!(gdview::api::script_key("Outer.Inner"), "Outer.Inner");

    let mut index = index();
    let player = ApiClass {
        name: "Player".into(),
        parent: Some("CharacterBody3D".into()),
        methods: vec![ApiMethod {
            name: "take_damage".into(),
            ..ApiMethod::default()
        }],
        script: Some(ScriptOrigin {
            path: "res://player.gd".into(),
            line: Some(2),
            from_engine: true,
        }),
        ..ApiClass::default()
    };
    let impostor = ApiClass {
        name: "Node".into(),
        ..ApiClass::default()
    };
    let shadowed = index.add_scripts(vec![player, impostor]);
    assert_eq!(shadowed, ["Node"]);
    assert_eq!(index.classes["Player"].api_type, "script");
    assert!(index.classes["Node"].script.is_none());
    let hit = index.lookup_member("Player", "move_and_slide").unwrap();
    assert_eq!(hit.declaring_class.name, "CharacterBody3D");
    let hit = index.lookup_member("Player", "take_damage").unwrap();
    assert_eq!(hit.declaring_class.name, "Player");
    assert_eq!(index.lineage("Player").len(), 7);
    assert!(
        index
            .search("damage", 5)
            .iter()
            .any(|hit| hit.name() == "take_damage")
    );
    assert_eq!(index.suggest(None, "Playr", 3), ["Player"]);
}

#[test]
fn answers_classes_members_globals_and_misses() {
    let index = index();
    let Answer::Class(class) = answer::lookup(&index, "CharacterBody3D") else {
        panic!()
    };
    assert_eq!(class.inherits[..2], ["PhysicsBody3D", "CollisionObject3D"]);
    assert_eq!(
        class.brief.as_deref(),
        Some("A 3D physics body specialized for characters moved by script.")
    );
    let slide = class
        .methods
        .iter()
        .find(|m| m.name == "move_and_slide")
        .unwrap();
    assert_eq!(slide.signature, "move_and_slide() -> bool");
    assert!(
        slide
            .brief
            .as_deref()
            .unwrap()
            .starts_with("Moves the body based on `velocity`.")
    );
    let velocity = class
        .properties
        .iter()
        .find(|p| p.name == "velocity")
        .unwrap();
    assert_eq!(velocity.signature, "velocity: Vector3 = Vector3(0, 0, 0)");
    assert!(class.enums.iter().any(|e| {
        e.signature
            .starts_with("enum MotionMode { MOTION_MODE_GROUNDED = 0")
    }));

    let Answer::Member(member) = answer::lookup_member(&index, "CharacterBody3D", "queue_free")
    else {
        panic!()
    };
    assert_eq!(member.member_kind, "method");
    assert_eq!(member.declaring_class.as_deref(), Some("Node"));
    assert_eq!(member.signature, "Node.queue_free() -> void");
    assert_eq!(member.return_type.as_deref(), Some("void"));
    assert!(member.description.is_some());

    let Answer::Member(member) = answer::lookup_member(&index, "Node", "process_mode") else {
        panic!()
    };
    assert_eq!(member.member_kind, "property");
    assert_eq!(
        member.signature,
        "Node.process_mode: Node.ProcessMode = Node.PROCESS_MODE_INHERIT"
    );
    assert_eq!(member.type_.as_deref(), Some("Node.ProcessMode"));

    let Answer::Member(member) = answer::lookup_member(&index, "Node", "ProcessMode") else {
        panic!()
    };
    assert_eq!(
        member.values.as_ref().unwrap()[0].signature,
        "PROCESS_MODE_INHERIT = 0"
    );
    let Answer::Member(member) = answer::lookup_member(&index, "Node", "ready") else {
        panic!()
    };
    assert_eq!(member.signature, "Node.signal ready()");

    let Answer::Member(lerp) = answer::lookup(&index, "lerp") else {
        panic!()
    };
    assert_eq!(lerp.member_kind, "utility_function");
    assert_eq!(lerp.arguments.as_ref().unwrap().len(), 3);
    let Answer::Member(range) = answer::lookup(&index, "range") else {
        panic!()
    };
    assert_eq!(
        (range.member_kind, range.description.as_deref()),
        ("gdscript_function", None)
    );
    let Answer::Member(export) = answer::lookup(&index, "@export_range") else {
        panic!()
    };
    assert_eq!(export.member_kind, "annotation");
    assert_eq!(export.return_type, None);

    let Answer::Miss(miss) = answer::lookup_member(&index, "CharacterBody3D", "move_and_slid")
    else {
        panic!()
    };
    assert_eq!(
        (miss.missing, miss.class.as_deref()),
        ("member", Some("CharacterBody3D"))
    );
    assert_eq!(miss.suggestions[0], "move_and_slide");
    let Answer::Miss(miss) = answer::lookup_member(&index, "CharacterBdy3D", "x") else {
        panic!()
    };
    assert_eq!(
        (miss.missing, miss.suggestions[0].as_str()),
        ("class", "CharacterBody3D")
    );
    let Answer::Miss(miss) = answer::lookup(&index, "lerpp") else {
        panic!()
    };
    assert_eq!(
        (miss.missing, miss.suggestions[0].as_str()),
        ("name", "lerp")
    );

    let json = serde_json::to_value(answer::lookup(&index, "Node")).unwrap();
    assert_eq!(json["kind"], "class");
}

#[test]
fn search_answers_carry_signatures_and_briefs() {
    let index = index();
    let Answer::Search(search) = answer::search(&index, "slide", 1) else {
        panic!()
    };
    assert!(search.truncated);
    assert_eq!(search.results.len(), 1);
    let Answer::Search(search) = answer::search(&index, "slide", 50) else {
        panic!()
    };
    let slide = search
        .results
        .iter()
        .find(|r| r.name == "move_and_slide")
        .unwrap();
    assert_eq!(slide.kind, "method");
    assert_eq!(slide.class.as_deref(), Some("CharacterBody3D"));
    assert_eq!(
        slide.signature.as_deref(),
        Some("CharacterBody3D.move_and_slide() -> bool")
    );
    let Answer::Search(search) = answer::search(&index, "characterbody3d", 10) else {
        panic!()
    };
    assert_eq!(search.results[0].kind, "class");
    assert!(!search.truncated);
}

#[test]
fn index_round_trips_through_json() {
    let index = index();
    let text = serde_json::to_string(&index).unwrap();
    let back: ApiIndex = serde_json::from_str(&text).unwrap();
    assert_eq!(back, index);
    assert!(ApiIndex::from_extension_api_json("{").is_err());
    assert!(ApiIndex::from_extension_api_json("{}").is_err());
}

fn strip_descriptions(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            map.remove("description");
            map.remove("brief_description");
            map.values_mut().for_each(strip_descriptions);
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(strip_descriptions),
        _ => {}
    }
}

/// Classes, builtins, utilities, and enums the fixture keeps from a full dump.
const KEEP_CLASSES: &[&str] = &[
    "Object",
    "Node",
    "Node3D",
    "CollisionObject3D",
    "PhysicsBody3D",
    "CharacterBody3D",
    "RefCounted",
];
const KEEP_BUILTINS: &[&str] = &["String", "Vector3"];
const KEEP_UTILITIES: &[&str] = &[
    "lerp",
    "clamp",
    "randf_range",
    "print",
    "str",
    "is_instance_valid",
];
const KEEP_ENUMS: &[&str] = &["Side", "Variant.Type"];
const KEEP_DOCTOOL: &[&str] = &["@GDScript.xml", "Node.xml", "CharacterBody3D.xml"];

#[test]
#[ignore = "requires GDKIT_TEST_GODOT; rewrites tests/fixtures/api"]
fn real_engine_refresh_api_fixtures() {
    let godot = std::env::var_os("GDKIT_TEST_GODOT").expect("set GDKIT_TEST_GODOT to opt in");
    let scratch = tempfile::tempdir().unwrap();
    for args in [
        vec!["--headless", "--dump-extension-api-with-docs"],
        vec!["--headless", "--doctool", "doc"],
    ] {
        std::fs::create_dir_all(scratch.path().join("doc")).unwrap();
        let status = std::process::Command::new(&godot)
            .args(&args)
            .current_dir(scratch.path())
            .status()
            .unwrap();
        assert!(status.success(), "{args:?}: {status}");
    }

    let text = std::fs::read_to_string(scratch.path().join("extension_api.json")).unwrap();
    let mut dump: serde_json::Value = serde_json::from_str(&text).unwrap();
    let keep = |key: &str, names: &[&str], dump: &mut serde_json::Value| {
        dump[key]
            .as_array_mut()
            .unwrap()
            .retain(|item| names.contains(&item["name"].as_str().unwrap()));
    };
    keep("classes", KEEP_CLASSES, &mut dump);
    keep("builtin_classes", KEEP_BUILTINS, &mut dump);
    keep("utility_functions", KEEP_UTILITIES, &mut dump);
    keep("global_enums", KEEP_ENUMS, &mut dump);
    keep("singletons", &["Input"], &mut dump);
    shrink(&mut dump);
    let object = dump.as_object_mut().unwrap();
    for unused in [
        "builtin_class_sizes",
        "builtin_class_member_offsets",
        "native_structures",
    ] {
        object.remove(unused);
    }
    let out = fixtures();
    std::fs::create_dir_all(out.join("doctool")).unwrap();
    std::fs::write(
        out.join("extension_api.json"),
        serde_json::to_string_pretty(&dump).unwrap() + "\n",
    )
    .unwrap();

    for name in KEEP_DOCTOOL {
        let found = find(&scratch.path().join("doc"), name)
            .unwrap_or_else(|| panic!("doctool wrote no {name}"));
        std::fs::copy(found, out.join("doctool").join(name)).unwrap();
    }
}

/// Keeps each description's first line and drops the binding hashes, which the index ignores.
fn shrink(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            map.remove("hash");
            map.remove("hash_compatibility");
            for (key, value) in map.iter_mut() {
                match value {
                    serde_json::Value::String(text) if key.ends_with("description") => {
                        let first = text.lines().next().unwrap_or_default().to_owned();
                        *text = first;
                    }
                    _ => shrink(value),
                }
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(shrink),
        _ => {}
    }
}

fn find(directory: &Path, name: &str) -> Option<PathBuf> {
    for entry in std::fs::read_dir(directory).ok()? {
        let path = entry.ok()?.path();
        if path.is_dir() {
            if let Some(found) = find(&path, name) {
                return Some(found);
            }
        } else if path.file_name().is_some_and(|file| file == name) {
            return Some(path);
        }
    }
    None
}
