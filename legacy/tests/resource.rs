use serde_json::{Value, json};
use std::{
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
#[ignore = "requires GDKIT_TEST_GODOT pointing to a Godot 4 editor"]
fn creates_verified_resources_and_preserves_destinations_on_failure() {
    let engine = std::env::var_os("GDKIT_TEST_GODOT").expect("set GDKIT_TEST_GODOT");
    let directory = std::env::temp_dir().join(format!(
        "gdkit-resource-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("project.godot"), "config_version=5\n").unwrap();
    fs::write(
        directory.join("base.gd"),
        "extends Resource\n@export var inherited: int = 7\n",
    )
    .unwrap();
    fs::write(directory.join("weapon.gd"), "extends \"res://base.gd\"\n@export var damage: int = 3\n@export var enabled: bool = false\n@export var label: String = \"default\"\n@export var ratio: float = 0.5\nvar transient: int = 0\n@export var clamped: int = 0:\n\tset(value):\n\t\tclamped = clampi(value, 0, 10)\n").unwrap();
    fs::write(directory.join("node.gd"), "extends Node\n").unwrap();
    fs::write(directory.join("reload.gd"), "extends Resource\n@export var value: int = 0:\n\tget:\n\t\treturn value if resource_path.is_empty() else value + 1\n").unwrap();
    fs::write(
        directory.join("constructor.gd"),
        "extends Resource\nfunc _init(required: int):\n\tresource_name = str(required)\n",
    )
    .unwrap();
    let validation = Command::new(&engine)
        .args(["--headless", "--editor", "--path"])
        .arg(&directory)
        .arg("--script")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/src/resource.gd"))
        .arg("--check-only")
        .output()
        .unwrap();
    let diagnostics = String::from_utf8_lossy(&validation.stderr);
    assert!(validation.status.success(), "{diagnostics}");
    assert!(!diagnostics.contains("ERROR:"), "{diagnostics}");
    for line in diagnostics
        .lines()
        .filter(|line| line.starts_with("WARNING:"))
    {
        assert!(line == "WARNING: 1 RID of type \"Canvas\" was leaked."
            || (line.starts_with("WARNING: ") && line.ends_with(" ObjectDB instances were leaked at exit (run with `--verbose` for details).")), "{diagnostics}");
    }
    let run = |spec: Value, destination: &str| {
        fs::write(
            directory.join("spec.json"),
            serde_json::to_vec(&spec).unwrap(),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_gdkit"))
            .current_dir(&directory)
            .env_remove("GDKIT_GODOT")
            .args([
                "resource",
                "create",
                "--spec",
                "spec.json",
                "--out",
                destination,
                "--output",
                "json",
                "--godot",
            ])
            .arg(&engine)
            .output()
            .unwrap();
        let result: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "{error}: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (output.status.success(), result)
    };
    let (ok, native) = run(
        json!({"class":"StandardMaterial3D","properties":{"metallic":0.5,"resource_name":"Native","next_pass":null}}),
        "res://native.tres",
    );
    assert!(ok, "{native}");
    assert_eq!(native["path"], "res://native.tres");
    let (ok, weapon) = run(
        json!({"script":"res://weapon.gd","properties":{"damage":15,"inherited":12,"enabled":true,"label":"Shotgun \"test\"\n","ratio":0.75}}),
        "res://weapon.tres",
    );
    assert!(ok, "{weapon}");
    assert_eq!(weapon["properties"]["damage"], 15);
    let serialized = fs::read_to_string(directory.join("weapon.tres")).unwrap();
    assert!(serialized.contains("res://weapon.gd"));
    assert!(!serialized.contains(".gdkit-resource-"));
    assert!(serialized.contains("inherited = 12"));
    assert!(!serialized.contains("clamped ="));
    let original = fs::read(directory.join("weapon.tres")).unwrap();
    let (ok, _) = run(
        json!({"class":"Resource","properties":{}}),
        "res://weapon.tres",
    );
    assert!(!ok);
    assert_eq!(fs::read(directory.join("weapon.tres")).unwrap(), original);
    for (spec, field, stage) in [
        (
            json!({"script":"res://weapon.gd","properties":{"damage":"bad"}}),
            "damage",
            "validate",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"damage":1.5}}),
            "damage",
            "validate",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"missing":1}}),
            "missing",
            "validate",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"transient":1}}),
            "transient",
            "validate",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"clamped":20}}),
            "clamped",
            "assign",
        ),
        (json!({"class":"Node","properties":{}}), "", "construct"),
        (
            json!({"script":"res://node.gd","properties":{}}),
            "",
            "construct",
        ),
        (
            json!({"class":"Resource","script":"res://weapon.gd","properties":{}}),
            "",
            "validate",
        ),
        (
            json!({"class":"Resource","properties":{"nested":{}}}),
            "nested",
            "validate",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"damage":9007199254740992_u64}}),
            "damage",
            "validate",
        ),
        (
            json!({"script":"res://reload.gd","properties":{"value":5}}),
            "value",
            "verify",
        ),
        (
            json!({"script":"res://constructor.gd","properties":{}}),
            "",
            "construct",
        ),
        (
            json!({"class":"Resource","properties":{"resource_path":"res://other.tres"}}),
            "resource_path",
            "validate",
        ),
    ] {
        let (ok, result) = run(spec, "res://failure.tres");
        assert!(!ok, "{result}");
        assert_eq!(result["field"], field, "{result}");
        assert_eq!(result["stage"], stage, "{result}");
        assert!(!directory.join("failure.tres").exists());
    }
    let (ok, defaults) = run(
        json!({"script":"res://weapon.gd","properties":{}}),
        "res://defaults.tres",
    );
    assert!(ok, "{defaults}");
    let (ok, result) = run(
        json!({"class":"Resource","properties":{}}),
        "res://../escape.tres",
    );
    assert!(!ok, "{result}");
    assert!(!fs::read_dir(&directory).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".gdkit-resource-")
    }));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn schema_requires_exactly_one_type_selector() {
    for arguments in [
        vec![],
        vec!["--class", "Resource", "--script", "res://test.gd"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_gdkit"))
            .args(["resource", "schema"])
            .args(arguments)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
    }
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT pointing to a Godot 4 editor"]
fn discovers_instance_defaults_hints_and_creation_support() {
    let engine = std::env::var_os("GDKIT_TEST_GODOT").expect("set GDKIT_TEST_GODOT");
    let directory = std::env::temp_dir().join(format!(
        "gdkit-schema-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("project.godot"), "config_version=5\n").unwrap();
    fs::write(
        directory.join("base.gd"),
        "extends Resource\n@export var inherited: int = 7\n",
    )
    .unwrap();
    fs::write(directory.join("fields.gd"), "extends \"res://base.gd\"\n@export_group(\"Details\")\n@export_enum(\"Off:0\", \"On:4\", \"Auto\") var mode: int = 4\n@export_enum(\"Small\", \"Large\") var size: String = \"Large\"\n@export_range(0.0, 1.0, 0.05) var ratio: float = 0.5\n@export var enabled: bool = true\n@export var offset: Vector3 = Vector3(1, 2, 3)\n@export var items: Array[int] = [1, 2]\n@export var child: Resource = Resource.new()\n@export var target: Resource\n@export var large: int = -9223372036854775808\nvar transient: int = 8\n@export var from_constructor: int = 0\nfunc _init():\n\tfrom_constructor = 42\n").unwrap();
    fs::write(
        directory.join("required.gd"),
        "extends Resource\nfunc _init(required: int):\n\tresource_name = str(required)\n",
    )
    .unwrap();
    let schema = |selector: &str, name: &str, format: &str| {
        Command::new(env!("CARGO_BIN_EXE_gdkit"))
            .current_dir(&directory)
            .env_remove("GDKIT_GODOT")
            .args([
                "resource", "schema", selector, name, "--output", format, "--godot",
            ])
            .arg(&engine)
            .output()
            .unwrap()
    };
    let native = schema("--class", "StandardMaterial3D", "json");
    assert!(
        native.status.success(),
        "{} {}",
        String::from_utf8_lossy(&native.stdout),
        String::from_utf8_lossy(&native.stderr)
    );
    let native: Value = serde_json::from_slice(&native.stdout).unwrap();
    assert_eq!(native["executes_constructors_and_getters"], true);
    let metallic = native["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|field| field["name"] == "metallic")
        .unwrap();
    assert_eq!(metallic["default"], json!({"encoding":"json","value":0.0}));
    assert_eq!(metallic["create_supported"], true);
    let discovery = schema("--script", "res://fields.gd", "json");
    assert!(
        discovery.status.success(),
        "{} {}",
        String::from_utf8_lossy(&discovery.stdout),
        String::from_utf8_lossy(&discovery.stderr)
    );
    let discovery: Value = serde_json::from_slice(&discovery.stdout).unwrap();
    assert_eq!(discovery["status"], "schema");
    assert_eq!(discovery["executes_constructors_and_getters"], true);
    let fields = discovery["fields"].as_array().unwrap();
    let field = |name: &str| fields.iter().find(|field| field["name"] == name).unwrap();
    assert!(!fields.iter().any(|field| field["name"] == "Details"));
    assert_eq!(field("inherited")["default"]["value"], 7);
    assert_eq!(field("from_constructor")["default"]["value"], 42);
    assert_eq!(
        field("mode")["enum_choices"],
        json!([{"name":"Off","value":0},{"name":"On","value":4},{"name":"Auto","value":5}])
    );
    assert_eq!(
        field("size")["enum_choices"],
        json!([{"name":"Small","value":"Small"},{"name":"Large","value":"Large"}])
    );
    assert!(
        field("ratio")["hint_string"]
            .as_str()
            .unwrap()
            .contains("0.05")
    );
    assert_eq!(
        field("offset")["default"],
        json!({"encoding":"tagged","value":{"$variant":{"type":"Vector3","value":[1.0,2.0,3.0]}}})
    );
    assert_eq!(field("items")["default"]["encoding"], "tagged");
    assert_eq!(
        field("large")["default"],
        json!({"encoding":"tagged","value":{"$variant":{"type":"int","value":"-9223372036854775808"}}})
    );
    assert_eq!(field("target")["default"]["value"], Value::Null);
    assert_eq!(field("target")["class_name"], "Resource");
    assert_eq!(field("target")["create_supported"], true);
    assert_eq!(
        field("target")["accepted_inputs"],
        json!(["null", "$ref", "$resource"])
    );
    assert_eq!(
        field("target")["resource_constraints"],
        json!([{"class":"Resource", "script":null}])
    );
    assert_eq!(
        field("child")["default"],
        json!({"encoding":"resource", "value":{"path":"", "type":"Resource", "script":null}})
    );
    assert_eq!(field("transient")["storage"], false);
    assert_eq!(field("inherited")["storage"], true);
    assert_eq!(field("inherited")["editor_visible"], true);
    for name in ["script", "resource_path", "transient"] {
        assert_eq!(field(name)["create_supported"], false, "{name}");
        assert!(
            !field(name)["unsupported_reason"]
                .as_str()
                .unwrap()
                .is_empty()
        );
    }
    let properties: serde_json::Map<String, Value> = fields
        .iter()
        .filter(|field| field["create_supported"] == true && field["default"]["encoding"] == "json")
        .map(|field| {
            (
                field["name"].as_str().unwrap().to_owned(),
                field["default"]["value"].clone(),
            )
        })
        .collect();
    fs::write(
        directory.join("spec.json"),
        serde_json::to_vec(&json!({"script":"res://fields.gd","properties":properties})).unwrap(),
    )
    .unwrap();
    let created = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .current_dir(&directory)
        .env_remove("GDKIT_GODOT")
        .args([
            "resource",
            "create",
            "--spec",
            "spec.json",
            "--out",
            "res://generated.tres",
            "--output",
            "json",
            "--godot",
        ])
        .arg(&engine)
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "{} {}",
        String::from_utf8_lossy(&created.stdout),
        String::from_utf8_lossy(&created.stderr)
    );
    let human = schema("--script", "res://fields.gd", "human");
    assert!(human.status.success());
    let human = String::from_utf8(human.stdout).unwrap();
    assert!(human.contains("executes constructors and getters"));
    assert!(human.contains("inherited: int"));
    for (selector, name) in [
        ("--class", "Node"),
        ("--script", "res://required.gd"),
        ("--script", "res://../escape.gd"),
        ("--class", "MissingClass"),
    ] {
        let failed = schema(selector, name, "json");
        assert!(!failed.status.success());
        let failed: Value = serde_json::from_slice(&failed.stdout).unwrap();
        assert_eq!(failed["status"], "error");
    }
    assert!(!fs::read_dir(&directory).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".gdkit-resource-")
    }));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT pointing to a Godot 4 editor"]
fn creates_nested_resources_and_verifies_external_references() {
    let engine = std::env::var_os("GDKIT_TEST_GODOT").expect("set GDKIT_TEST_GODOT");
    let directory = std::env::temp_dir().join(format!(
        "gdkit-nested-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("project.godot"), "config_version=5\n").unwrap();
    fs::write(directory.join("stats.gd"), "class_name NestedStats extends Resource\n@export var damage: int = 3\n@export var child: Resource\n").unwrap();
    fs::write(
        directory.join("derived.gd"),
        "extends NestedStats\n@export var bonus: int = 1\n",
    )
    .unwrap();
    fs::write(directory.join("weapon.gd"), "extends Resource\n@export var stats: NestedStats\n@export var icon: Texture2D\n@export var any: Resource\n@export var effects: Array[NestedStats] = []\n@export var resources: Array[Resource] = []\n@export var textures: Array[Texture2D] = []\n").unwrap();
    fs::write(directory.join("mutator.gd"), "extends Resource\n@export var stats: NestedStats:\n\tset(value):\n\t\tstats = value\n\t\tif stats != null:\n\t\t\tstats.damage = 99\n").unwrap();
    fs::write(
        directory.join("cycle.gd"),
        "extends Resource\n@export var loop: Resource\nfunc _init():\n\tloop = self\n",
    )
    .unwrap();
    fs::write(directory.join("reload_child.gd"), "extends Resource\n@export var damage: int = 0:\n\tget:\n\t\treturn damage if resource_path.is_empty() else damage + 1\n").unwrap();
    fs::write(directory.join("external.tres"), "[gd_resource type=\"Resource\" script_class=\"NestedStats\" load_steps=2 format=3]\n[ext_resource type=\"Script\" path=\"res://stats.gd\" id=\"1\"]\n[resource]\nscript = ExtResource(\"1\")\ndamage = 15\n").unwrap();
    fs::write(directory.join("icon.svg"), "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"4\" height=\"4\"><rect width=\"4\" height=\"4\" fill=\"red\"/></svg>").unwrap();
    fs::write(directory.join("anonymous.gd"), "extends Resource\nconst Stats = preload(\"res://derived.gd\")\n@export var effects: Array[Stats] = []\n").unwrap();
    let import = Command::new(&engine)
        .args(["--headless", "--editor", "--path"])
        .arg(&directory)
        .args(["--import"])
        .output()
        .unwrap();
    assert!(
        import.status.success(),
        "{}",
        String::from_utf8_lossy(&import.stderr)
    );
    assert!(!String::from_utf8_lossy(&import.stderr).contains("SCRIPT ERROR:"));
    let run = |spec: Value, out: &str| {
        fs::write(
            directory.join("spec.json"),
            serde_json::to_vec(&spec).unwrap(),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_gdkit"))
            .current_dir(&directory)
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
            .arg(&engine)
            .output()
            .unwrap();
        let result: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "{error}: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (output.status.success(), result)
    };
    let external = fs::read(directory.join("external.tres")).unwrap();
    let icon = fs::read(directory.join("icon.svg")).unwrap();
    let (ok, native) = run(
        json!({"class":"StandardMaterial3D", "properties":{"next_pass":{"$resource":{"class":"StandardMaterial3D", "properties":{"metallic":0.5}}}}}),
        "res://material.tres",
    );
    assert!(ok, "{native}");
    let (ok, result) = run(
        json!({"script":"res://weapon.gd", "properties": {
            "stats":{"$resource":{"script":"res://derived.gd", "properties":{"damage":20,"bonus":4,"child":{"$resource":{"class":"Resource", "properties":{"resource_name":"Inner"}}}}}},
            "icon":{"$ref":"res://icon.svg"}, "any":{"$ref":"res://external.tres"}
        }}),
        "res://nested.tres",
    );
    assert!(ok, "{result}");
    assert_eq!(
        result["properties"]["stats"]["$resource"]["properties"]["damage"],
        20
    );
    let text = fs::read_to_string(directory.join("nested.tres")).unwrap();
    assert!(text.contains("[sub_resource"));
    assert!(text.contains("res://external.tres"));
    assert!(text.contains("res://icon.svg"));
    assert!(!text.contains(".gdkit-resource-"));
    let (ok, result) = run(
        json!({"script":"res://weapon.gd", "properties":{"stats":{"$ref":"res://external.tres"},"icon":null,"any":null}}),
        "res://references.tres",
    );
    assert!(ok, "{result}");
    for script in ["res://weapon.gd", "res://anonymous.gd"] {
        let (ok, result) = run(
            json!({"script":script,"properties":{"effects":[null,{"$ref":"res://external.tres"},{"$resource":{"script":"res://derived.gd","properties":{"damage":21}}},{"$resource":{"script":"res://derived.gd","properties":{"damage":22}}}]}}),
            if script.contains("anonymous") {
                "res://anonymous_array.tres"
            } else {
                "res://arrays.tres"
            },
        );
        if script.contains("anonymous") {
            assert!(!ok, "{result}");
            assert_eq!(result["field"], "properties.effects[1]");
        } else {
            assert!(ok, "{result}");
        }
        let (ok, result) = run(
            json!({"script":script,"properties":{"effects":[]}}),
            if script.contains("anonymous") {
                "res://anonymous_empty.tres"
            } else {
                "res://empty.tres"
            },
        );
        assert!(ok, "{result}");
    }
    let (ok, result) = run(
        json!({"script":"res://weapon.gd","properties":{"resources":[null,{"$resource":{"class":"Resource","properties":{}}},{"$ref":"res://external.tres"}],"textures":[{"$ref":"res://icon.svg"}]}}),
        "res://native_arrays.tres",
    );
    assert!(ok, "{result}");
    let (ok, result) = run(
        json!({"script":"res://anonymous.gd","properties":{"effects":[{"$resource":{"script":"res://derived.gd","properties":{"damage":23}}}]}}),
        "res://anonymous_valid.tres",
    );
    assert!(ok, "{result}");
    fs::write(directory.join("verify_arrays.gd"), "extends SceneTree\nfunc _initialize():\n\tvar graph = load(\"res://arrays.tres\")\n\tassert(graph.effects.size() == 4)\n\tassert(graph.effects[0] == null)\n\tassert(graph.effects[1].resource_path == \"res://external.tres\")\n\tassert(graph.effects[2].damage == 21 and graph.effects[3].damage == 22)\n\tassert(graph.effects[2] != graph.effects[3])\n\tvar empty = load(\"res://empty.tres\").effects\n\tassert(empty.is_empty() and empty.get_typed_script() == load(\"res://stats.gd\"))\n\tvar anonymous = load(\"res://anonymous_empty.tres\").effects\n\tassert(anonymous.is_empty() and anonymous.get_typed_script() == load(\"res://derived.gd\"))\n\tquit()\n").unwrap();
    let verification = Command::new(&engine)
        .args(["--headless", "--path"])
        .arg(&directory)
        .args(["--script", "res://verify_arrays.gd"])
        .output()
        .unwrap();
    assert!(verification.status.success());
    assert!(
        !String::from_utf8_lossy(&verification.stderr).contains("SCRIPT ERROR:"),
        "{}",
        String::from_utf8_lossy(&verification.stderr)
    );
    let schema = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .current_dir(&directory)
        .env_remove("GDKIT_GODOT")
        .args([
            "resource",
            "schema",
            "--script",
            "res://weapon.gd",
            "--output",
            "json",
            "--godot",
        ])
        .arg(&engine)
        .output()
        .unwrap();
    assert!(schema.status.success());
    let schema: Value = serde_json::from_slice(&schema.stdout).unwrap();
    let stats = schema["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|field| field["name"] == "stats")
        .unwrap();
    assert_eq!(stats["create_supported"], true);
    let effects = schema["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|field| field["name"] == "effects")
        .unwrap();
    assert_eq!(effects["create_supported"], true);
    assert_eq!(effects["accepted_inputs"], json!(["array"]));
    assert_eq!(effects["variant_contract"], json!({}));
    assert_eq!(
        effects["element_accepted_inputs"],
        json!(["null", "$ref", "$resource"])
    );
    assert_eq!(
        effects["element_constraints"][0]["script"],
        "res://stats.gd"
    );

    assert_eq!(
        stats["resource_constraints"],
        json!([{"class":"NestedStats", "script":"res://stats.gd"}])
    );
    for (spec, field, stage) in [
        (
            json!({"script":"res://weapon.gd","properties":{"resources":[{"$resource":{"script":"res://reload_child.gd","properties":{"damage":5}}}]}}),
            "properties.resources[0].properties.damage",
            "verify",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"effects":[{"$ref":"res://missing.tres"}]}}),
            "properties.effects[0]",
            "load",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"effects":[null,{"$resource":{"class":"Resource","properties":{}}}]}}),
            "properties.effects[1]",
            "validate",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"effects":[null,{"$resource":{"script":"res://stats.gd","properties":{"damage":"bad"}}}]}}),
            "properties.effects[1].properties.damage",
            "validate",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"effects":[1]}}),
            "effects[0]",
            "validate",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"effects":null}}),
            "effects",
            "validate",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"stats":{"$resource":{"class":"Resource","properties":{}}}}}),
            "stats",
            "validate",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"icon":{"$ref":"res://external.tres"}}}),
            "icon",
            "validate",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"any":{"$ref":"res://missing.tres"}}}),
            "any",
            "load",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"any":{"$ref":"res://../escape.tres"}}}),
            "any",
            "validate",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"any":{"$ref":"res://external.tres","extra":1}}}),
            "any",
            "validate",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"stats":{"$resource":{"script":"res://stats.gd","properties":{"damage":"bad"}}}}}),
            "properties.stats.properties.damage",
            "validate",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"stats":{"$resource":{"script":"res://stats.gd","properties":{"damage":9007199254740992_u64}}}}}),
            "properties.stats.properties.damage",
            "validate",
        ),
        (
            json!({"script":"res://mutator.gd","properties":{"stats":{"$ref":"res://external.tres"}}}),
            "properties.stats.properties.damage",
            "assign",
        ),
        (
            json!({"script":"res://mutator.gd","properties":{"stats":{"$resource":{"script":"res://stats.gd","properties":{"damage":20}}}}}),
            "properties.stats.properties.damage",
            "assign",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"any":{"$resource":{"script":"res://reload_child.gd","properties":{"damage":5}}}}}),
            "properties.any.properties.damage",
            "verify",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"any":{"$resource":{"script":"res://cycle.gd","properties":{}}}}}),
            "properties.any.properties.loop",
            "validate",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"any":[]}}),
            "any",
            "validate",
        ),
    ] {
        let (ok, result) = run(spec, "res://failure.tres");
        assert!(!ok, "{result}");
        assert_eq!(result["field"], field, "{result}");
        assert_eq!(result["stage"], stage, "{result}");
        assert!(!directory.join("failure.tres").exists());
        assert_eq!(fs::read(directory.join("external.tres")).unwrap(), external);
        assert_eq!(fs::read(directory.join("icon.svg")).unwrap(), icon);
    }
    let mut deep = json!({"script":"res://stats.gd","properties":{}});
    for _ in 0..17 {
        deep = json!({"script":"res://stats.gd","properties":{"child":{"$resource":deep}}});
    }
    let (ok, result) = run(deep, "res://deep.tres");
    assert!(!ok, "{result}");
    assert!(
        result["message"]
            .as_str()
            .unwrap()
            .contains("nesting exceeds")
    );
    assert!(!directory.join("deep.tres").exists());
    assert!(!fs::read_dir(&directory).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".gdkit-resource-")
    }));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT pointing to a Godot 4 editor"]
