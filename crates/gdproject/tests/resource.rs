// Acceptance tests for gdproject::resource. Offline unless prefixed real_engine_.
#![allow(unused)]

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gdproject::config::SelectionSource;
use gdproject::engine::Engine;
use std::collections::BTreeMap;

use gdproject::resource::{self, CreateSpec, DEFAULT_DEADLINE};
use gdproject::runner::{self, Invocation};
use gdproject::{Error, Workspace};
use gdview::ResPath;
use gdview::variant::{Limits, ResourceTarget, VariantJson, VariantType};
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

fn decode_map(value: Value) -> BTreeMap<String, VariantJson> {
    value
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, value)| {
            (
                name.clone(),
                VariantJson::from_json(value, &Limits::default()).unwrap(),
            )
        })
        .collect()
}

fn tag(ty: &str, value: Value) -> Value {
    json!({"$variant": {"type": ty, "value": value}})
}

#[test]
fn echo_comparison_allows_only_lossless_readings_of_the_spec() {
    let spec = decode_map(json!({
        "damage": 3,
        "spread": 2,
        "offset": tag("Vector2", json!([1, 2.5])),
        "id": "hero",
        "path": "../Player",
        "tags": ["a", "b"],
        "stats": {"hp": 3},
        "ammo": {"$resource": {"script": "res://ammo.gd", "properties": {"count": 6}}},
        "icon": {"$ref": "res://icon.png"},
        "nan": tag("float", json!("nan")),
        "nothing": null,
    }));
    let echo = decode_map(json!({
        "damage": 3,
        "spread": tag("float", json!(2.0)),
        "offset": tag("Vector2", json!([1.0, 2.5])),
        "id": tag("StringName", json!("hero")),
        "path": tag("NodePath", json!("../Player")),
        "tags": {"$variant": {"type": "Array", "element": "String", "value": ["a", "b"]}},
        "stats": {"hp": 3},
        "ammo": {"$resource": {"script": "res://ammo.gd", "properties": {
            "count": 6, "resource_name": "", "resource_local_to_scene": false,
        }}},
        "icon": {"$ref": "res://icon.png"},
        "nan": tag("float", json!("nan")),
        "nothing": null,
        "extra": 1,
    }));
    resource::verify_echo(&spec, &echo).unwrap();

    let mismatch = |spec: Value, echo: Value| {
        resource::verify_echo(&decode_map(spec), &decode_map(echo)).unwrap_err()
    };
    let lossy = mismatch(
        json!({"offset": tag("Vector2", json!([0.1, 2]))}),
        json!({"offset": tag("Vector2", json!([0.10000000149011612, 2.0]))}),
    );
    assert_eq!(lossy.field, "properties.offset[0]");
    assert_eq!(
        lossy.message,
        "the engine stored 0.10000000149011612 where the spec has 0.1"
    );
    for (spec, echo, field) in [
        (
            json!({"damage": 2.5}),
            json!({"damage": 2}),
            "properties.damage",
        ),
        (json!({"damage": 3}), json!({}), "properties.damage"),
        (
            json!({"zero": tag("float", json!("-0.0"))}),
            json!({"zero": tag("float", json!(0.0))}),
            "properties.zero",
        ),
        (json!({"name": "3"}), json!({"name": 3}), "properties.name"),
        (
            json!({"kind": "Ranged"}),
            json!({"kind": 5}),
            "properties.kind",
        ),
        (
            json!({"id": tag("StringName", json!("a"))}),
            json!({"id": "a"}),
            "properties.id",
        ),
        (
            json!({"tags": ["a"]}),
            json!({"tags": ["a", "b"]}),
            "properties.tags",
        ),
        (
            json!({"tags": ["a", "c"]}),
            json!({"tags": ["a", "b"]}),
            "properties.tags[1]",
        ),
        (
            json!({"stats": {"hp": 3}}),
            json!({"stats": {"hp": 4}}),
            r#"properties.stats["hp"]"#,
        ),
        (
            json!({"stats": {"hp": 3}}),
            json!({"stats": {"mp": 3}}),
            "properties.stats",
        ),
        (
            json!({"ammo": {"$resource": {"script": "res://ammo.gd", "properties": {"count": 6}}}}),
            json!({"ammo": {"$resource": {"script": "res://ammo.gd", "properties": {"count": 1}}}}),
            "properties.ammo.properties.count",
        ),
        (
            json!({"ammo": {"$resource": {"script": "res://ammo.gd"}}}),
            json!({"ammo": {"$resource": {"class": "Resource"}}}),
            "properties.ammo",
        ),
        (
            json!({"icon": {"$ref": "res://a.png"}}),
            json!({"icon": {"$ref": "res://b.png"}}),
            "properties.icon",
        ),
        (
            json!({"icon": {"$ref": "res://a.png"}}),
            json!({"icon": null}),
            "properties.icon",
        ),
    ] {
        assert_eq!(mismatch(spec.clone(), echo).field, field, "{spec}");
    }
}

