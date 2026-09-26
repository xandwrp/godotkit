// Acceptance tests for gdproject::api. Offline unless prefixed real_engine_.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gdproject::api::{self, CacheUse, DEFAULT_DUMP_DEADLINE};
use gdproject::config::SelectionSource;
use gdproject::engine::Engine;
use gdproject::{Error, Workspace};
use serde_json::{Value, json};

fn gdview_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../gdview/tests/fixtures/api")
        .join(name);
    fs::read_to_string(path).unwrap()
}

/// The real (trimmed) 4.7.2 outputs, as the fake engine's files.
fn real_outputs() -> Value {
    json!({
        "--dump-extension-api-with-docs": {
            "files": {"extension_api.json": gdview_fixture("extension_api.json")}
        },
        "--doctool": {
            "files": {
                "modules/gdscript/doc_classes/@GDScript.xml": gdview_fixture("doctool/@GDScript.xml"),
                "doc/classes/Node.xml": gdview_fixture("doctool/Node.xml"),
                "doc/classes/CharacterBody3D.xml": gdview_fixture("doctool/CharacterBody3D.xml"),
            }
        }
    })
}

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

fn project() -> (tempfile::TempDir, Workspace) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("project.godot"), "config_version=5\n").unwrap();
    let workspace = Workspace::open(dir.path()).unwrap();
    (dir, workspace)
}

#[test]
fn load_populates_cache_then_reuses_it() {
    let (_engine_dir, engine) = fake(real_outputs());
    let (_dir, workspace) = project();
    let (index, used) = api::load_native(&workspace, &engine, DEFAULT_DUMP_DEADLINE).unwrap();
    assert_eq!(used, CacheUse::Miss);
    assert_eq!(index.engine_version, "4.7.2.stable.arch_linux");
    assert!(index.gdscript.functions.iter().any(|f| f.name == "range"));
    assert!(workspace.api_cache_path().is_file());
    assert_eq!(invocations(&engine).len(), 2);

    let (cached, used) = api::load_native(&workspace, &engine, DEFAULT_DUMP_DEADLINE).unwrap();
    assert_eq!(used, CacheUse::Hit);
    assert_eq!(cached, index);
    assert_eq!(invocations(&engine).len(), 2, "a hit runs no engine");
}

#[test]
fn cache_key_changes_with_engine_fingerprint() {
    let (_engine_dir, mut engine) = fake(real_outputs());
    let (_dir, workspace) = project();
    api::load_native(&workspace, &engine, DEFAULT_DUMP_DEADLINE).unwrap();
    engine.fingerprint = "rebuilt".into();
    let (_, used) = api::load_native(&workspace, &engine, DEFAULT_DUMP_DEADLINE).unwrap();
    assert_eq!(used, CacheUse::Miss);
    assert_eq!(invocations(&engine).len(), 4);
    let (_, used) = api::load_native(&workspace, &engine, DEFAULT_DUMP_DEADLINE).unwrap();
    assert_eq!(used, CacheUse::Hit);
}

#[test]
fn failed_or_empty_engine_runs_are_errors_and_are_not_cached() {
    let doctool = real_outputs()["--doctool"].clone();
    let cases = [
        json!({"--dump-extension-api-with-docs": {"mode": "crash", "stderr": "boom\n"}}),
        json!({"--dump-extension-api-with-docs": {"files": {}}}),
        json!({"--dump-extension-api-with-docs": {"files": {"extension_api.json": "{"}}}),
        json!({"--doctool": {"mode": "crash"}}),
        json!({"--doctool": {"files": {}}}),
        json!({"--doctool": {"files": {"doc/classes/Bad.xml": "<class name=\"Bad\"><methods></class>"}}}),
        json!({"--dump-extension-api-with-docs": {"exit": 3}, "--doctool": doctool}),
    ];
    for scenario in cases {
        let (_engine_dir, engine) = fake(scenario.clone());
        let (_dir, workspace) = project();
        let error = api::load_native(&workspace, &engine, DEFAULT_DUMP_DEADLINE).unwrap_err();
        assert!(
            matches!(error, Error::EngineRun { .. } | Error::View(_)),
            "{scenario}: {error:?}"
        );
        assert!(!workspace.api_cache_path().exists(), "{scenario}");
    }

    let (_engine_dir, engine) = fake(
        json!({"--dump-extension-api-with-docs": {"mode": "crash", "stderr": "first\nboom\n"}}),
    );
    let (_dir, workspace) = project();
    let message = api::load_native(&workspace, &engine, DEFAULT_DUMP_DEADLINE)
        .unwrap_err()
        .to_string();
    assert!(
        message.contains("--dump-extension-api-with-docs") && message.contains("boom"),
        "{message}"
    );

    let (_engine_dir, engine) = fake(json!({"--dump-extension-api-with-docs": {"mode": "hang"}}));
    let (_dir, workspace) = project();
    let error = api::load_native(&workspace, &engine, Duration::from_millis(300)).unwrap_err();
    assert!(matches!(error, Error::Timeout { .. }), "{error:?}");
    assert!(!workspace.api_cache_path().exists());
}

