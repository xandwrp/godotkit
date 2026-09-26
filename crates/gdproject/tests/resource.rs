// Acceptance tests for gdproject::resource. Offline unless prefixed real_engine_.
#![allow(unused)]

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gdproject::config::SelectionSource;
use gdproject::engine::Engine;
use gdproject::resource::{self, CreateSpec, DEFAULT_DEADLINE};
use gdproject::runner::{self, Invocation};
use gdproject::{Error, Workspace};
use gdview::ResPath;
use gdview::variant::{ResourceTarget, VariantType};
use serde_json::{Value, json};

fn fake(scenario: Value) -> (tempfile::TempDir, Engine) {
    let dir = tempfile::tempdir().unwrap();
    let executable = dir.path().join("godot");
    // A waited child owns the copy's writable descriptor (see tests/check.rs).
    #[cfg(unix)]
    {
        let status = std::process::Command::new("cp")
            .arg(env!("CARGO_BIN_EXE_fake-godot"))
            .arg(&executable)
            .status()
            .unwrap();
        assert!(status.success(), "fixture executable copy failed: {status}");
    }
    #[cfg(windows)]
    fs::copy(env!("CARGO_BIN_EXE_fake-godot"), &executable).unwrap();
    fs::write(dir.path().join("godot.scenario.json"), scenario.to_string()).unwrap();
    let engine = Engine {
        executable,
        version: "4.7.2.fake".into(),
        fingerprint: "fake".into(),
        source: SelectionSource::CommandLine,
    };
    (dir, engine)
}