/// A project with the scripts and directories the create tests name.
fn create_project() -> (tempfile::TempDir, Workspace) {
    project(&[("weapon.gd", "extends Resource\n"), ("weapons/.keep", "")])
}

fn weapon_spec() -> CreateSpec {
    CreateSpec::from_json(&json!({
        "script": "res://weapon.gd",
        "properties": {"damage": 3, "offset": tag("Vector2", json!([1, 2.5]))},
    }))
    .unwrap()
}

fn shotgun() -> ResPath {
    ResPath::parse("res://weapons/shotgun.tres").unwrap()
}

/// Every file under `root` except the gdkit state directory.
fn files(root: &Path) -> Vec<PathBuf> {
    snapshot(root)
        .into_iter()
        .map(|(path, _)| path)
        .filter(|path| !path.starts_with(".godot"))
        .collect()
}

#[test]
fn create_stages_beside_the_destination_and_publishes_the_verified_file() {
    let (_engine_dir, engine) = fake(json!({}));
    let (dir, workspace) = create_project();
    let report = resource::create(
        &workspace,
        &engine,
        &weapon_spec(),
        &shotgun(),
        DEFAULT_DEADLINE,
    )
    .unwrap();
    assert_eq!(report.path, shotgun());
    assert_eq!(report.os_path, dir.path().join("weapons/shotgun.tres"));
    assert_eq!(report.target, script("res://weapon.gd"));
    assert_eq!(report.properties_written, 2);
    assert_eq!(
        serde_json::to_value(&report.properties).unwrap(),
        json!({"damage": 3, "offset": tag("Vector2", json!([1.0, 2.5]))})
    );
    assert_eq!(
        fs::read_to_string(&report.os_path).unwrap(),
        "[gd_resource type=\"Resource\" format=3]\n\n[resource]\n"
    );
    assert_eq!(
        files(dir.path()),
        [
            PathBuf::from("project.godot"),
            "weapon.gd".into(),
            "weapons/.keep".into(),
            "weapons/shotgun.tres".into(),
        ]
    );

    let runs = invocations(&engine);
    let run = runs.last().unwrap();
    assert!(run[5].ends_with("/resource_create.gd"), "{run:?}");
    assert!(!run.iter().any(|arg| arg == "--editor"));
    let [dash, spec_file, staged] = &run[6..] else {
        panic!("{run:?}")
    };
    assert_eq!(dash, "--");
    assert!(
        !Path::new(spec_file).starts_with(dir.path()),
        "spec file is scratch"
    );
    assert!(!Path::new(spec_file).exists(), "scratch is removed");
    assert_eq!(
        staged,
        &format!(
            "res://weapons/.shotgun.gdkit-staged-{}.tres",
            std::process::id()
        )
    );
}

