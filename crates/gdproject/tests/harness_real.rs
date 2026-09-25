//! Real-engine tests for embedded harness sources and for diagnostics parsing of
//! verbatim engine output. All require GDKIT_TEST_GODOT (Godot 4.7.2 verified).
//!
//! - `real_engine_bootstrap_hands_off_init_initialize_and_process_scripts`
//! - `real_engine_check_harness_compiles_unreferenced_shaders`
//! - `real_engine_shutdown_leaks_are_noise_and_resource_errors_are_located`
//! - `real_engine_gdextension_paths_in_the_copy_are_rewritten_to_res`

use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::time::Duration;

use gdproject::config::SelectionSource;
use gdproject::diagnostics::{self, Diagnostic, Severity};
use gdproject::engine::Engine;
use gdproject::process::Captured;
use gdproject::runner::{self, Harness, Invocation};

const DEADLINE: Duration = Duration::from_secs(60);

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

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (path, contents) in [("project.godot", "config_version=5\n")]
        .iter()
        .chain(files)
    {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    dir
}

fn run(
    dir: &Path,
    harness: Harness,
    engine_args: &[&str],
    user_args: &[&str],
) -> (Captured, Vec<Diagnostic>) {
    let engine = real_engine();
    let mut invocation = Invocation::new(&engine, dir, DEADLINE);
    invocation.engine_args = engine_args.iter().map(OsString::from).collect();
    invocation.user_args = user_args.iter().map(OsString::from).collect();
    let (captured, diagnostics) = runner::run_harness_raw(&invocation, harness).unwrap();
    assert!(!captured.timed_out && !captured.output_limit_exceeded);
    (captured, diagnostics)
}

