use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
struct Fixture {
    root: PathBuf,
    engine: std::ffi::OsString,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "gdkit-containers-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("project.godot"), "config_version=5\n").unwrap();
        let mut script = "extends Resource\n".to_owned();
        for (name, kind) in [
            ("bytes", "PackedByteArray"),
            ("int32s", "PackedInt32Array"),
            ("int64s", "PackedInt64Array"),
            ("float32s", "PackedFloat32Array"),
            ("float64s", "PackedFloat64Array"),
            ("strings", "PackedStringArray"),
            ("v2s", "PackedVector2Array"),
            ("v3s", "PackedVector3Array"),
            ("v4s", "PackedVector4Array"),
            ("colors", "PackedColorArray"),
            ("integers", "Array[int]"),
            ("numbers", "Array[float]"),
            ("booleans", "Array[bool]"),
            ("texts", "Array[String]"),
            ("symbols", "Array[StringName]"),
            ("paths", "Array[NodePath]"),
            ("positions", "Array[Vector3]"),
            ("by_id", "Dictionary[int, StringName]"),
            ("by_name", "Dictionary[StringName, int]"),
            ("by_position", "Dictionary[Vector3, Color]"),
            ("untyped", "Array"),
            ("untyped_dict", "Dictionary"),
            ("child", "Resource"),
        ] {
            let initializer = match name {
                "float32s" => " = PackedFloat32Array([0.1])",
                "positions" => " = [Vector3(0.1,0.2,0.3)]",
                _ => "",
            };
            script.push_str(&format!("@export var {name}: {kind}{initializer}\n"));
        }
        fs::write(root.join("containers.gd"), script).unwrap();
        fs::write(root.join("mutator.gd"),"extends Resource\n@export var integers: Array[int] = []:\n\tset(new_values):\n\t\tintegers = new_values\n\t\tif not integers.is_empty(): integers[0] = 99\n@export var mapping: Dictionary[int, int] = {}:\n\tset(new_values):\n\t\tmapping = new_values\n\t\tif mapping.has(1): mapping[1] = 99\n@export var packed: PackedInt64Array = PackedInt64Array():\n\tget:\n\t\treturn packed if resource_path.is_empty() else PackedInt64Array([99])\n").unwrap();
        Self {
            root,
            engine: std::env::var_os("GDKIT_TEST_GODOT").expect("set GDKIT_TEST_GODOT"),
        }
    }
    fn create(&self, script: &str, properties: Value, out: &str) -> (bool, Value) {
        fs::write(
            self.root.join("spec.json"),
            serde_json::to_vec(&json!({"script":script,"properties":properties})).unwrap(),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_gdkit"))
            .current_dir(&self.root)
            .env_remove("GDKIT_GODOT")
            .args([
                "resource",
                "create",
                "--spec",
                "spec.json",
                "--out",
                out,
                "--output",
                "json",
                "--godot",
            ])
            .arg(&self.engine)
            .output()
            .unwrap();
        let result = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "{error}: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (output.status.success(), result)
    }
    fn schema(&self) -> Value {
        let output = Command::new(env!("CARGO_BIN_EXE_gdkit"))
            .current_dir(&self.root)
            .env_remove("GDKIT_GODOT")
            .args([
                "resource",
                "schema",
                "--script",
                "res://containers.gd",
                "--output",
                "json",
                "--godot",
            ])
            .arg(&self.engine)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn probe(&self, body: &str) {
        fs::write(
            self.root.join("probe.gd"),
            format!("extends SceneTree\nfunc _initialize():\n{body}\n\tquit()\n"),
        )
        .unwrap();
        let output = Command::new(&self.engine)
            .args(["--headless", "--path"])
            .arg(&self.root)
            .args(["--script", "res://probe.gd"])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains("ERROR:"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
fn tag(kind: &str, value: Value) -> Value {
    json!({"$variant":{"type":kind,"value":value}})
}
fn array(kind: &str, items: Value) -> Value {
    tag("Array", json!({"element_type":kind,"items":items}))
}
fn dict(key: &str, value: &str, entries: Value) -> Value {
    tag(
        "Dictionary",
        json!({"key_type":key,"value_type":value,"entries":entries}),
    )
}
fn integer(value: &str) -> Value {
    tag("int", json!(value))
}
#[test]
#[ignore = "requires GDKIT_TEST_GODOT pointing to a Godot 4 editor"]
fn containers_round_trip_with_empty_typing_and_reusable_specs() {
    let fixture = Fixture::new();
    let properties = json!({
        "bytes":tag("PackedByteArray",json!([0,255])),"int32s":tag("PackedInt32Array",json!([-2147483648,2147483647])),
        "int64s":tag("PackedInt64Array",json!([integer("-9223372036854775808"),integer("9223372036854775807")])),
        "float32s":tag("PackedFloat32Array",json!([0.125,-2.5])),"float64s":tag("PackedFloat64Array",json!([0.1,-2.5])),
        "strings":tag("PackedStringArray",json!(["","hello"])),
        "v2s":tag("PackedVector2Array",json!([tag("Vector2",json!([1.5,2.25]))])),
        "v3s":tag("PackedVector3Array",json!([tag("Vector3",json!([1.5,2.25,3.125]))])),
        "v4s":tag("PackedVector4Array",json!([tag("Vector4",json!([1.5,2.25,3.125,4.5]))])),
        "colors":tag("PackedColorArray",json!([tag("Color",json!([2.0,-0.5,0.75,0.25]))])),
        "integers":array("int",json!([integer("-9223372036854775808"),integer("9223372036854775807")])),
        "numbers":array("float",json!([0.1,0.125])),"booleans":array("bool",json!([true,false])),"texts":array("String",json!(["","hello"])),
        "symbols":array("StringName",json!([tag("StringName",json!("hello"))])),"paths":array("NodePath",json!([tag("NodePath",json!("Root:position"))])),
        "positions":array("Vector3",json!([tag("Vector3",json!([1.5,2.25,3.125]))])),
        "by_id":dict("int","StringName",json!([[integer("9223372036854775807"),tag("StringName",json!("high"))],[-1,tag("StringName",json!("low"))]])),
        "by_name":dict("StringName","int",json!([[tag("StringName",json!("high")),integer("9223372036854775807")]])),
        "by_position":dict("Vector3","Color",json!([[tag("Vector3",json!([1.5,2.25,3.125])),tag("Color",json!([2.0,-0.5,0.75,0.25]))]]))
    });
    let (ok, result) = fixture.create(
        "res://containers.gd",
        properties.clone(),
        "res://containers.tres",
    );
    assert!(ok, "{result}");
    assert_eq!(result["properties"], properties);
    fixture.probe("\tvar graph = load(\"res://containers.tres\")\n\tassert(graph.bytes == PackedByteArray([0,255]))\n\tassert(graph.int32s == PackedInt32Array([-2147483648,2147483647]))\n\tassert(graph.int64s == PackedInt64Array([-9223372036854775808,9223372036854775807]))\n\tassert(graph.float32s == PackedFloat32Array([0.125,-2.5]))\n\tassert(graph.float64s == PackedFloat64Array([0.1,-2.5]))\n\tassert(graph.strings == PackedStringArray([\"\",\"hello\"]))\n\tassert(graph.v2s == PackedVector2Array([Vector2(1.5,2.25)]))\n\tassert(graph.v3s == PackedVector3Array([Vector3(1.5,2.25,3.125)]))\n\tassert(graph.v4s == PackedVector4Array([Vector4(1.5,2.25,3.125,4.5)]))\n\tassert(graph.colors == PackedColorArray([Color(2,-0.5,0.75,0.25)]))\n\tassert(graph.integers[0] == -9223372036854775808)\n\tassert(graph.by_id.get_typed_key_builtin() == TYPE_INT and graph.by_id.get_typed_value_builtin() == TYPE_STRING_NAME)\n\tassert(graph.by_id[9223372036854775807] == &\"high\")\n\tassert(graph.by_name[&\"high\"] == 9223372036854775807)\n\tassert(graph.by_position[Vector3(1.5,2.25,3.125)] == Color(2,-0.5,0.75,0.25))");
    let (ok, result) = fixture.create(
        "res://containers.gd",
        json!({"child":{"$resource":{"script":"res://containers.gd","properties":properties}}}),
        "res://nested.tres",
    );
    assert!(ok, "{result}");
    assert_eq!(
        result["properties"]["child"]["$resource"]["properties"],
        properties
    );
    let schema = fixture.schema();
    let fields = schema["fields"].as_array().unwrap();
    let mut empty = serde_json::Map::new();
    for (name, value) in properties.as_object().unwrap() {
        let field = fields.iter().find(|field| field["name"] == *name).unwrap();
        assert_eq!(field["create_supported"], true, "{field}");
        assert_eq!(field["accepted_inputs"], json!(["$variant"]));
        assert_eq!(field["default"]["encoding"], "tagged");
        assert_eq!(field["variant_contract"]["type"], value["$variant"]["type"]);
        if value["$variant"]["type"] == "Array" {
            assert_eq!(
                field["variant_contract"]["element_type"],
                value["$variant"]["value"]["element_type"]
            );
            assert_eq!(
                field["variant_contract"]["element_contract"]["type"],
                value["$variant"]["value"]["element_type"]
            );
        } else if value["$variant"]["type"] == "Dictionary" {
            assert_eq!(
                field["variant_contract"]["key_contract"]["type"],
                value["$variant"]["value"]["key_type"]
            );
            assert_eq!(
                field["variant_contract"]["value_contract"]["type"],
                value["$variant"]["value"]["value_type"]
            );
        } else {
            assert_eq!(field["variant_contract"]["exact_elements"], true);
            assert!(
                !field["variant_contract"]["element_contract"]["accepted_inputs"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
        }

        let mut value = value.clone();
        let payload = &mut value["$variant"]["value"];
        if payload.is_array() {
            *payload = json!([]);
        } else if payload.get("items").is_some() {
            payload["items"] = json!([]);
        } else {
            payload["entries"] = json!([]);
        }
        empty.insert(name.to_owned(), value);
    }
    for name in ["untyped", "untyped_dict"] {
        assert_eq!(
            fields.iter().find(|field| field["name"] == name).unwrap()["create_supported"],
            false
        );
    }
    let (ok, result) = fixture.create("res://containers.gd", json!(empty), "res://empty.tres");
    assert!(ok, "{result}");
    fixture.probe("\tvar graph = load(\"res://empty.tres\")\n\tassert(graph.integers.is_empty() and graph.integers.get_typed_builtin() == TYPE_INT)\n\tassert(graph.symbols.is_empty() and graph.symbols.get_typed_builtin() == TYPE_STRING_NAME)\n\tassert(graph.by_name.is_empty() and graph.by_name.get_typed_key_builtin() == TYPE_STRING_NAME and graph.by_name.get_typed_value_builtin() == TYPE_INT)\n\tassert(graph.v4s.is_empty() and typeof(graph.v4s) == TYPE_PACKED_VECTOR4_ARRAY)");
    let defaults: serde_json::Map<String, Value> = fields
        .iter()
        .filter(|field| {
            field["create_supported"] == true
                && ["json", "tagged"].contains(&field["default"]["encoding"].as_str().unwrap())
        })
        .map(|field| {
            (
                field["name"].as_str().unwrap().to_owned(),
                field["default"]["value"].clone(),
            )
        })
        .collect();
    let (ok, result) = fixture.create(
        "res://containers.gd",
        json!(defaults),
        "res://defaults.tres",
    );
    assert!(ok, "{result}");
    let (ok, result) = fixture.create(
        "res://containers.gd",
        result["properties"].clone(),
        "res://recreated.tres",
    );
    assert!(ok, "{result}");
}
#[test]
#[ignore = "requires GDKIT_TEST_GODOT pointing to a Godot 4 editor"]
fn errors_reject_loss_duplicate_keys_and_mutation_without_publication() {
    let fixture = Fixture::new();
    for (name, value, path) in [
        (
            "integers",
            array("int", json!([9007199254740993_i64])),
            "integers.$variant.value.items[0]",
        ),
        (
            "integers",
            tag(
                "Array",
                json!({"element_type":"int","items":[],"extra":true}),
            ),
            "integers.$variant.value",
        ),
        (
            "by_id",
            tag(
                "Dictionary",
                json!({"key_type":"int","value_type":"StringName"}),
            ),
            "by_id.$variant.value",
        ),
        ("untyped", array("int", json!([])), "untyped"),
        (
            "untyped_dict",
            dict("int", "int", json!([])),
            "untyped_dict",
        ),
        (
            "bytes",
            tag("PackedByteArray", json!([-1])),
            "bytes.$variant.value[0]",
        ),
        (
            "bytes",
            tag("PackedByteArray", json!([256])),
            "bytes.$variant.value[0]",
        ),
        (
            "int32s",
            tag("PackedInt32Array", json!([2147483648_i64])),
            "int32s.$variant.value[0]",
        ),
        (
            "int64s",
            tag("PackedInt64Array", json!([9007199254740993_i64])),
            "int64s.$variant.value[0]",
        ),
        (
            "float32s",
            tag("PackedFloat32Array", json!([0.1])),
            "float32s.$variant.value[0]",
        ),
        (
            "strings",
            tag("PackedStringArray", json!([1])),
            "strings.$variant.value[0]",
        ),
        (
            "v2s",
            tag("PackedVector2Array", json!([[1, 2]])),
            "v2s.$variant.value[0]",
        ),
        (
            "integers",
            array("int", json!([1.5])),
            "integers.$variant.value.items[0]",
        ),
        (
            "integers",
            array("float", json!([])),
            "integers.$variant.value",
        ),
        (
            "symbols",
            array("StringName", json!(["ordinary string"])),
            "symbols.$variant.value.items[0]",
        ),
        (
            "by_id",
            dict(
                "int",
                "StringName",
                json!([
                    [1, tag("StringName", json!("one"))],
                    [1.0, tag("StringName", json!("two"))]
                ]),
            ),
            "by_id.$variant.value.entries[1][0]",
        ),
        (
            "by_id",
            dict("int", "StringName", json!([[1]])),
            "by_id.$variant.value.entries[0]",
        ),
        (
            "by_id",
            dict("String", "StringName", json!([])),
            "by_id.$variant.value",
        ),
        (
            "by_id",
            dict("int", "StringName", json!([[1, "ordinary string"]])),
            "by_id.$variant.value.entries[0][1]",
        ),
    ] {
        let (ok, result) = fixture.create(
            "res://containers.gd",
            json!({name:value}),
            "res://failure.tres",
        );
        assert!(!ok, "{result}");
        assert_eq!(result["stage"], "validate", "{result}");
        assert_eq!(result["field"], path, "{result}");
        assert!(!fixture.root.join("failure.tres").exists());
    }
    for (properties, path, stage) in [
        (
            json!({"integers":array("int",json!([1]))}),
            "properties.integers[0]",
            "assign",
        ),
        (
            json!({"mapping":dict("int","int",json!([[1,1]]))}),
            "properties.mapping[1]",
            "assign",
        ),
        (
            json!({"packed":tag("PackedInt64Array",json!([integer("9223372036854775807")]))}),
            "properties.packed[0]",
            "verify",
        ),
    ] {
        let (ok, result) = fixture.create("res://mutator.gd", properties, "res://failure.tres");
        assert!(!ok, "{result}");
        assert_eq!(result["stage"], stage, "{result}");
        assert_eq!(result["field"], path, "{result}");
        assert!(!fixture.root.join("failure.tres").exists());
    }
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT pointing to a Godot 4 editor"]
fn scalar_containers_cover_math_types_and_non_string_keys() {
    let fixture = Fixture::new();
    let cases = [
        ("Vector2", tag("Vector2", json!([1.5, 2.25]))),
        ("Vector2i", tag("Vector2i", json!([1, 2]))),
        ("Vector3", tag("Vector3", json!([1.5, 2.25, 3.125]))),
        ("Vector3i", tag("Vector3i", json!([1, 2, 3]))),
        ("Vector4", tag("Vector4", json!([1.5, 2.25, 3.125, 4.5]))),
        ("Vector4i", tag("Vector4i", json!([1, 2, 3, 4]))),
        ("Color", tag("Color", json!([0.5, 0.25, 0.75, 1.0]))),
        ("Rect2", tag("Rect2", json!([1.5, 2.25, 3.125, 4.5]))),
        ("Rect2i", tag("Rect2i", json!([1, 2, 3, 4]))),
        (
            "Quaternion",
            tag("Quaternion", json!([1.5, 2.25, 3.125, 4.5])),
        ),
        ("Plane", tag("Plane", json!([1.5, 2.25, 3.125, 4.5]))),
        (
            "AABB",
            tag("AABB", json!([1.5, 2.25, 3.125, 4.5, 5.25, 6.125])),
        ),
        (
            "Basis",
            tag(
                "Basis",
                json!([1.5, 2.25, 3.125, 4.5, 5.25, 6.125, 7.5, 8.25, 9.125]),
            ),
        ),
        (
            "Transform2D",
            tag("Transform2D", json!([1.5, 2.25, 3.125, 4.5, 5.25, 6.125])),
        ),
        (
            "Transform3D",
            tag(
                "Transform3D",
                json!([
                    1.5, 2.25, 3.125, 4.5, 5.25, 6.125, 7.5, 8.25, 9.125, 10.5, 11.25, 12.125
                ]),
            ),
        ),
        ("int", integer("9223372036854775807")),
        ("StringName", tag("StringName", json!("key"))),
        ("NodePath", tag("NodePath", json!("Root:position"))),
        ("bool", json!(true)),
        ("float", json!(0.125)),
        ("String", json!("key")),
    ];
    let mut script = "extends Resource\n".to_owned();
    let mut properties = serde_json::Map::new();
    let mut probe = "\tvar graph = load(\"res://all_scalars.tres\")\n".to_owned();
    for (index, (kind, value)) in cases.iter().enumerate() {
        script.push_str(&format!("@export var items{index}: Array[{kind}]\n@export var mapping{index}: Dictionary[{kind}, {kind}]\n"));
        properties.insert(format!("items{index}"), array(kind, json!([value])));
        properties.insert(
            format!("mapping{index}"),
            dict(kind, kind, json!([[value, value]])),
        );
        probe.push_str(&format!("\tassert(graph.mapping{index}.has(graph.items{index}[0]))\n\tassert(graph.mapping{index}[graph.items{index}[0]] == graph.items{index}[0])\n\tassert(graph.mapping{index}.get_typed_key_builtin() == graph.items{index}.get_typed_builtin())\n\tassert(graph.mapping{index}.get_typed_value_builtin() == graph.items{index}.get_typed_builtin())\n"));
    }
    fs::write(fixture.root.join("all_scalars.gd"), script).unwrap();
    let (ok, result) = fixture.create(
        "res://all_scalars.gd",
        json!(properties),
        "res://all_scalars.tres",
    );
    assert!(ok, "{result}");
    assert_eq!(result["properties"], json!(properties));
    fixture.probe(&probe);
}