#[test]
fn corrupt_or_stale_cache_is_replaced_not_fatal() {
    let (_engine_dir, engine) = fake(real_outputs());
    let (_dir, workspace) = project();
    fs::create_dir_all(workspace.state_dir()).unwrap();
    fs::write(workspace.api_cache_path(), "not json").unwrap();
    let (index, used) = api::load_native(&workspace, &engine, DEFAULT_DUMP_DEADLINE).unwrap();
    assert_eq!(used, CacheUse::Miss);

    let mut record: Value =
        serde_json::from_slice(&fs::read(workspace.api_cache_path()).unwrap()).unwrap();
    assert_eq!(record["key"]["engine_fingerprint"], "fake");
    record["key"]["schema_version"] = json!(0);
    fs::write(workspace.api_cache_path(), record.to_string()).unwrap();
    let (again, used) = api::load_native(&workspace, &engine, DEFAULT_DUMP_DEADLINE).unwrap();
    assert_eq!(used, CacheUse::Miss);
    assert_eq!(again, index);
    let (_, used) = api::load_native(&workspace, &engine, DEFAULT_DUMP_DEADLINE).unwrap();
    assert_eq!(used, CacheUse::Hit);
    let leftovers: Vec<_> = fs::read_dir(workspace.state_dir())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .filter(|name| name.to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn dump_runs_without_path_in_a_bare_directory() {
    let (_engine_dir, engine) = fake(real_outputs());
    let (dir, workspace) = project();
    api::load_native(&workspace, &engine, DEFAULT_DUMP_DEADLINE).unwrap();
    let runs = invocations(&engine);
    assert_eq!(
        runs[0],
        [
            "--headless",
            "--no-header",
            "--dump-extension-api-with-docs"
        ]
    );
    assert_eq!(&runs[1][..3], ["--headless", "--no-header", "--doctool"]);
    assert!(Path::new(&runs[1][3]).is_absolute());
    assert!(runs.iter().flatten().all(|arg| arg != "--path"));
    assert!(!dir.path().join("extension_api.json").exists());
    assert!(!Path::new(&runs[1][3]).exists(), "scratch is removed");

    let (_engine_dir, engine) = fake(real_outputs());
    let index = api::load_native_standalone(&engine, DEFAULT_DUMP_DEADLINE).unwrap();
    assert!(index.classes.contains_key("Node"));
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT"]
fn real_engine_native_index_has_builtins_utilities_gdscript_and_docs() {
    let executable = std::env::var_os("GDKIT_TEST_GODOT").expect("set GDKIT_TEST_GODOT to opt in");
    let engine = Engine {
        executable: executable.into(),
        version: "real".into(),
        fingerprint: "real".into(),
        source: SelectionSource::CommandLine,
    };
    let started = std::time::Instant::now();
    let index = api::load_native_standalone(&engine, DEFAULT_DUMP_DEADLINE).unwrap();
    eprintln!("dump + doctool + parse: {:?}", started.elapsed());
    assert!(index.has_docs);
    assert!(index.classes.len() > 900, "{}", index.classes.len());
    assert!(
        index
            .lookup_member("CharacterBody3D", "move_and_slide")
            .is_some()
    );
    assert!(index.lookup_member("String", "split").is_some());
    assert!(index.utility_function("lerp").is_some());
    for name in ["range", "preload", "len", "@export", "@onready"] {
        assert!(index.global(name).is_some(), "{name}");
    }
    let velocity = index.classes["CharacterBody3D"]
        .properties
        .iter()
        .find(|p| p.name == "velocity")
        .unwrap();
    assert_eq!(velocity.default.as_deref(), Some("Vector3(0, 0, 0)"));

    let (dir, workspace) = project();
    let started = std::time::Instant::now();
    api::load_native(&workspace, &engine, DEFAULT_DUMP_DEADLINE).unwrap();
    let (cached, used) = api::load_native(&workspace, &engine, DEFAULT_DUMP_DEADLINE).unwrap();
    eprintln!(
        "cache: {} bytes, miss + hit {:?}",
        fs::metadata(workspace.api_cache_path()).unwrap().len(),
        started.elapsed()
    );
    assert_eq!(used, CacheUse::Hit);
    assert_eq!(cached, index);
    assert!(!dir.path().join("extension_api.json").exists());
}

const PLAYER: &str = "class_name Player\nextends CharacterBody3D\n\n## Hurts.\nfunc take_damage(amount: int) -> void:\n\tpass\n";
const PLAYER_XML: &str = r#"<class name="Player" inherits="CharacterBody3D">
	<brief_description>
		The hero.
	</brief_description>
	<methods>
		<method name="take_damage">
			<return type="void" />
			<param index="0" name="amount" type="int" />
			<description>
				Hurts.
			</description>
		</method>
	</methods>
</class>"#;
const HELPER: &str = "extends Node\n\nclass Inner:\n\tvar speed := 2.0\n\nsignal done(ok: bool)\nconst LIMIT = 3\nfunc run(target: Player, times := 1, ...rest) -> Array[Player]:\n\treturn []\n";

/// A project with two scripts; `imported` adds Godot's class cache.
fn script_project(imported: bool) -> (tempfile::TempDir, Workspace) {
    let (dir, workspace) = project();
    fs::write(dir.path().join("player.gd"), PLAYER).unwrap();
    fs::create_dir(dir.path().join("tools")).unwrap();
    fs::write(dir.path().join("tools/helper.gd"), HELPER).unwrap();
    if imported {
        fs::create_dir_all(dir.path().join(".godot")).unwrap();
        fs::write(
            dir.path().join(".godot/global_script_class_cache.cfg"),
            "list=[]\n",
        )
        .unwrap();
    }
    (dir, workspace)
}

fn script_docs(files: Value, stderr: &str) -> Value {
    let mut scenario = real_outputs();
    scenario["--gdscript-docs"] = json!({"files": files, "stderr": stderr});
    scenario
}

fn docs_runs(engine: &Engine) -> Vec<Vec<String>> {
    invocations(engine)
        .into_iter()
        .filter(|args| args.iter().any(|arg| arg == "--gdscript-docs"))
        .collect()
}

#[test]
fn imported_projects_are_documented_in_place_and_merge_over_native_classes() {
    let (_engine_dir, engine) = fake(script_docs(json!({"Player.xml": PLAYER_XML}), ""));
    let (dir, workspace) = script_project(true);
    let (native, _) = api::load_native(&workspace, &engine, DEFAULT_DUMP_DEADLINE).unwrap();
    let project =
        api::ProjectApi::with_scripts(native, &workspace, &engine, DEFAULT_DUMP_DEADLINE).unwrap();
    assert_eq!(project.scripts.source, api::ScriptDocsSource::Project);
    assert!(!project.scripts.cached);

    let runs = docs_runs(&engine);
    assert_eq!(runs.len(), 1);
    let args = &runs[0];
    let path = args.iter().position(|arg| arg == "--path").unwrap();
    assert_eq!(
        Path::new(&args[path + 1]),
        dir.path().canonicalize().unwrap()
    );
    let docs = args
        .iter()
        .position(|arg| arg == "--gdscript-docs")
        .unwrap();
    assert_eq!(args[docs + 1], "res://");
    assert!(
        args.iter()
            .any(|arg| arg.starts_with("res://") && arg.ends_with(".tscn"))
    );
    assert!(!args.iter().any(|arg| arg == "--editor"));

    let player = &project.index.classes["Player"];
    assert_eq!(player.api_type, "script");
    let origin = player.script.as_ref().unwrap();
    assert_eq!(
        (origin.path.as_str(), origin.line, origin.from_engine),
        ("res://player.gd", Some(1), true)
    );
    assert_eq!(player.brief.as_deref(), Some("The hero."));
    let hit = project
        .index
        .lookup_member("Player", "take_damage")
        .unwrap();
    let gdview::api::Member::Method(method) = hit.member else {
        panic!()
    };
    assert_eq!(method.line, Some(5));
    assert_eq!(method.description.as_deref(), Some("Hurts."));
    let inherited = project
        .index
        .lookup_member("Player", "move_and_slide")
        .unwrap();
    assert_eq!(inherited.declaring_class.name, "CharacterBody3D");
    assert!(!dir.path().join("extension_api.json").exists());
}

#[test]
fn undocumented_scripts_fall_back_to_source_with_the_engine_reason() {
    let stderr = "SCRIPT ERROR: Parse Error: Identifier \"Player\" not declared in the current scope.\n   at: GDScript::reload (res://tools/helper.gd:8)\nERROR: Failed to load script \"res://tools/helper.gd\" with error \"Parse error\".\n   at: load (modules/gdscript/gdscript_resource_format.cpp:46)\n";
    let (_engine_dir, engine) = fake(script_docs(json!({"Player.xml": PLAYER_XML}), stderr));
    let (_dir, workspace) = script_project(true);
    let (classes, docs) = api::load_scripts(&workspace, &engine, DEFAULT_DUMP_DEADLINE).unwrap();
    assert_eq!(docs.source, api::ScriptDocsSource::Project);
    assert_eq!(docs.fallbacks.len(), 1, "{:?}", docs.fallbacks);
    assert_eq!(docs.fallbacks[0].path, "res://tools/helper.gd");
    assert!(
        docs.fallbacks[0]
            .reason
            .contains("Identifier \"Player\" not declared"),
        "{}",
        docs.fallbacks[0].reason
    );

    let helper = classes
        .iter()
        .find(|c| c.name == "res://tools/helper.gd")
        .unwrap();
    assert!(!helper.script.as_ref().unwrap().from_engine);
    assert_eq!(helper.parent.as_deref(), Some("Node"));
    let run = helper.methods.iter().find(|m| m.name == "run").unwrap();
    assert_eq!(run.line, Some(8));
    assert!(run.is_vararg);
    assert_eq!(run.return_type.display(), "Array[Player]");
    let types: Vec<_> = run.arguments.iter().map(|a| a.type_.display()).collect();
    assert_eq!(types, ["Player", "Variant"]);
    assert_eq!(run.arguments[1].default.as_deref(), Some("1"));
    assert!(helper.signals.iter().any(|s| s.name == "done"));
    assert!(helper.constants.iter().any(|c| c.name == "LIMIT"));
    let inner = classes
        .iter()
        .find(|c| c.name == "res://tools/helper.gd.Inner")
        .unwrap();
    assert_eq!(inner.properties[0].name, "speed");
    assert_eq!(inner.script.as_ref().unwrap().line, Some(3));
}

#[test]
fn not_imported_or_locked_projects_are_documented_from_a_copy() {
    let (_engine_dir, engine) = fake(script_docs(json!({"Player.xml": PLAYER_XML}), ""));
    let (dir, workspace) = script_project(false);
    let (_, docs) = api::load_scripts(&workspace, &engine, DEFAULT_DUMP_DEADLINE).unwrap();
    assert_eq!(docs.source, api::ScriptDocsSource::ScriptCopy);
    let args = docs_runs(&engine).pop().unwrap();
    let path = args.iter().position(|arg| arg == "--path").unwrap();
    assert_ne!(
        Path::new(&args[path + 1]),
        dir.path().canonicalize().unwrap()
    );

    let (_engine_dir, engine) = fake(script_docs(json!({"Player.xml": PLAYER_XML}), ""));
    let (_dir, workspace) = script_project(true);
    let lock = workspace.lock().unwrap();
    let (_, docs) = api::load_scripts(&workspace, &engine, DEFAULT_DUMP_DEADLINE).unwrap();
    assert_eq!(docs.source, api::ScriptDocsSource::ScriptCopy);
    drop(lock);
}

#[test]
fn a_failed_docs_run_falls_back_everything_and_is_not_cached() {
    let mut scenario = real_outputs();
    scenario["--gdscript-docs"] = json!({"mode": "crash", "stderr": "ERROR: exploded\n"});
    let (_engine_dir, engine) = fake(scenario);
    let (_dir, workspace) = script_project(true);
    let (classes, docs) = api::load_scripts(&workspace, &engine, DEFAULT_DUMP_DEADLINE).unwrap();
    assert_eq!(docs.source, api::ScriptDocsSource::Source);
    assert_eq!(docs.fallbacks.len(), 2);
    assert!(
        docs.fallbacks.iter().all(|f| f.reason.contains("exploded")),
        "{:?}",
        docs.fallbacks
    );
    assert!(classes.iter().any(|c| c.name == "Player"));
    assert!(!workspace.state_dir().join("api-scripts.json").exists());
    api::load_scripts(&workspace, &engine, DEFAULT_DUMP_DEADLINE).unwrap();
    assert_eq!(docs_runs(&engine).len(), 2, "failures are retried");
}

#[test]
fn script_docs_are_cached_until_a_script_changes() {
    let (_engine_dir, engine) = fake(script_docs(json!({"Player.xml": PLAYER_XML}), ""));
    let (dir, workspace) = script_project(true);
    let (first, docs) = api::load_scripts(&workspace, &engine, DEFAULT_DUMP_DEADLINE).unwrap();
    assert!(!docs.cached);
    let (second, docs) = api::load_scripts(&workspace, &engine, DEFAULT_DUMP_DEADLINE).unwrap();
    assert!(docs.cached);
    assert_eq!(first, second);
    assert_eq!(docs_runs(&engine).len(), 1);
    fs::write(
        dir.path().join("player.gd"),
        format!("{PLAYER}\nvar hp := 3\n"),
    )
    .unwrap();
    let (_, docs) = api::load_scripts(&workspace, &engine, DEFAULT_DUMP_DEADLINE).unwrap();
    assert!(!docs.cached);
    assert_eq!(docs_runs(&engine).len(), 2);
}

#[test]
fn autoload_scripts_answer_to_their_autoload_name() {
    let helper_xml = r#"<class name="Helpers" inherits="Node"><brief_description>Tools.</brief_description></class>"#;
    let inner_xml = r#"<class name="Helpers.Inner" inherits="RefCounted"></class>"#;
    let (_engine_dir, engine) = fake(script_docs(
        json!({"Player.xml": PLAYER_XML, "Helpers.xml": helper_xml, "Helpers.Inner.xml": inner_xml}),
        "",
    ));
    let (dir, workspace) = script_project(true);
    fs::write(
        dir.path().join("project.godot"),
        "config_version=5\n\n[autoload]\n\nHelpers=\"*res://tools/helper.gd\"\n",
    )
    .unwrap();
    let (classes, docs) = api::load_scripts(&workspace, &engine, DEFAULT_DUMP_DEADLINE).unwrap();
    assert!(docs.fallbacks.is_empty(), "{:?}", docs.fallbacks);
    let helpers = classes.iter().find(|c| c.name == "Helpers").unwrap();
    assert_eq!(
        helpers.script.as_ref().unwrap().path,
        "res://tools/helper.gd"
    );
    assert!(helpers.script.as_ref().unwrap().from_engine);
    assert!(classes.iter().any(|c| c.name == "Helpers.Inner"));
}

#[test]
fn a_uid_main_scene_does_not_abort_the_docs_run_and_scriptless_projects_skip_it() {
    let (_engine_dir, engine) = fake(script_docs(json!({"Player.xml": PLAYER_XML}), ""));
    let (dir, workspace) = script_project(true);
    fs::write(
        dir.path().join("project.godot"),
        "config_version=5\n[application]\nrun/main_scene=\"uid://abc\"\n",
    )
    .unwrap();
    let (classes, docs) = api::load_scripts(&workspace, &engine, DEFAULT_DUMP_DEADLINE).unwrap();
    assert_eq!(docs.source, api::ScriptDocsSource::Project);
    let player = classes.iter().find(|c| c.name == "Player").unwrap();
    assert!(
        player.script.as_ref().unwrap().from_engine,
        "the run did not abort"
    );
    // Only the script the scenario leaves undocumented falls back.
    let paths: Vec<_> = docs.fallbacks.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["res://tools/helper.gd"]);

    let (_engine_dir, engine) = fake(real_outputs());
    let (_dir, workspace) = project();
    let (classes, docs) = api::load_scripts(&workspace, &engine, DEFAULT_DUMP_DEADLINE).unwrap();
    assert!(classes.is_empty());
    assert_eq!(docs.source, api::ScriptDocsSource::NoScripts);
    assert!(invocations(&engine).is_empty());
}

/// Every file under `root` except gdkit's own state, with its bytes.
fn snapshot(root: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut std::collections::BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let relative = path.strip_prefix(root).unwrap().to_owned();
            if relative == Path::new(".godot/gdkit") {
                continue;
            }
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.insert(relative, fs::read(&path).unwrap());
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    walk(root, root, &mut out);
    out
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT"]
fn real_engine_script_docs_leave_the_project_untouched() {
    let executable = std::env::var_os("GDKIT_TEST_GODOT").expect("set GDKIT_TEST_GODOT to opt in");
    let engine = Engine {
        executable: executable.into(),
        version: "real".into(),
        fingerprint: "real".into(),
        source: SelectionSource::CommandLine,
    };
    let (dir, workspace) = project();
    fs::write(dir.path().join("player.gd"), "## The hero.\nclass_name Player\nextends CharacterBody3D\n\n## Hurts by [param amount].\nfunc take_damage(amount: int) -> void:\n\tpass\n").unwrap();
    fs::write(
        dir.path().join("stats.tres"),
        "[gd_resource type=\"Resource\" format=3]\n\n[resource]\n",
    )
    .unwrap();
    fs::write(dir.path().join("helper.gd"), "extends Node\n\nconst STATS = preload(\"res://stats.tres\")\n\nfunc spawn(target: Player, spread := 0.5) -> Player:\n\treturn target\n").unwrap();
    let mut import = gdproject::runner::Invocation::new(&engine, dir.path(), DEFAULT_DUMP_DEADLINE);
    import.engine_args = vec!["--editor".into(), "--quiet".into(), "--import".into()];
    gdproject::runner::run_engine(&import).unwrap();
    assert!(
        dir.path()
            .join(".godot/global_script_class_cache.cfg")
            .is_file()
    );
    // An unresolvable uid main scene would abort without the placeholder scene.
    let mut settings = fs::read_to_string(dir.path().join("project.godot")).unwrap();
    settings.push_str("\n[application]\nrun/main_scene=\"uid://gdkitmissing\"\n");
    fs::write(dir.path().join("project.godot"), settings).unwrap();

    let before = snapshot(dir.path());
    let (classes, docs) = api::load_scripts(&workspace, &engine, DEFAULT_DUMP_DEADLINE).unwrap();
    assert_eq!(docs.source, api::ScriptDocsSource::Project);
    assert!(docs.fallbacks.is_empty(), "{:?}", docs.fallbacks);
    let after = snapshot(dir.path());
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>()
    );
    for (path, bytes) in &before {
        assert!(after[path] == *bytes, "{} changed", path.display());
    }

    let player = classes.iter().find(|c| c.name == "Player").unwrap();
    assert_eq!(player.brief.as_deref(), Some("The hero."));
    let damage = player
        .methods
        .iter()
        .find(|m| m.name == "take_damage")
        .unwrap();
    assert_eq!(
        damage.description.as_deref(),
        Some("Hurts by [param amount].")
    );
    assert_eq!(damage.line, Some(6));
    let helper = classes
        .iter()
        .find(|c| c.name == "res://helper.gd")
        .unwrap();
    assert!(helper.script.as_ref().unwrap().from_engine);
    let spawn = helper.methods.iter().find(|m| m.name == "spawn").unwrap();
    let types: Vec<_> = spawn.arguments.iter().map(|a| a.type_.display()).collect();
    assert_eq!(types, ["Player", "float"], "the engine infers `:=` types");
}
