use std::{fs, path::Path, process::Command};

#[test]
#[ignore = "requires GDKIT_TEST_GODOT pointing to a Godot 4 editor"]
fn reports_engine_cache_policy_and_project_gotchas() {
    let directory = std::env::temp_dir().join(format!("gdkit-doctor-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    fs::write(
        directory.join("project.godot"),
        "config_version=5\n[debug]\ngdscript/warnings/enable=false\n",
    )
    .unwrap();
    fs::write(
        directory.join("gdkit.toml"),
        "[engine]\nexecutable='unused'\n[check]\nstrict_methods=true\n[[check.ignore_import_errors]]\nmessage='ERROR: expected'\nsource='res://fixture.gd'\n",
    )
    .unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_gdkit"))
            .env_remove("GDKIT_GODOT")
            .args(["doctor", directory.to_str().unwrap(), "--godot"])
            .arg(std::env::var_os("GDKIT_TEST_GODOT").unwrap())
            .output()
            .unwrap()
    };
    let first = run();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first = String::from_utf8(first.stdout).unwrap();
    assert!(first.contains("selected by: --godot"), "{first}");
    assert!(
        first.contains("probe cache: healthy (created; was missing)"),
        "{first}"
    );
    assert!(first.contains("legacy worker: stopped"), "{first}");
    assert!(first.contains("GDScript warnings: false"), "{first}");
    assert!(first.contains("strict methods: enabled"), "{first}");
    assert!(first.contains("suppresses 1 exact import error"), "{first}");

    let second = run();
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(
        String::from_utf8(second.stdout)
            .unwrap()
            .contains("probe cache: healthy (hit)")
    );

    let environment = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .env("GDKIT_GODOT", std::env::var_os("GDKIT_TEST_GODOT").unwrap())
        .args(["doctor", directory.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(environment.status.success());
    assert!(
        String::from_utf8(environment.stdout)
            .unwrap()
            .contains("selected by: GDKIT_GODOT")
    );

    let engine = std::env::var("GDKIT_TEST_GODOT")
        .unwrap()
        .replace('\\', "/");
    fs::write(
        directory.join("gdkit.toml"),
        format!("[engine]\nexecutable='{engine}'\n"),
    )
    .unwrap();
    let configured = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .env_remove("GDKIT_GODOT")
        .args(["doctor", directory.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(configured.status.success());
    let configured = String::from_utf8(configured.stdout).unwrap();
    assert!(configured.contains("selected by:"), "{configured}");
    assert!(configured.contains("gdkit.toml"), "{configured}");
    assert!(
        configured.contains("checkpoint adapter: not configured"),
        "{configured}"
    );

    fs::write(
        directory.join("gdkit.toml"),
        format!(
            "[engine]\nexecutable='{engine}'\n[inspect]\ncheckpoint_adapter='res://tools/checkpoints.gd'\n"
        ),
    )
    .unwrap();
    let checkpoints = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .env_remove("GDKIT_GODOT")
        .args(["doctor", directory.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(checkpoints.status.success());
    assert!(
        String::from_utf8(checkpoints.stdout)
            .unwrap()
            .contains("checkpoint adapter: res://tools/checkpoints.gd")
    );

    fs::write(directory.join("gdkit.toml"), "invalid toml [").unwrap();
    let overridden = run();
    assert!(overridden.status.success());
    assert!(
        String::from_utf8(overridden.stdout)
            .unwrap()
            .contains("gdkit.toml is invalid; the engine still came from --godot")
    );
    fs::remove_dir_all(Path::new(&directory)).unwrap();
}