fn tagged_scalars_round_trip_exactly_and_expose_reusable_defaults() {
    let engine = std::env::var_os("GDKIT_TEST_GODOT").expect("set GDKIT_TEST_GODOT");
    let directory = std::env::temp_dir().join(format!(
        "gdkit-tagged-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("project.godot"), "config_version=5\n").unwrap();
    fs::write(directory.join("values.gd"), "extends Resource\n@export var integer: int = -9223372036854775808\n@export var symbol: StringName = &\"default\"\n@export var node_path: NodePath = ^\"Root/Child:position:x\"\n@export var child: Resource\n@export var clamped: int = 0:\n\tset(new_value):\n\t\tclamped = clampi(new_value, 0, 10)\n").unwrap();
    let run = |spec: Value, destination: &str| {
        fs::write(
            directory.join("spec.json"),
            serde_json::to_vec(&spec).unwrap(),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_gdkit"))
            .current_dir(&directory)
            .env_remove("GDKIT_GODOT")
            .args([
                "resource",
                "create",
                "--spec",
                "spec.json",
                "--out",
                destination,
                "--output",
                "json",
                "--godot",
            ])
            .arg(&engine)
            .output()
            .unwrap();
        let result: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "{error}: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (output.status.success(), result)
    };
    let tag = |kind: &str, value: &str| json!({"$variant":{"type":kind,"value":value}});
    for (index, integer) in [
        "-9223372036854775808",
        "9223372036854775807",
        "9007199254740992",
        "-9007199254740992",
        "0",
        "-1",
    ]
    .into_iter()
    .enumerate()
    {
        let properties = json!({"integer":tag("int", integer), "symbol":tag("StringName", "name with spaces \u{03bb}"), "node_path":tag("NodePath", "/Root/Child:position:x")});
        let (ok, result) = run(
            json!({"script":"res://values.gd","properties":properties}),
            &format!("res://values{index}.tres"),
        );
        assert!(ok, "{result}");
        assert_eq!(result["properties"], properties);
    }
    let (ok, result) = run(
        json!({"script":"res://values.gd","properties":{"symbol":tag("StringName", ""),"node_path":tag("NodePath", "")}}),
        "res://empty.tres",
    );
    assert!(ok, "{result}");
    let (ok, result) = run(
        json!({"script":"res://values.gd","properties":{"child":{"$resource":{"script":"res://values.gd","properties":{"integer":tag("int", "9223372036854775807")}}}}}),
        "res://nested.tres",
    );
    assert!(ok, "{result}");
    assert_eq!(
        result["properties"]["child"]["$resource"]["properties"]["integer"],
        tag("int", "9223372036854775807")
    );
    let schema = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .current_dir(&directory)
        .env_remove("GDKIT_GODOT")
        .args([
            "resource",
            "schema",
            "--script",
            "res://values.gd",
            "--output",
            "json",
            "--godot",
        ])
        .arg(&engine)
        .output()
        .unwrap();
    assert!(
        schema.status.success(),
        "{}",
        String::from_utf8_lossy(&schema.stderr)
    );
    let schema: Value = serde_json::from_slice(&schema.stdout).unwrap();
    assert_eq!(schema["variant_codec_version"], 1);
    let fields = schema["fields"].as_array().unwrap();
    for (name, kind) in [
        ("integer", "int"),
        ("symbol", "StringName"),
        ("node_path", "NodePath"),
    ] {
        let field = fields.iter().find(|field| field["name"] == name).unwrap();
        assert_eq!(field["create_supported"], true);
        assert_eq!(field["variant_contract"]["type"], kind);
        assert_eq!(field["variant_contract"]["value_encoding"], "string");
        assert_eq!(field["default"]["encoding"], "tagged");
    }
    let properties: serde_json::Map<String, Value> = fields
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
    let (ok, result) = run(
        json!({"script":"res://values.gd","properties":properties}),
        "res://defaults.tres",
    );
    assert!(ok, "{result}");
    fs::write(directory.join("probe.gd"), "extends SceneTree\nfunc _initialize():\n\tvar low = load(\"res://values0.tres\")\n\tvar high = load(\"res://values1.tres\")\n\tassert(low.integer == -9223372036854775808)\n\tassert(high.integer == 9223372036854775807)\n\tassert(typeof(low.symbol) == TYPE_STRING_NAME)\n\tassert(str(low.symbol) == \"name with spaces \u{03bb}\")\n\tassert(typeof(low.node_path) == TYPE_NODE_PATH)\n\tassert(str(low.node_path) == \"/Root/Child:position:x\")\n\tquit()\n").unwrap();
    let probe = Command::new(&engine)
        .args(["--headless", "--path"])
        .arg(&directory)
        .args(["--script", "res://probe.gd"])
        .output()
        .unwrap();
    assert!(probe.status.success());
    assert!(
        !String::from_utf8_lossy(&probe.stderr).contains("ERROR:"),
        "{}",
        String::from_utf8_lossy(&probe.stderr)
    );
    for invalid in [
        tag("int", "9223372036854775808"),
        tag("int", "-9223372036854775809"),
        tag("int", "01"),
        tag("int", "+1"),
        tag("int", "-0"),
        tag("int", "1.0"),
        tag("int", "1x"),
        tag("int", ""),
        json!({"$variant":{"type":"int","value":1}}),
        json!({"$variant":{"type":"int","value":"1","extra":true}}),
        tag("Unknown", "1"),
        tag("StringName", "1"),
        json!({"$variant":null}),
    ] {
        let (ok, result) = run(
            json!({"script":"res://values.gd","properties":{"integer":invalid}}),
            "res://failure.tres",
        );
        assert!(!ok, "{result}");
        assert_eq!(result["stage"], "validate");
        assert_eq!(result["field"], "integer");
        assert!(!directory.join("failure.tres").exists());
    }
    let (ok, result) = run(
        json!({"script":"res://values.gd","properties":{"child":tag("int", "1")}}),
        "res://failure.tres",
    );
    assert!(!ok, "{result}");
    assert_eq!(result["stage"], "validate");
    assert_eq!(result["field"], "child");
    for name in ["symbol", "node_path"] {
        let (ok, result) = run(
            json!({"script":"res://values.gd","properties":{name:"ordinary string"}}),
            "res://failure.tres",
        );
        assert!(!ok, "{result}");
        assert_eq!(result["field"], name);
    }
    let (ok, result) = run(
        json!({"script":"res://values.gd","properties":{"clamped":tag("int", "9223372036854775807")}}),
        "res://failure.tres",
    );
    assert!(!ok, "{result}");
    assert_eq!(result["stage"], "assign");
    assert_eq!(result["field"], "clamped");
    assert!(!directory.join("failure.tres").exists());
    fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT pointing to a Godot 4 editor"]
fn tagged_vectors_and_colors_preserve_components_and_reject_loss() {
    let engine = std::env::var_os("GDKIT_TEST_GODOT").expect("set GDKIT_TEST_GODOT");
    let directory = std::env::temp_dir().join(format!(
        "gdkit-components-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("project.godot"), "config_version=5\n").unwrap();
    fs::write(directory.join("math.gd"), "extends Resource\n@export var v2: Vector2 = Vector2(0.1, 0.2)\n@export var v3: Vector3 = Vector3(1, 2, 3)\n@export var v4: Vector4 = Vector4(1, 2, 3, 4)\n@export var i2: Vector2i = Vector2i(1, 2)\n@export var i3: Vector3i = Vector3i(1, 2, 3)\n@export var i4: Vector4i = Vector4i(1, 2, 3, 4)\n@export var color: Color = Color(0.1, 0.2, 0.3, 0.4)\n@export var child: Resource\n@export var clamped: Vector2 = Vector2.ZERO:\n\tset(new_value):\n\t\tclamped = new_value.limit_length(1)\n").unwrap();
    let run = |spec: Value, destination: &str| {
        fs::write(
            directory.join("spec.json"),
            serde_json::to_vec(&spec).unwrap(),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_gdkit"))
            .current_dir(&directory)
            .env_remove("GDKIT_GODOT")
            .args([
                "resource",
                "create",
                "--spec",
                "spec.json",
                "--out",
                destination,
                "--output",
                "json",
                "--godot",
            ])
            .arg(&engine)
            .output()
            .unwrap();
        let result: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "{error}: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (output.status.success(), result)
    };
    let tag = |kind: &str, value: Value| json!({"$variant":{"type":kind,"value":value}});
    let properties = json!({"v2":tag("Vector2", json!([1.5,-2.25])),"v3":tag("Vector3", json!([1.5,-2.25,3.125])),"v4":tag("Vector4", json!([1.5,-2.25,3.125,4.5])),"i2":tag("Vector2i", json!([-2147483648,2147483647])),"i3":tag("Vector3i", json!([-2147483648,0,2147483647])),"i4":tag("Vector4i", json!([-2147483648,0,-1,2147483647])),"color":tag("Color", json!([2.0,-0.5,0.75,0.25]))});
    let (ok, result) = run(
        json!({"script":"res://math.gd","properties":properties}),
        "res://math.tres",
    );
    assert!(ok, "{result}");
    assert_eq!(result["properties"], properties);
    let (ok, result) = run(
        json!({"class":"StandardMaterial3D","properties":{"albedo_color":tag("Color",json!([0.5,0.25,0.75,1.0]))}}),
        "res://material.tres",
    );
    assert!(ok, "{result}");
    let schema = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .current_dir(&directory)
        .env_remove("GDKIT_GODOT")
        .args([
            "resource",
            "schema",
            "--script",
            "res://math.gd",
            "--output",
            "json",
            "--godot",
        ])
        .arg(&engine)
        .output()
        .unwrap();
    assert!(
        schema.status.success(),
        "{}",
        String::from_utf8_lossy(&schema.stderr)
    );
    let schema: Value = serde_json::from_slice(&schema.stdout).unwrap();
    let fields = schema["fields"].as_array().unwrap();
    for (name, kind, length) in [
        ("v2", "Vector2", 2),
        ("v3", "Vector3", 3),
        ("v4", "Vector4", 4),
        ("i2", "Vector2i", 2),
        ("i3", "Vector3i", 3),
        ("i4", "Vector4i", 4),
        ("color", "Color", 4),
    ] {
        let field = fields.iter().find(|field| field["name"] == name).unwrap();
        assert_eq!(field["create_supported"], true);
        assert_eq!(field["accepted_inputs"], json!(["$variant"]));
        assert_eq!(field["default"]["encoding"], "tagged");
        assert_eq!(field["variant_contract"]["type"], kind);
        assert_eq!(field["variant_contract"]["value_encoding"], "array");
        assert_eq!(field["variant_contract"]["length"], length);
        assert_eq!(field["variant_contract"]["exact_components"], true);
    }
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
    let (ok, result) = run(
        json!({"script":"res://math.gd","properties":defaults}),
        "res://defaults.tres",
    );
    assert!(ok, "{result}");
    let (ok, result) = run(
        json!({"script":"res://math.gd","properties":{"child":{"$resource":{"script":"res://math.gd","properties":properties}}}}),
        "res://nested.tres",
    );
    assert!(ok, "{result}");
    assert_eq!(
        result["properties"]["child"]["$resource"]["properties"],
        properties
    );
    fs::write(directory.join("probe.gd"), "extends SceneTree\nfunc _initialize():\n\tvar graph = load(\"res://math.tres\")\n\tassert(graph.v2 == Vector2(1.5, -2.25))\n\tassert(graph.v3 == Vector3(1.5, -2.25, 3.125))\n\tassert(graph.v4 == Vector4(1.5, -2.25, 3.125, 4.5))\n\tassert(graph.i2 == Vector2i(-2147483648, 2147483647))\n\tassert(graph.i3 == Vector3i(-2147483648, 0, 2147483647))\n\tassert(graph.i4 == Vector4i(-2147483648, 0, -1, 2147483647))\n\tassert(graph.color == Color(2, -0.5, 0.75, 0.25))\n\tquit()\n").unwrap();
    let probe = Command::new(&engine)
        .args(["--headless", "--path"])
        .arg(&directory)
        .args(["--script", "res://probe.gd"])
        .output()
        .unwrap();
    assert!(probe.status.success());
    assert!(
        !String::from_utf8_lossy(&probe.stderr).contains("ERROR:"),
        "{}",
        String::from_utf8_lossy(&probe.stderr)
    );
    for (name, value, field, stage) in [
        ("v2", json!([1, 2]), "v2[0]", "validate"),
        ("v2", tag("Vector2", json!([1])), "v2", "validate"),
        ("v2", tag("Vector2", json!([1, 2, 3])), "v2", "validate"),
        ("v2", tag("Vector3", json!([1, 2, 3])), "v2", "validate"),
        (
            "v2",
            tag("Vector2", json!("Vector2(1,2)")),
            "v2",
            "validate",
        ),
        (
            "v2",
            tag("Vector2", json!([true, 2])),
            "v2.$variant.value[0]",
            "validate",
        ),
        (
            "v2",
            tag("Vector2", json!([null, 2])),
            "v2.$variant.value[0]",
            "validate",
        ),
        (
            "v2",
            tag("Vector2", json!([0.1, 2])),
            "v2.$variant.value[0]",
            "validate",
        ),
        (
            "v2",
            tag("Vector2", json!([9007199254740993_i64, 2])),
            "v2.$variant.value[0]",
            "validate",
        ),
        (
            "v2",
            tag("Vector2", json!([1e100, 2])),
            "v2.$variant.value[0]",
            "validate",
        ),
        (
            "i2",
            tag("Vector2i", json!([2147483648_i64, 2])),
            "i2.$variant.value[0]",
            "validate",
        ),
        (
            "i2",
            tag("Vector2i", json!([-2147483649_i64, 2])),
            "i2.$variant.value[0]",
            "validate",
        ),
        (
            "i2",
            tag("Vector2i", json!([1.5, 2])),
            "i2.$variant.value[0]",
            "validate",
        ),
        ("color", tag("Color", json!([1, 2, 3])), "color", "validate"),
        (
            "clamped",
            tag("Vector2", json!([2.0, 0.0])),
            "clamped",
            "assign",
        ),
    ] {
        let (ok, result) = run(
            json!({"script":"res://math.gd","properties":{name:value}}),
            "res://failure.tres",
        );
        assert!(!ok, "{result}");
        assert_eq!(result["stage"], stage, "{result}");
        assert_eq!(result["field"], field, "{result}");
        assert!(!directory.join("failure.tres").exists());
    }
    let (ok, result) = run(
        json!({"script":"res://math.gd","properties":{"child":{"$resource":{"script":"res://math.gd","properties":{"v3":tag("Vector3",json!([1,2,0.1]))}}}}}),
        "res://failure.tres",
    );
    assert!(!ok, "{result}");
    assert_eq!(result["stage"], "validate");
    assert_eq!(
        result["field"],
        "properties.child.properties.v3.$variant.value[2]"
    );
    assert!(!directory.join("failure.tres").exists());
    fs::write(
        directory.join("nonfinite.gd"),
        "extends Resource\n@export var offset: Vector2 = Vector2(INF, 0)\n",
    )
    .unwrap();
    let schema = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .current_dir(&directory)
        .env_remove("GDKIT_GODOT")
        .args([
            "resource",
            "schema",
            "--script",
            "res://nonfinite.gd",
            "--output",
            "json",
            "--godot",
        ])
        .arg(&engine)
        .output()
        .unwrap();
    assert!(schema.status.success());
    let schema: Value = serde_json::from_slice(&schema.stdout).unwrap();
    let offset = schema["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|field| field["name"] == "offset")
        .unwrap();
    assert_eq!(offset["default"]["encoding"], "godot");
    fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT pointing to a Godot 4 editor"]
fn tagged_rectangles_and_spatial_values_round_trip_without_normalization() {
    let engine = std::env::var_os("GDKIT_TEST_GODOT").expect("set GDKIT_TEST_GODOT");
    let directory = std::env::temp_dir().join(format!(
        "gdkit-spatial-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("project.godot"), "config_version=5\n").unwrap();
    fs::write(directory.join("spatial.gd"), "extends Resource\n@export var rect: Rect2 = Rect2(0.1, 0.2, 0.3, 0.4)\n@export var recti: Rect2i\n@export var transform2: Transform2D\n@export var transform3: Transform3D\n@export var quaternion: Quaternion\n@export var basis: Basis\n@export var plane: Plane\n@export var bounds: AABB\n@export var child: Resource\n@export var clamped: Transform3D = Transform3D.IDENTITY:\n\tset(new_value):\n\t\tnew_value.origin = Vector3.ZERO\n\t\tclamped = new_value\n").unwrap();
    fs::write(directory.join("reload.gd"), "extends Resource\n@export var reload_transform: Transform3D = Transform3D.IDENTITY:\n\tget:\n\t\treturn reload_transform if resource_path.is_empty() else Transform3D(reload_transform.basis, reload_transform.origin + Vector3.ONE)\n").unwrap();
    let run = |spec: Value, destination: &str| {
        fs::write(
            directory.join("spec.json"),
            serde_json::to_vec(&spec).unwrap(),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_gdkit"))
            .current_dir(&directory)
            .env_remove("GDKIT_GODOT")
            .args([
                "resource",
                "create",
                "--spec",
                "spec.json",
                "--out",
                destination,
                "--output",
                "json",
                "--godot",
            ])
            .arg(&engine)
            .output()
            .unwrap();
        let result: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "{error}: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (output.status.success(), result)
    };
    let cases = [
        (
            "rect",
            "Rect2",
            json!([1.5, -2.25, -3.125, 4.5]),
            "Rect2(1.5, -2.25, -3.125, 4.5)",
        ),
        (
            "recti",
            "Rect2i",
            json!([-2147483648, 2147483647, -3, 4]),
            "Rect2i(-2147483648, 2147483647, -3, 4)",
        ),
        (
            "transform2",
            "Transform2D",
            json!([2, 3, 4, 5, -6, 7]),
            "Transform2D(Vector2(2,3), Vector2(4,5), Vector2(-6,7))",
        ),
        (
            "basis",
            "Basis",
            json!([2, 3, 4, 5, 6, 7, 8, 9, 10]),
            "Basis(Vector3(2,3,4), Vector3(5,6,7), Vector3(8,9,10))",
        ),
        (
            "transform3",
            "Transform3D",
            json!([2, 3, 4, 5, 6, 7, 8, 9, 10, -11, 12, 13]),
            "Transform3D(Basis(Vector3(2,3,4), Vector3(5,6,7), Vector3(8,9,10)), Vector3(-11,12,13))",
        ),
        (
            "quaternion",
            "Quaternion",
            json!([1.5, -2.25, 3.125, 4.5]),
            "Quaternion(1.5,-2.25,3.125,4.5)",
        ),
        (
            "plane",
            "Plane",
            json!([2, 3, 4, -5]),
            "Plane(Vector3(2,3,4), -5)",
        ),
        (
            "bounds",
            "AABB",
            json!([1, 2, 3, -4, 5, 6]),
            "AABB(Vector3(1,2,3), Vector3(-4,5,6))",
        ),
    ];
    let tag = |kind: &str, payload: Value| json!({"$variant":{"type":kind,"value":payload}});
    let properties: serde_json::Map<String, Value> = cases
        .iter()
        .map(|(name, kind, payload, _)| (name.to_string(), tag(kind, payload.clone())))
        .collect();
    let expected: serde_json::Map<String, Value> = cases
        .iter()
        .map(|(name, kind, payload, _)| {
            let payload = if *kind == "Rect2i" {
                payload.clone()
            } else {
                json!(
                    payload
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|value| value.as_f64().unwrap())
                        .collect::<Vec<_>>()
                )
            };
            (name.to_string(), tag(kind, payload))
        })
        .collect();
    let (ok, result) = run(
        json!({"script":"res://spatial.gd","properties":properties}),
        "res://spatial.tres",
    );
    assert!(ok, "{result}");
    assert_eq!(result["properties"], json!(expected));
    let (ok, result) = run(
        json!({"script":"res://spatial.gd","properties":{"child":{"$resource":{"script":"res://spatial.gd","properties":properties}}}}),
        "res://nested.tres",
    );
    assert!(ok, "{result}");
    assert_eq!(
        result["properties"]["child"]["$resource"]["properties"],
        json!(expected)
    );
    let mut probe =
        "extends SceneTree\nfunc _initialize():\n\tvar graph = load(\"res://spatial.tres\")\n"
            .to_owned();
    for (name, _, _, constructor) in &cases {
        probe.push_str(&format!("\tassert(graph.{name} == {constructor})\n"));
    }
    probe.push_str("\tquit()\n");
    fs::write(directory.join("probe.gd"), probe).unwrap();
    let probe = Command::new(&engine)
        .args(["--headless", "--path"])
        .arg(&directory)
        .args(["--script", "res://probe.gd"])
        .output()
        .unwrap();
    assert!(probe.status.success());
    assert!(
        !String::from_utf8_lossy(&probe.stderr).contains("ERROR:"),
        "{}",
        String::from_utf8_lossy(&probe.stderr)
    );
    let schema = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .current_dir(&directory)
        .env_remove("GDKIT_GODOT")
        .args([
            "resource",
            "schema",
            "--script",
            "res://spatial.gd",
            "--output",
            "json",
            "--godot",
        ])
        .arg(&engine)
        .output()
        .unwrap();
    assert!(schema.status.success());
    let schema: Value = serde_json::from_slice(&schema.stdout).unwrap();
    let fields = schema["fields"].as_array().unwrap();
    for (name, kind, payload, _) in &cases {
        let field = fields.iter().find(|field| field["name"] == *name).unwrap();
        assert_eq!(field["create_supported"], true);
        assert_eq!(field["accepted_inputs"], json!(["$variant"]));
        assert_eq!(field["default"]["encoding"], "tagged");
        assert_eq!(field["variant_contract"]["type"], *kind);
        assert_eq!(
            field["variant_contract"]["length"],
            payload.as_array().unwrap().len()
        );
        assert_eq!(
            field["variant_contract"]["components"]
                .as_array()
                .unwrap()
                .len(),
            payload.as_array().unwrap().len()
        );
        assert_eq!(field["variant_contract"]["exact_components"], true);
        let mut short = payload.as_array().unwrap().clone();
        short.pop();
        let mut long = payload.as_array().unwrap().clone();
        long.push(json!(0));
        for invalid in [
            tag(kind, json!(short)),
            tag(kind, json!(long)),
            tag(kind, json!("not an array")),
            tag("Vector2", json!([1, 2])),
        ] {
            let (ok, result) = run(
                json!({"script":"res://spatial.gd","properties":{*name:invalid}}),
                "res://failure.tres",
            );
            assert!(!ok, "{result}");
            assert_eq!(result["stage"], "validate");
            assert_eq!(result["field"], *name);
            assert!(!directory.join("failure.tres").exists());
        }
        let mut inexact = payload.as_array().unwrap().clone();
        inexact[0] = json!(0.1);
        let (ok, result) = run(
            json!({"script":"res://spatial.gd","properties":{*name:tag(kind,json!(inexact))}}),
            "res://failure.tres",
        );
        assert!(!ok, "{result}");
        assert_eq!(result["field"], format!("{name}.$variant.value[0]"));
    }
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
    let (ok, result) = run(
        json!({"script":"res://spatial.gd","properties":defaults}),
        "res://defaults.tres",
    );
    assert!(ok, "{result}");
    let transform = properties["transform3"].clone();
    let (ok, result) = run(
        json!({"script":"res://spatial.gd","properties":{"clamped":transform}}),
        "res://failure.tres",
    );
    assert!(!ok, "{result}");
    assert_eq!(result["stage"], "assign");
    assert_eq!(result["field"], "clamped");
    let (ok, result) = run(
        json!({"script":"res://reload.gd","properties":{"reload_transform":transform}}),
        "res://failure.tres",
    );
    assert!(!ok, "{result}");
    assert_eq!(result["stage"], "verify", "{result}");
    assert_eq!(result["field"], "reload_transform");
    assert!(!directory.join("failure.tres").exists());
    fs::remove_dir_all(directory).unwrap();
}