#[test]
fn echo_mismatch_is_a_verify_failure_and_nothing_is_published() {
    let (_engine_dir, engine) = fake(json!({"resource_create": {
        "payload": {"echo": {"damage": 3, "offset": tag("Vector2", json!([1.0, 2.0]))}},
    }}));
    let (dir, workspace) = create_project();
    let error = resource::create(
        &workspace,
        &engine,
        &weapon_spec(),
        &shotgun(),
        DEFAULT_DEADLINE,
    )
    .unwrap_err();
    match &error {
        Error::Harness {
            harness,
            stage,
            field,
            message,
        } => {
            assert_eq!(*harness, "resource_create");
            assert_eq!(stage, "verify");
            assert_eq!(field.as_deref(), Some("properties.offset[1]"));
            assert_eq!(message, "the engine stored 2.0 where the spec has 2.5");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        files(dir.path()),
        [
            PathBuf::from("project.godot"),
            "weapon.gd".into(),
            "weapons/.keep".into()
        ]
    );
}

#[test]
fn engine_diagnostics_are_reported_and_do_not_fail_create() {
    let (_engine_dir, engine) = fake(json!({"resource_create": {
        "stderr": "ERROR: AUTOLOAD RAN\nWARNING: deprecated thing\n",
    }}));
    let (_dir, workspace) = create_project();
    let report = resource::create(
        &workspace,
        &engine,
        &weapon_spec(),
        &shotgun(),
        DEFAULT_DEADLINE,
    )
    .unwrap();
    let messages: Vec<_> = report
        .engine_diagnostics
        .iter()
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(messages, ["AUTOLOAD RAN", "deprecated thing"]);
    assert!(report.os_path.is_file());
}

#[test]
fn staged_file_is_removed_on_every_failure_path() {
    let failures = [
        json!({"mode": "error_envelope", "staged": "saved, then verify failed"}),
        json!({"mode": "crash", "staged": "saved, then crashed"}),
        json!({"mode": "no_envelope", "staged": "saved, no envelope"}),
        json!({"mode": "hang", "staged": "saved, then hung"}),
        json!({"payload": null, "staged": "saved, empty payload"}),
        json!({"payload": {"echo": {"damage": 3}}, "staged": "saved, short echo"}),
        json!({"payload": {"echo": {"damage": {"$variant": {}}}}, "staged": "bad echo"}),
        json!({"payload": {"echo": {"damage": 3, "offset": tag("Vector2", json!([1, 2.5]))}},
               "mode": "error_envelope"}),
    ];
    for scenario in failures {
        let (_engine_dir, engine) = fake(json!({"resource_create": scenario.clone()}));
        let (dir, workspace) = create_project();
        let deadline = Duration::from_secs(if scenario["mode"] == "hang" { 1 } else { 20 });
        assert!(
            resource::create(&workspace, &engine, &weapon_spec(), &shotgun(), deadline).is_err(),
            "{scenario}"
        );
        assert_eq!(
            files(dir.path()),
            [
                PathBuf::from("project.godot"),
                "weapon.gd".into(),
                "weapons/.keep".into()
            ],
            "{scenario}"
        );
    }

    // Success reported without a saved file is a save failure.
    let (_engine_dir, engine) = fake(json!({"resource_create": {"mode": "no_envelope",
        "stdout": format!("GDKIT_RESULT:{}\n", json!({"protocol": 1, "harness": "resource_create", "ok": true,
            "payload": {"echo": {"damage": 3, "offset": tag("Vector2", json!([1.0, 2.5]))}}})),
    }}));
    let (dir, workspace) = create_project();
    match resource::create(
        &workspace,
        &engine,
        &weapon_spec(),
        &shotgun(),
        DEFAULT_DEADLINE,
    ) {
        Err(Error::Harness { stage, message, .. }) => {
            assert_eq!(stage, "save");
            assert!(message.contains("wrote no file"), "{message}");
        }
        other => panic!("{other:?}"),
    }
    assert!(!dir.path().join("weapons/shotgun.tres").exists());
}

#[test]
fn invalid_requests_fail_before_the_engine_runs() {
    let (_engine_dir, engine) = fake(json!({}));
    let (dir, workspace) = create_project();
    let spec = |value: Value| CreateSpec::from_json(&value).unwrap();
    let out = |path: &str| ResPath::parse(path).unwrap();
    for (spec, destination, expected) in [
        (
            weapon_spec(),
            out("res://armor/vest.tres"),
            "does not exist; create the directory",
        ),
        (
            weapon_spec(),
            out("res://weapons/shotgun.res"),
            "must name a .tres file",
        ),
        (
            spec(json!({"script": "res://gone.gd"})),
            shotgun(),
            "do not exist: res://gone.gd",
        ),
    ] {
        let error = resource::create(&workspace, &engine, &spec, &destination, DEFAULT_DEADLINE)
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{error}");
    }

    // A leftover staging file is never overwritten.
    let leftover = dir.path().join(format!(
        "weapons/.shotgun.gdkit-staged-{}.tres",
        std::process::id()
    ));
    fs::write(&leftover, "keep me").unwrap();
    let error = resource::create(
        &workspace,
        &engine,
        &weapon_spec(),
        &shotgun(),
        DEFAULT_DEADLINE,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("leftover staging file"), "{error}");
    assert_eq!(fs::read_to_string(&leftover).unwrap(), "keep me");
    fs::remove_file(&leftover).unwrap();

    // Another gdkit holding the workspace lock.
    let lock = workspace.lock().unwrap();
    assert!(matches!(
        resource::create(
            &workspace,
            &engine,
            &weapon_spec(),
            &shotgun(),
            DEFAULT_DEADLINE
        ),
        Err(Error::Locked(_))
    ));
    drop(lock);
    assert!(invocations(&engine).is_empty());
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

/// Everything under `root` except gdkit's own state (`.godot/gdkit`: lock, probe cache).
fn project_files(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    snapshot(root)
        .into_iter()
        .filter(|(path, _)| !path.starts_with(".godot/gdkit"))
        .collect()
}

/// GDScript's spelling of a value's declared type, or `None` for values a
/// typed property cannot hold as written (null, resources).
fn gdscript_type(value: &VariantJson) -> Option<String> {
    Some(match value {
        VariantJson::Nil | VariantJson::Ref(_) | VariantJson::Resource(_) => return None,
        VariantJson::TypedArray { element, .. } => format!("Array[{}]", element.name()),
        other => other.variant_type()?.name().to_owned(),
    })
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT"]
fn real_engine_round_trips_every_variant_type() {
    // Every value the engine's own encoder produced for the golden fixture,
    // assigned once to a Variant property (the .tres round trip alone) and
    // once to a property of its own type (the harness's typed reading).
    // Godot 4.7.2's .tres text writes -0.0 as 0 (floats, vector components,
    // packed arrays), so those values cannot be saved and must be rejected.
    let golden: Value =
        serde_json::from_str(include_str!("fixtures/protocol_golden.json")).unwrap();
    let (unsavable, values): (Vec<&Value>, Vec<&Value>) = golden["payload"]["encoded"]
        .as_array()
        .unwrap()
        .iter()
        .partition(|value| value.to_string().contains("-0.0"));
    assert_eq!(unsavable.len(), 3);
    let mut script = String::from("extends Resource\n");
    let mut properties = serde_json::Map::new();
    for (index, value) in values.into_iter().enumerate() {
        script.push_str(&format!("@export var v{index}: Variant\n"));
        properties.insert(format!("v{index}"), value.clone());
        let decoded = VariantJson::from_json(value, &Limits::default()).unwrap();
        if let Some(ty) = gdscript_type(&decoded) {
            script.push_str(&format!("@export var t{index}: {ty}\n"));
            properties.insert(format!("t{index}"), value.clone());
        }
    }
    let engine = real_engine();
    let (dir, workspace) = project(&[
        ("values.gd", &script),
        (
            "probe.tres",
            "[gd_resource type=\"Resource\" format=3]\n\n[resource]\nresource_name = \"probe\"\n",
        ),
        (
            "scripted_resource.gd",
            "extends Resource\n@export var answer := 42\n",
        ),
    ]);
    let spec =
        CreateSpec::from_json(&json!({"script": "res://values.gd", "properties": properties}))
            .unwrap();
    let out = ResPath::parse("res://values.tres").unwrap();
    let report = resource::create(&workspace, &engine, &spec, &out, DEFAULT_DEADLINE)
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(report.properties_written, properties.len());
    for (name, value) in &properties {
        let stored = &report.properties[name];
        // Inline resources echo their defaults too, and a reload may reorder
        // dictionary pairs; everything else comes back exactly as written.
        let unordered = value.get("$resource").is_some()
            || value.to_string().contains(r#""type":"Dictionary""#);
        if unordered {
            let [spec, echo] = [value, stored].map(|value| {
                BTreeMap::from([(
                    name.clone(),
                    VariantJson::from_json(value, &Limits::default()).unwrap(),
                )])
            });
            resource::verify_echo(&spec, &echo).unwrap();
        } else {
            assert_eq!(stored, value, "{name}");
        }
    }
    assert!(dir.path().join("values.tres").is_file());

    for value in unsavable {
        let spec = CreateSpec::from_json(
            &json!({"script": "res://values.gd", "properties": {"v0": value}}),
        )
        .unwrap();
        let out = ResPath::parse("res://negative_zero.tres").unwrap();
        match resource::create(&workspace, &engine, &spec, &out, DEFAULT_DEADLINE) {
            Err(Error::Harness {
                stage,
                field,
                message,
                ..
            }) => {
                assert_eq!(
                    (stage.as_str(), field.as_deref()),
                    ("verify", Some("properties.v0"))
                );
                assert!(message.contains("sign of -0.0"), "{value}: {message}");
            }
            other => panic!("{value}: {other:?}"),
        }
        assert!(!dir.path().join("negative_zero.tres").exists());
    }
}

const LOADOUT: &str = r#"class_name Loadout
extends Resource
enum Slot { PRIMARY, SIDEARM = 4 }
@export var slot: Slot
@export var weight: float
@export var id: StringName
@export var primary: AmmoDefinition
@export var reserve: Array[AmmoDefinition] = []
@export var by_name: Dictionary[StringName, AmmoDefinition] = {}
@export var palette: Gradient
@export var clamped: float = 0.0:
	set(value):
		clamped = clampf(value, 0.0, 1.0)
"#;

const GRADIENT: &str = "[gd_resource type=\"Gradient\" format=3]\n\n[resource]\noffsets = PackedFloat32Array(0, 0.5, 1)\ncolors = PackedColorArray(0, 0, 0, 1, 1, 0, 0, 1, 1, 1, 1, 1)\n";

/// The loadout project, imported so `class_name` scripts resolve.
fn loadout_project(engine: &Engine) -> (tempfile::TempDir, Workspace) {
    let (dir, workspace) = project(&[
        ("loadout.gd", LOADOUT),
        ("ammo.gd", AMMO),
        ("palette.tres", GRADIENT),
        ("gear/.keep", ""),
    ]);
    import(engine, dir.path());
    (dir, workspace)
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT"]
fn real_engine_create_nested_resources_and_refs() {
    let engine = real_engine();
    let (dir, workspace) = loadout_project(&engine);
    let ammo = |count: i64| json!({"$resource": {"script": "res://ammo.gd", "properties": {"count": count}}});
    let spec = CreateSpec::from_json(&json!({
        "script": "res://loadout.gd",
        "properties": {
            "slot": 4,
            "weight": 3,
            "id": "rifle",
            "primary": ammo(30),
            "reserve": [ammo(10), ammo(20)],
            "by_name": {"slugs": ammo(8)},
            "palette": {"$ref": "res://palette.tres"},
        },
    }))
    .unwrap();
    let out = ResPath::parse("res://gear/rifle.tres").unwrap();
    let report = resource::create(&workspace, &engine, &spec, &out, DEFAULT_DEADLINE)
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        report.properties["weight"],
        json!({"$variant": {"type": "float", "value": 3.0}})
    );
    assert_eq!(
        report.properties["id"],
        json!({"$variant": {"type": "StringName", "value": "rifle"}})
    );
    assert_eq!(
        report.properties["palette"],
        json!({"$ref": "res://palette.tres"})
    );
    assert_eq!(
        report.properties["reserve"][1]["$resource"]["properties"]["count"],
        json!(20)
    );
    let text = fs::read_to_string(dir.path().join("gear/rifle.tres")).unwrap();
    assert!(text.contains("path=\"res://palette.tres\""), "{text}");
    assert!(
        text.contains("reserve = Array[ExtResource("),
        "typed by script:\n{text}"
    );
    assert!(
        text.contains("by_name = Dictionary[StringName, ExtResource("),
        "{text}"
    );
    assert_eq!(text.matches("[sub_resource").count(), 4, "{text}");

    // Each rejection names its field and leaves the project as it was.
    let before = project_files(dir.path());
    for (properties, stage, field, expected) in [
        (
            json!({"slot": "SIDEARM"}),
            "assign",
            "properties.slot",
            "write the enum's value",
        ),
        (
            json!({"clamped": 5}),
            "assign",
            "properties.clamped",
            "a setter or type rejected it",
        ),
        (
            json!({"primary": {"$resource": {"class": "Curve"}}}),
            "assign",
            "properties.primary",
            "Expected AmmoDefinition, got Curve",
        ),
        (
            json!({"reserve": [ammo(1), {"$ref": "res://palette.tres"}]}),
            "assign",
            "properties.reserve[1]",
            "Expected AmmoDefinition",
        ),
        (
            json!({"primary": {"$resource": {"script": "res://ammo.gd", "properties": {"calibre": 9}}}}),
            "assign",
            "properties.primary.properties.calibre",
            "no stored property `calibre`",
        ),
        (
            json!({"weight": 0.1, "palette": {"$resource": {"class": "Gradient", "properties": {"offsets": {"$variant": {"type": "PackedFloat32Array", "value": [0.1]}}}}}}),
            "verify",
            "properties.palette.properties.offsets[0]",
            "the engine stored 0.10000000149011612 where the spec has 0.1",
        ),
    ] {
        let spec =
            CreateSpec::from_json(&json!({"script": "res://loadout.gd", "properties": properties}))
                .unwrap();
        let out = ResPath::parse("res://gear/rejected.tres").unwrap();
        match resource::create(&workspace, &engine, &spec, &out, DEFAULT_DEADLINE) {
            Err(Error::Harness {
                stage: got_stage,
                field: got_field,
                message,
                ..
            }) => {
                assert_eq!(
                    (got_stage.as_str(), got_field.as_deref()),
                    (stage, Some(field)),
                    "{message}"
                );
                assert!(message.contains(expected), "{field}: {message}");
            }
            other => panic!("{properties}: {other:?}"),
        }
        assert_eq!(project_files(dir.path()), before, "{properties}");
    }
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT"]
fn real_engine_create_writes_only_the_destination() {
    let engine = real_engine();
    let (dir, workspace) = loadout_project(&engine);
    let before = project_files(dir.path());
    let spec =
        CreateSpec::from_json(&json!({"script": "res://ammo.gd", "properties": {"count": 12}}))
            .unwrap();
    let out = ResPath::parse("res://gear/box.tres").unwrap();
    resource::create(&workspace, &engine, &spec, &out, DEFAULT_DEADLINE).unwrap();
    let mut after = project_files(dir.path());
    let published = after
        .iter()
        .position(|(path, _)| path == Path::new("gear/box.tres"))
        .expect("published");
    let (_, bytes) = after.remove(published);
    assert_eq!(after, before, "nothing else in the project changed");
    let text = String::from_utf8(bytes).unwrap();
    assert!(
        text.starts_with("[gd_resource type=\"Resource\" script_class=\"AmmoDefinition\""),
        "{text}"
    );
    assert!(text.contains("count = 12"), "{text}");
}