fn stdout_lines(captured: &Captured) -> Vec<String> {
    String::from_utf8_lossy(&captured.stdout())
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT"]
fn real_engine_bootstrap_hands_off_init_initialize_and_process_scripts() {
    let dir = project(&[
        (
            "init_only.gd",
            "extends SceneTree\nfunc _init() -> void:\n\tprint(\"INIT\")\n\tquit(0)\n",
        ),
        (
            "initialize_only.gd",
            "extends SceneTree\nfunc _initialize() -> void:\n\tprint(\"INITIALIZE\")\n\tquit(0)\n",
        ),
        (
            "process_only.gd",
            "extends SceneTree\nvar frames := 0\nfunc _process(_delta: float) -> bool:\n\tframes += 1\n\tif frames == 3:\n\t\tprint(\"PROCESS\")\n\t\tquit(0)\n\treturn false\n",
        ),
        (
            "both.gd",
            "extends SceneTree\nfunc _init() -> void:\n\tprint(\"INIT\")\nfunc _initialize() -> void:\n\tprint(\"INITIALIZE\")\n\tquit(0)\n",
        ),
    ]);
    for (script, expected) in [
        ("res://init_only.gd", &["INIT"][..]),
        ("res://initialize_only.gd", &["INITIALIZE"]),
        ("res://process_only.gd", &["PROCESS"]),
        ("res://both.gd", &["INIT", "INITIALIZE"]),
    ] {
        let (captured, diagnostics) = run(dir.path(), Harness::ScriptBootstrap, &[], &[script]);
        let mut lines = vec!["GDKIT_SCRIPT_STARTED".to_owned()];
        lines.extend(expected.iter().map(|s| (*s).to_owned()));
        assert_eq!(stdout_lines(&captured), lines, "{script}");
        assert!(captured.success(), "{script}: {:?}", captured.status);
        assert!(diagnostics.is_empty(), "{script}: {diagnostics:#?}");
        assert!(captured.stderr().is_empty(), "{script}");
    }
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT"]
fn real_engine_check_harness_compiles_unreferenced_shaders() {
    let broken =
        "shader_type spatial;\nvoid fragment() {\n\tALBEDO = vec3(1.0) + undefined_thing;\n}\n";
    let good = "shader_type canvas_item;\nuniform float x = 1.0;\nvoid fragment() {\n\tCOLOR = vec4(x);\n}\n";
    for (files, expect_error) in [
        (vec![("good.gdshader", good)], false),
        (
            vec![("good.gdshader", good), ("broken.gdshader", broken)],
            true,
        ),
    ] {
        let dir = project(&files);
        let scratch = tempfile::tempdir().unwrap();
        let manifest = scratch.path().join("manifest.json");
        let paths: Vec<_> = files.iter().map(|(p, _)| format!("res://{p}")).collect();
        fs::write(&manifest, serde_json::to_vec(&paths).unwrap()).unwrap();
        let extensions = scratch.path().join("extensions.json");
        fs::write(&extensions, "[\"gdshader\"]").unwrap();
        let (captured, diagnostics) = run(
            dir.path(),
            Harness::Check,
            &[],
            &[
                manifest.to_str().unwrap(),
                "project-policy",
                extensions.to_str().unwrap(),
            ],
        );
        assert!(captured.success(), "{:?}", captured.status);
        let envelope = gdproject::protocol::parse_envelope::<serde_json::Value>(
            stdout_lines(&captured).into_iter(),
        )
        .unwrap();
        let payload = envelope.payload.unwrap();
        // A compile error is visible only as diagnostics, not as a load failure.
        assert_eq!(payload["failures"], serde_json::json!([]));
        assert_eq!(payload["counts"]["resources"], files.len());
        if !expect_error {
            assert!(diagnostics.is_empty(), "{diagnostics:#?}");
            continue;
        }
        let shader = diagnostics
            .iter()
            .find(|d| d.code.as_deref() == Some("SHADER_ERROR"))
            .unwrap_or_else(|| panic!("{diagnostics:#?}"));
        assert_eq!(shader.severity, Severity::Error);
        assert_eq!(
            shader.message,
            "Unknown identifier in expression: 'undefined_thing'."
        );
        assert_eq!(shader.frames[0].line, Some(3));
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message == "Shader compilation failed.")
        );
    }
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT"]
fn real_engine_shutdown_leaks_are_noise_and_resource_errors_are_located() {
    let dir = project(&[
        (
            "thing.tres",
            "[gd_resource type=\"Resource\" format=3]\n\n[resource]\n",
        ),
        (
            "leak.gd",
            "extends SceneTree\nstatic var keep: Array = []\nfunc _initialize() -> void:\n\tvar node := Node.new()\n\tnode.set_meta(\"r\", load(\"res://thing.tres\"))\n\tkeep.append(RenderingServer.canvas_item_create())\n\tkeep.append(PhysicsServer2D.body_create())\n\tquit(0)\n",
        ),
        (
            "bad.tres",
            "[gd_resource type=\"Resource\" format=3]\n\n[resource]\nfoo = Vector2(1,\nbar = 3\n",
        ),
        (
            "bad.tscn",
            "[gd_scene format=3]\n\n[node name=\"Main\" type=\"Node\"]\n\n[node name=\"X\" type=\"Node\" parent=\".\"\nfoo = 1\n",
        ),
        ("bad.cfg", "[a]\nx = \ny = (\n"),
        (
            "load_bad.gd",
            "extends SceneTree\nfunc _initialize() -> void:\n\tload(\"res://bad.tres\")\n\tload(\"res://bad.tscn\")\n\tConfigFile.new().load(\"res://bad.cfg\")\n\tquit(0)\n",
        ),
    ]);
    let (captured, leaks) = run(
        dir.path(),
        Harness::ScriptBootstrap,
        &[],
        &["res://leak.gd"],
    );
    assert!(captured.success());
    for needle in [
        "RID of type \"CanvasItem\" was leaked.",
        "ObjectDB instances were leaked at exit",
        "resources still in use at exit",
        "RID allocations of type",
    ] {
        assert!(
            leaks.iter().any(|d| d.message.contains(needle)),
            "{needle}: {leaks:#?}"
        );
    }
    assert!(leaks.iter().all(|d| d.is_shutdown_noise), "{leaks:#?}");

    let (_, located) = run(
        dir.path(),
        Harness::ScriptBootstrap,
        &[],
        &["res://load_bad.gd"],
    );
    let find = |prefix: &str| {
        located
            .iter()
            .find(|d| d.message.starts_with(prefix))
            .unwrap_or_else(|| panic!("{prefix}: {located:#?}"))
    };
    for (prefix, resource, line) in [
        ("res://bad.tres:5 - Parse Error: ", "res://bad.tres", 5),
        (
            "Parse Error: Parse error. [Resource file ",
            "res://bad.tscn",
            7,
        ),
        (
            "ConfigFile parse error at res://bad.cfg:2: ",
            "res://bad.cfg",
            2,
        ),
    ] {
        let diagnostic = find(prefix);
        assert_eq!(diagnostic.resource.as_deref(), Some(resource), "{prefix}");
        assert_eq!(diagnostic.line, Some(line), "{prefix}");
    }
    assert_eq!(
        find("Failed loading resource: res://bad.tres")
            .resource
            .as_deref(),
        Some("res://load_bad.gd"),
        "a loader's res:// frame outranks the mentioned path"
    );
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT"]
fn real_engine_gdextension_paths_in_the_copy_are_rewritten_to_res() {
    let dir = project(&[
        ("bin/libx.so", "not a shared library\n"),
        (
            "x.gdextension",
            "[configuration]\nentry_symbol = \"x_init\"\ncompatibility_minimum = \"4.1\"\n\n[libraries]\nlinux.x86_64 = \"res://bin/libx.so\"\nlinux.arm64 = \"res://bin/libx.so\"\nwindows.x86_64 = \"res://bin/libx.so\"\nmacos = \"res://bin/libx.so\"\n",
        ),
    ]);
    let engine = real_engine();
    let mut invocation = Invocation::new(&engine, dir.path(), DEADLINE);
    invocation.engine_args = vec!["--quiet".into(), "--editor".into(), "--import".into()];
    let (captured, from_runner) = runner::run_engine(&invocation).unwrap();
    let unrooted = diagnostics::parse(&captured, 0);
    let rooted = diagnostics::parse_rooted(&captured, 0, Some(dir.path()));
    assert_eq!(rooted.len(), unrooted.len());
    // The runner roots diagnostics at the invocation's project directory.
    assert_eq!(from_runner, rooted);
    let library = rooted
        .iter()
        .find(|d| d.message.starts_with("Can't open dynamic library: "))
        .unwrap_or_else(|| panic!("{rooted:#?}"));
    assert!(
        library
            .message
            .starts_with("Can't open dynamic library: res://bin/libx.so."),
        "{}",
        library.message
    );
    assert_eq!(library.resource.as_deref(), Some("res://bin/libx.so"));
    let root = dir.path().display().to_string();
    assert!(rooted.iter().all(|d| !d.message.contains(&root)));
    assert!(unrooted.iter().any(|d| d.message.contains(&root)));
    assert!(rooted.iter().any(|d| {
        d.message == "Can't open GDExtension dynamic library: 'res://x.gdextension'."
            && d.resource.as_deref() == Some("res://x.gdextension")
    }));
}