fn invocations(engine: &Engine) -> Vec<Vec<String>> {
    let mut log = engine.executable.clone().into_os_string();
    log.push(".log");
    fs::read_to_string(PathBuf::from(log))
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn project(files: &[(&str, &str)]) -> (tempfile::TempDir, Workspace) {
    let dir = tempfile::tempdir().unwrap();
    for (path, contents) in [("project.godot", "config_version=5\n")]
        .iter()
        .chain(files)
    {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    let workspace = Workspace::open(dir.path()).unwrap();
    (dir, workspace)
}

fn class(name: &str) -> ResourceTarget {
    ResourceTarget::Class(name.into())
}

fn script(path: &str) -> ResourceTarget {
    ResourceTarget::Script(ResPath::parse(path).unwrap())
}

#[test]
fn schema_passes_the_target_and_derives_fields_from_the_raw_payload() {
    // Verbatim 4.7.2 entries for a WeaponDefinition script (category, script, fields).
    let payload = json!({
        "class": "Resource",
        "script_class": "WeaponDefinition",
        "properties": [
            {"name": "Resource", "type": 0, "class_name": "", "hint": 0, "hint_string": "Resource", "usage": 128, "default": null, "default_error": null},
            {"name": "script", "type": 24, "class_name": "Script", "hint": 17, "hint_string": "Script", "usage": 1048590, "default": null, "default_error": null},
            {"name": "kind", "type": 2, "class_name": "WeaponDefinition.Kind", "hint": 2, "hint_string": "Melee:0,Ranged:5", "usage": 69638, "default": 5, "default_error": null},
            {"name": "textures", "type": 28, "class_name": "", "hint": 23, "hint_string": "24/17:Texture2D", "usage": 4102, "default": [], "default_error": null},
            {"name": "callback", "type": 25, "class_name": "", "hint": 0, "hint_string": "", "usage": 4102, "default": null, "default_error": "Variant type Callable is not transportable"},
        ],
    });
    let (_engine_dir, engine) = fake(json!({"resource_schema": {"payload": payload}}));
    let (dir, workspace) = project(&[]);
    let target = script("res://weapon.gd");
    let schema = resource::schema(&workspace, &engine, &target, DEFAULT_DEADLINE).unwrap();
    assert_eq!(schema.schema_version, 1);
    assert_eq!(schema.target, target);
    assert_eq!(schema.class, "Resource");
    assert_eq!(schema.script_class.as_deref(), Some("WeaponDefinition"));
    let names: Vec<_> = schema.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["kind", "textures", "callback"]);
    assert_eq!(
        schema.fields[0].enum_name.as_deref(),
        Some("WeaponDefinition.Kind")
    );
    assert_eq!(schema.fields[0].default, json!(5));
    assert_eq!(
        schema.fields[1]
            .element
            .as_ref()
            .unwrap()
            .class_name
            .as_deref(),
        Some("Texture2D")
    );
    assert!(schema.fields[2].unsupported.is_some());
    assert!(schema.engine_diagnostics.is_empty());

    let runs = invocations(&engine);
    assert_eq!(runs.len(), 1);
    let run = &runs[0];
    assert_eq!(
        &run[..4],
        [
            "--headless",
            "--no-header",
            "--path",
            dir.path().to_str().unwrap()
        ]
    );
    assert!(run[5].ends_with("/resource_schema.gd"), "{run:?}");
    assert_eq!(&run[6..], ["--", "script", "res://weapon.gd"]);
    assert!(!run.iter().any(|arg| arg == "--editor"));

    let (_engine_dir, engine) = fake(json!({}));
    let schema = resource::schema(&workspace, &engine, &class("Curve"), DEFAULT_DEADLINE).unwrap();
    assert_eq!(schema.class, "Curve");
    assert!(schema.fields.is_empty());
    assert_eq!(&invocations(&engine)[0][6..], ["--", "class", "Curve"]);
}

#[test]
fn schema_errors_keep_the_field_and_hint_at_import_only_without_a_class_cache() {
    let error = json!({"resource_schema": {"mode": "error_envelope", "payload": {
        "stage": "target", "message": "res://weapon.gd cannot be instantiated", "field": "script",
    }}});
    let (_engine_dir, engine) = fake(error.clone());
    let (dir, workspace) = project(&[]);
    let target = script("res://weapon.gd");
    let message = match resource::schema(&workspace, &engine, &target, DEFAULT_DEADLINE) {
        Err(Error::Harness {
            stage,
            field,
            message,
            ..
        }) => {
            assert_eq!(
                (stage.as_str(), field.as_deref()),
                ("target", Some("script"))
            );
            message
        }
        other => panic!("{other:?}"),
    };
    assert!(message.starts_with("res://weapon.gd cannot be instantiated"));
    assert!(message.contains("has not been imported"), "{message}");

    fs::create_dir_all(dir.path().join(".godot")).unwrap();
    fs::write(
        dir.path().join(".godot/global_script_class_cache.cfg"),
        "list=[]\n",
    )
    .unwrap();
    match resource::schema(&workspace, &engine, &target, DEFAULT_DEADLINE) {
        Err(Error::Harness { message, .. }) => {
            assert_eq!(message, "res://weapon.gd cannot be instantiated");
        }
        other => panic!("{other:?}"),
    }

    // Unknown class names get it; other class failures and other stages do not.
    fs::remove_file(dir.path().join(".godot/global_script_class_cache.cfg")).unwrap();
    for (stage, message, field, hinted) in [
        ("target", "Unknown class Weapon", "class", true),
        ("target", "Node is not a Resource", "class", false),
        ("arguments", "Expected `class <Name>`", "script", false),
    ] {
        let (_engine_dir, engine) = fake(json!({"resource_schema": {"mode": "error_envelope",
            "payload": {"stage": stage, "message": message, "field": field}}}));
        match resource::schema(&workspace, &engine, &class("Weapon"), DEFAULT_DEADLINE) {
            Err(Error::Harness { message: got, .. }) => {
                assert!(got.starts_with(message));
                assert_eq!(got.contains("has not been imported"), hinted, "{got}");
            }
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn schema_reports_engine_diagnostics_without_failing() {
    let (_engine_dir, engine) = fake(json!({"resource_schema": {
        "stderr": "ERROR: AUTOLOAD RAN\n   at: push_error (core/variant/variant_utility.cpp:1023)\nWARNING: something odd\n",
    }}));
    let (_dir, workspace) = project(&[]);
    let schema =
        resource::schema(&workspace, &engine, &class("Resource"), DEFAULT_DEADLINE).unwrap();
    let messages: Vec<_> = schema
        .engine_diagnostics
        .iter()
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(messages, ["AUTOLOAD RAN", "something odd"]);
}

#[test]
fn schema_rejects_a_success_envelope_without_a_payload() {
    let (_engine_dir, engine) = fake(json!({"resource_schema": {"payload": null}}));
    let (_dir, workspace) = project(&[]);
    assert!(matches!(
        resource::schema(&workspace, &engine, &class("Resource"), DEFAULT_DEADLINE),
        Err(Error::Protocol { .. })
    ));
}

#[test]
fn spec_validation_rejects_bad_targets_paths_and_variants() {
    let spec = CreateSpec::from_json(&json!({
        "script": "res://weapon.gd",
        "properties": {"damage": 3, "offset": {"$variant": {"type": "Vector3", "value": [1, 2, 3]}}},
    }))
    .unwrap();
    assert_eq!(spec.target, script("res://weapon.gd"));
    assert_eq!(spec.properties.len(), 2);
    assert!(
        CreateSpec::from_json(&json!({"class": "Curve"}))
            .unwrap()
            .properties
            .is_empty()
    );

    for (value, expected) in [
        (json!("res://weapon.gd"), "invalid resource spec"),
        (json!({}), "exactly one of class or script"),
        (json!({"class": ""}), "class must be a class name"),
        (
            json!({"class": "Curve", "script": "res://a.gd"}),
            "exactly one of class or script",
        ),
        (json!({"script": "weapon.gd"}), "saved res:// script path"),
        (
            json!({"script": "res://.godot/x.gd"}),
            "saved res:// script path",
        ),
        (
            json!({"script": "res://a.tscn::GDScript_1"}),
            "saved res:// script path",
        ),
        (
            json!({"class": "Curve", "path": "res://x.tres"}),
            "unknown resource spec field \"path\"",
        ),
        (
            json!({"class": "Curve", "properties": [1]}),
            "properties must be an object",
        ),
        (
            json!({"class": "Curve", "properties": {"n": 9007199254740993_i64}}),
            "at /properties/n",
        ),
        (
            json!({"class": "Curve", "properties": {"v": {"$variant": {"type": "Vector9", "value": []}}}}),
            "at /properties/v",
        ),
        (
            json!({"class": "Curve", "properties": {"r": {"$ref": "res://../x.tres"}}}),
            "at /properties/r",
        ),
        (
            json!({"class": "Curve", "properties": {"x": {"$resource": {"class": "Curve", "script": "res://a.gd"}}}}),
            "at /properties/x/$resource",
        ),
    ] {
        let error = CreateSpec::from_json(&value).unwrap_err().to_string();
        assert!(error.contains(expected), "{value}: {error}");
    }
}

#[test]
fn destination_must_be_new_tres_inside_project() {
    let (dir, workspace) = project(&[
        ("weapons/rifle.tres", "[gd_resource format=3]\n"),
        ("notes.txt", ""),
    ]);
    let out = |path: &str| ResPath::parse(path).unwrap();
    assert_eq!(
        resource::check_destination(&workspace, &out("res://weapons/shotgun.tres")).unwrap(),
        dir.path().join("weapons/shotgun.tres")
    );
    assert_eq!(
        resource::check_destination(&workspace, &out("res://top.tres")).unwrap(),
        dir.path().join("top.tres")
    );
    fs::create_dir(dir.path().join("elsewhere")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(dir.path().join("elsewhere"), dir.path().join("linked")).unwrap();
    let mut cases = vec![
        ("res://weapons/shotgun.res", "must name a .tres file"),
        ("res://weapons/shotgun", "must name a .tres file"),
        ("res://weapons/.tres", "must name a .tres file"),
        ("res://weapons/.shotgun.tres", "hidden files are ignored"),
        (
            "res://weapons/rifle.tres",
            "already exists; gdkit never overwrites",
        ),
        (
            "res://armor/vest.tres",
            "res://armor does not exist; create the directory first",
        ),
        (
            "res://notes.txt/vest.tres",
            "res://notes.txt is not a directory",
        ),
    ];
    #[cfg(unix)]
    cases.push(("res://linked/vest.tres", "res://linked is a symlink"));
    for (path, expected) in cases {
        let error = resource::check_destination(&workspace, &out(path))
            .unwrap_err()
            .to_string();
        assert!(error.starts_with(&format!("--out {path}: ")), "{error}");
        assert!(error.contains(expected), "{path}: {error}");
    }
    assert!(
        !dir.path().join("armor").exists(),
        "no directory is created"
    );
}

#[test]
fn references_must_exist_in_the_project() {
    let (_dir, workspace) = project(&[("weapon.gd", ""), ("icon.png", "")]);
    let spec = |value: Value| CreateSpec::from_json(&value).unwrap();
    resource::check_references(
        &workspace,
        &spec(json!({"script": "res://weapon.gd", "properties": {"icon": {"$ref": "res://icon.png"}}})),
    )
    .unwrap();
    let error = resource::check_references(
        &workspace,
        &spec(json!({"script": "res://gone.gd", "properties": {
            "icon": {"$ref": "res://icons/missing.png"},
            "ammo": {"$resource": {"script": "res://gone.gd", "properties": {"sfx": {"$ref": "res://a.wav"}}}},
        }})),
    )
    .unwrap_err()
    .to_string();
    assert_eq!(
        error,
        "the spec names files that do not exist: res://a.wav, res://gone.gd, res://icons/missing.png"
    );
}

#[test]
#[ignore = "scaffold"]
fn echo_mismatch_is_a_verify_failure_and_nothing_is_published() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn warnings_do_not_fail_create() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn staged_file_is_removed_on_every_failure_path() {
    todo!()
}

fn real_engine() -> Engine {
    Engine {
        executable: std::env::var_os("GDKIT_TEST_GODOT")
            .expect("set GDKIT_TEST_GODOT to opt in")
            .into(),
        version: "real".into(),
        fingerprint: "test".into(),
        source: SelectionSource::Environment,
    }
}

/// `godot --headless --editor --quiet --import`, as check and api tests do.
fn import(engine: &Engine, root: &Path) {
    let mut import = Invocation::new(engine, root, Duration::from_secs(120));
    import.engine_args = vec!["--editor".into(), "--quiet".into(), "--import".into()];
    let (captured, _) = runner::run_engine(&import).unwrap();
    assert!(captured.success(), "import failed: {captured:?}");
    assert!(root.join(".godot/global_script_class_cache.cfg").is_file());
}

const WEAPON: &str = r#"class_name WeaponDefinition
extends Resource
enum Kind { MELEE, RANGED = 5 }
@export var name: String = "gun"
@export var kind: Kind = Kind.RANGED
@export var damage: int = 3
@export_range(0, 10, 0.5) var spread: float = 1.5
@export var offset: Vector3 = Vector3(1, 2, 3)
@export var tags: Array[String] = ["a"]
@export var textures: Array[Texture2D] = []
@export var kinds: Array[Kind] = []
@export var stats: Dictionary[String, int] = {"hp": 3}
@export var icon: Texture2D
@export var ammo: AmmoDefinition
@export_flags("a", "b") var flags := 0
@export var callback: Callable
var not_exported := 1
"#;

const AMMO: &str = "class_name AmmoDefinition\nextends Resource\n@export var count := 6\n";

#[test]
#[ignore = "requires GDKIT_TEST_GODOT"]
fn real_engine_schema_reports_fields_hints_enums_and_typed_arrays_from_hint_string() {
    let engine = real_engine();
    let (dir, workspace) = project(&[("weapon.gd", WEAPON), ("ammo.gd", AMMO)]);

    // Before an import, AmmoDefinition does not resolve: the target fails with the hint.
    match resource::schema(
        &workspace,
        &engine,
        &script("res://weapon.gd"),
        DEFAULT_DEADLINE,
    ) {
        Err(Error::Harness { stage, message, .. }) => {
            assert_eq!(stage, "target");
            assert!(message.contains("has not been imported"), "{message}");
        }
        other => panic!("{other:?}"),
    }

    import(&engine, dir.path());
    let before = snapshot(dir.path());
    let schema = resource::schema(
        &workspace,
        &engine,
        &script("res://weapon.gd"),
        DEFAULT_DEADLINE,
    )
    .unwrap();
    assert_eq!(snapshot(dir.path()), before, "schema writes nothing");
    assert_eq!(schema.class, "Resource");
    assert_eq!(schema.script_class.as_deref(), Some("WeaponDefinition"));
    let field = |name: &str| {
        schema
            .fields
            .iter()
            .find(|f| f.name == name)
            .unwrap_or_else(|| panic!("no field {name}"))
    };
    assert!(schema.fields.iter().all(|f| f.name != "script"));
    assert!(schema.fields.iter().all(|f| f.name != "not_exported"));

    let kind = field("kind");
    assert_eq!(kind.variant_type, VariantType::Int);
    assert_eq!(kind.enum_name.as_deref(), Some("WeaponDefinition.Kind"));
    assert_eq!(
        serde_json::to_value(&kind.enum_choices).unwrap(),
        json!([{"name": "Melee", "value": 0}, {"name": "Ranged", "value": 5}])
    );
    assert_eq!(kind.default, json!(5));

    let spread = field("spread");
    assert_eq!(spread.hint.as_deref(), Some("range"));
    assert_eq!(spread.hint_string.as_deref(), Some("0.0,10.0,0.5"));
    assert_eq!(spread.default, json!(1.5));
    assert_eq!(
        field("offset").default,
        json!({"$variant": {"type": "Vector3", "value": [1.0, 2.0, 3.0]}})
    );

    let tags = field("tags");
    assert_eq!(
        tags.element.as_ref().unwrap().variant_type,
        VariantType::String
    );
    assert_eq!(
        tags.default,
        json!({"$variant": {"type": "Array", "element": "String", "value": ["a"]}})
    );
    let textures = field("textures");
    assert_eq!(
        textures.element.as_ref().unwrap().class_name.as_deref(),
        Some("Texture2D")
    );
    assert_eq!(
        textures.default,
        json!([]),
        "Object-typed arrays encode loosely"
    );
    let kinds = field("kinds").element.as_ref().unwrap();
    assert_eq!(kinds.variant_type, VariantType::Int);
    assert_eq!(kinds.enum_choices.len(), 2);

    let stats = field("stats");
    assert_eq!(
        stats.key.as_ref().unwrap().variant_type,
        VariantType::String
    );
    assert_eq!(stats.value.as_ref().unwrap().variant_type, VariantType::Int);
    assert_eq!(stats.default, json!({"hp": 3}));

    assert_eq!(field("icon").class_name.as_deref(), Some("Texture2D"));
    assert_eq!(field("ammo").class_name.as_deref(), Some("AmmoDefinition"));
    assert_eq!(field("flags").enum_choices.len(), 2);
    assert!(field("callback").unsupported.is_some());

    // A native class, and targets that are not instantiable Resources.
    let material = resource::schema(
        &workspace,
        &engine,
        &class("StandardMaterial3D"),
        DEFAULT_DEADLINE,
    )
    .unwrap();
    let transparency = material
        .fields
        .iter()
        .find(|f| f.name == "transparency")
        .unwrap();
    assert_eq!(transparency.enum_choices[1].name, "Alpha");
    assert_eq!(transparency.enum_choices[1].value, json!(1));
    for (target, expected) in [
        (class("Node"), "is not a Resource"),
        (class("NoSuchClass"), "Unknown class"),
        (class("WeaponDefinition"), "use --script res://weapon.gd"),
        (script("res://missing.gd"), "No script at"),
    ] {
        match resource::schema(&workspace, &engine, &target, DEFAULT_DEADLINE) {
            Err(Error::Harness { stage, message, .. }) => {
                assert_eq!(stage, "target");
                assert!(message.contains(expected), "{target:?}: {message}");
            }
            other => panic!("{target:?}: {other:?}"),
        }
    }
}

/// Every file under `root` with its contents, for "wrote nothing" checks.
fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let relative = path.strip_prefix(root).unwrap().to_owned();
                out.push((relative, fs::read(&path).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

#[test]
#[ignore = "scaffold"]
fn real_engine_round_trips_every_variant_type() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn real_engine_create_nested_resources_and_refs() {
    todo!()
}
