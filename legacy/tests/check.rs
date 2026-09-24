use std::{fs, process::Command};

#[test]
#[ignore = "requires GDKIT_TEST_GODOT pointing to a Godot 4 editor"]
fn project_script_runtime_initializes_autoload_identifiers() {
    let engine = std::env::var_os("GDKIT_TEST_GODOT").expect("set GDKIT_TEST_GODOT");
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(".tools")
        .join(format!("gdkit-check-autoload-{}", std::process::id()));
    fs::create_dir_all(directory.parent().unwrap()).unwrap();
    fs::create_dir(&directory).unwrap();
    fs::write(
        directory.join("project.godot"),
        "config_version=5\n[autoload]\nGameSettings=\"*res://game_settings.gd\"\n",
    )
    .unwrap();
    fs::write(
        directory.join("game_settings.gd"),
        "extends Node\n\nvar enabled := true\n",
    )
    .unwrap();
    fs::write(
        directory.join("contract.gd"),
        "extends SceneTree\n\nfunc _initialize() -> void:\n\tassert(GameSettings.enabled)\n\tprint(\"autoload contract passed\")\n\tquit(0)\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .env("NO_COLOR", "1")
        .env_remove("GDKIT_GODOT")
        .args([
            "check",
            directory.to_str().unwrap(),
            "--godot",
            engine.to_str().unwrap(),
            "--script",
            "contract.gd",
        ])
        .output()
        .unwrap();

    let _ = fs::remove_dir_all(&directory);
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT pointing to a Godot 4 editor"]
fn checks_project_scripts_scenes_and_resources() {
    let engine = std::env::var_os("GDKIT_TEST_GODOT").expect("set GDKIT_TEST_GODOT");
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(".tools")
        .join(format!("gdkit-check-{}", std::process::id()));
    fs::create_dir_all(directory.parent().unwrap()).unwrap();
    fs::create_dir(&directory).unwrap();
    fs::create_dir(directory.join(".git")).unwrap();
    fs::create_dir(directory.join("local only")).unwrap();
    fs::write(directory.join(".gitignore"), "/local only/\nignored.tres\n").unwrap();
    fs::write(directory.join("local only/unused.gd"), "extends Node\n").unwrap();
    fs::write(
        directory.join("ignored.tres"),
        "[gd_resource type=\"Resource\" format=3]\n[resource]\n",
    )
    .unwrap();
    fs::write(
        directory.join("project.godot"),
        "config_version=5\n[application]\nconfig/name=\"gdkit check test\"\n",
    )
    .unwrap();
    fs::create_dir_all(directory.join(".godot/editor")).unwrap();
    fs::write(
        directory.join(".godot/editor/filesystem_cache10"),
        "source cache sentinel",
    )
    .unwrap();

    let initialized = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .current_dir(&directory)
        .env_remove("GDKIT_GODOT")
        .arg("init")
        .arg("--godot")
        .arg(&engine)
        .output()
        .unwrap();
    assert!(
        initialized.status.success(),
        "{}",
        String::from_utf8_lossy(&initialized.stderr)
    );
    let config = fs::read_to_string(directory.join("gdkit.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&config).unwrap();
    let configured = parsed["engine"]["executable"].as_str().unwrap();
    assert_eq!(
        fs::canonicalize(directory.join(configured)).unwrap(),
        fs::canonicalize(&engine).unwrap()
    );
    if pathdiff::diff_paths(
        fs::canonicalize(&engine).unwrap(),
        fs::canonicalize(&directory).unwrap(),
    )
    .is_some()
    {
        assert!(std::path::Path::new(configured).is_relative());
    }
    let repeated = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .current_dir(&directory)
        .args(["init", "--godot", "missing-engine"])
        .output()
        .unwrap();
    assert_eq!(repeated.status.code(), Some(2));
    assert_eq!(
        fs::read_to_string(directory.join("gdkit.toml")).unwrap(),
        config
    );
    fs::write(
        directory.join("player.gd"),
        "class_name CheckPlayer extends Node\n\nvar health: int = 100\n",
    )
    .unwrap();
    fs::write(
		directory.join("player.tscn"),
		"[gd_scene load_steps=2 format=3]\n\n[ext_resource type=\"Script\" path=\"res://player.gd\" id=\"1\"]\n\n[node name=\"Player\" type=\"Node\"]\nscript = ExtResource(\"1\")\n",
	)
	.unwrap();
    fs::write(
        directory.join("stats.tres"),
        "[gd_resource type=\"Resource\" format=3]\n\n[resource]\nresource_name = \"Stats\"\n",
    )
    .unwrap();

    let clean = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .env("NO_COLOR", "1")
        .env_remove("GDKIT_GODOT")
        .args(["check", directory.to_str().unwrap(), "--timings"])
        .output()
        .unwrap();
    assert!(
        clean.status.success(),
        "{}",
        String::from_utf8_lossy(&clean.stderr)
    );
    let timing_output = String::from_utf8_lossy(&clean.stderr);
    assert!(
        timing_output.contains("timing: engine validation (cached)"),
        "{timing_output}"
    );
    for phase in ["file scan", "import", "resource loading", "total"] {
        assert!(
            timing_output.contains(&format!("timing: {phase} ")),
            "{timing_output}"
        );
    }
    assert_eq!(
        String::from_utf8(clean.stdout).unwrap(),
        concat!(
            "check passed: resource validation loaded 1 script, 1 scene, 1 resource\n",
            "runtime execution: none requested\n",
            "validation policy: project warning policy\n",
        )
    );
    let json = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .env_remove("GDKIT_GODOT")
        .args(["check", directory.to_str().unwrap(), "--output", "json"])
        .output()
        .unwrap();
    assert!(
        json.status.success(),
        "{}",
        String::from_utf8_lossy(&json.stderr)
    );
    let report: gdkit::report::CheckReport = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(report.outcome, gdkit::report::CheckOutcome::Passed);
    assert_eq!(report.schema_version, 2);
    assert_eq!(report.completed_phases, report.requested_phases);
    assert_eq!(report.checked.as_ref().unwrap().scripts, 1);
    assert!(report.policy.fresh_import);
    assert!(report.project.fingerprint.len() == 64);
    assert!(report.engine.as_ref().unwrap().fingerprint.len() == 64);
    assert!(report.diagnostics.is_empty());
    assert!(report.failures.is_empty());
    assert!(report.artifacts.iter().any(|artifact| {
        artifact.kind == gdkit::report::ArtifactKind::Report && artifact.path.is_file()
    }));
    assert!(report.artifacts.iter().any(|artifact| {
        artifact.phase.as_ref().is_some_and(|phase| {
            phase.kind == gdkit::report::CheckPhase::Import
                && artifact.kind == gdkit::report::ArtifactKind::Stdout
                && artifact
                    .path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy() == "cache-import.stdout.log")
        })
    }));
    assert!(!String::from_utf8_lossy(&json.stderr).contains("check passed"));
    assert_eq!(
        fs::read_to_string(directory.join(".godot/editor/filesystem_cache10")).unwrap(),
        "source cache sentinel"
    );
    fs::write(
        directory.join("contract.gd"),
        "extends SceneTree\n\nfunc _initialize() -> void:\n\tprint(\"contract passed\")\n\tquit(0)\n",
    )
    .unwrap();
    let scripted = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .env("NO_COLOR", "1")
        .env_remove("GDKIT_GODOT")
        .args([
            "check",
            directory.to_str().unwrap(),
            "--script",
            "res://contract.gd",
        ])
        .output()
        .unwrap();
    assert!(
        scripted.status.success(),
        "{}",
        String::from_utf8_lossy(&scripted.stderr)
    );
    assert_eq!(
        String::from_utf8(scripted.stdout).unwrap(),
        concat!(
            "check passed: resource validation loaded 2 scripts, 1 scene, 1 resource\n",
            "runtime execution: ran 1/1 project script\n",
            "validation policy: project warning policy\n",
        )
    );
    fs::write(
        directory.join("failing_contract.gd"),
        "extends SceneTree\n\nfunc _initialize() -> void:\n\tquit(1)\n",
    )
    .unwrap();
    fs::write(
        directory.join("skipped_contract.gd"),
        "extends SceneTree\n\nfunc _initialize() -> void:\n\tquit(0)\n",
    )
    .unwrap();
    let partial = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .env("NO_COLOR", "1")
        .env_remove("GDKIT_GODOT")
        .args([
            "check",
            directory.to_str().unwrap(),
            "--script",
            "res://failing_contract.gd",
            "--script",
            "res://skipped_contract.gd",
        ])
        .output()
        .unwrap();
    assert_eq!(partial.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(partial.stdout).unwrap(),
        concat!(
            "check failed: resource validation loaded 4 scripts, 1 scene, 1 resource\n",
            "runtime execution: ran 1/2 project scripts; 1 skipped (an earlier phase failed)\n",
            "validation policy: project warning policy\n",
        )
    );
    fs::remove_file(directory.join("failing_contract.gd")).unwrap();
    fs::remove_file(directory.join("skipped_contract.gd")).unwrap();
    let isolated = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .env_remove("GDKIT_GODOT")
        .args([
            "check",
            directory.to_str().unwrap(),
            "--isolated",
            "--script",
            "res://contract.gd",
            "--output",
            "json",
        ])
        .output()
        .unwrap();
    assert!(
        isolated.status.success(),
        "{}",
        String::from_utf8_lossy(&isolated.stderr)
    );
    let isolated_report: gdkit::report::CheckReport =
        serde_json::from_slice(&isolated.stdout).unwrap();
    assert_eq!(
        isolated_report.project.root,
        fs::canonicalize(&directory).unwrap()
    );
    assert!(isolated_report.policy.fresh_import);
    assert_eq!(isolated_report.checked.as_ref().unwrap().project_scripts, 1);
    assert!(isolated_report.completed_phases.iter().any(|phase| {
        phase.kind == gdkit::report::CheckPhase::ProjectScript
            && phase.id == "project_script:1:res://contract.gd"
    }));
    assert!(isolated_report.artifacts.iter().any(|artifact| {
        artifact.phase.as_ref().is_some_and(|phase| {
            phase.kind == gdkit::report::CheckPhase::ProjectScript
                && artifact.kind == gdkit::report::ArtifactKind::Stdout
                && fs::read_to_string(&artifact.path)
                    .unwrap()
                    .contains("contract passed")
        })
    }));
    fs::remove_file(directory.join("contract.gd")).unwrap();
    let colored = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .env_remove("GDKIT_GODOT")
        .env_remove("NO_COLOR")
        .env("CLICOLOR_FORCE", "1")
        .args(["check", directory.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(colored.status.success());
    assert!(!String::from_utf8_lossy(&colored.stderr).contains("timing:"));
    assert_eq!(
        String::from_utf8(colored.stdout).unwrap(),
        concat!(
            "\x1b[32mcheck passed: resource validation loaded 1 script, 1 scene, 1 resource\x1b[0m\n",
            "runtime execution: none requested\n",
            "validation policy: project warning policy\n",
        )
    );

    fs::write(
		directory.join("broken.tscn"),
		"[gd_scene load_steps=2 format=3]\n\n[ext_resource type=\"Script\" path=\"res://missing.gd\" id=\"1\"]\n\n[node name=\"Broken\" type=\"Node\"]\nscript = ExtResource(\"1\")\n",
	)
	.unwrap();
    let broken = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .env("NO_COLOR", "1")
        .env_remove("GDKIT_GODOT")
        .args(["check", directory.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(broken.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&broken.stdout).starts_with("check failed:"));
    assert!(String::from_utf8_lossy(&broken.stderr).contains("missing.gd"));
    let broken_json = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .env_remove("GDKIT_GODOT")
        .args(["check", directory.to_str().unwrap(), "--output", "json"])
        .output()
        .unwrap();
    assert_eq!(broken_json.status.code(), Some(1));
    let report: gdkit::report::CheckReport = serde_json::from_slice(&broken_json.stdout).unwrap();
    assert_eq!(
        report.outcome,
        gdkit::report::CheckOutcome::ValidationFailed
    );
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == gdkit::report::DiagnosticSeverity::Error)
    );
    assert!(
        report
            .failures
            .iter()
            .any(|failure| failure.kind == gdkit::report::FailureKind::Diagnostic)
    );

    fs::remove_file(directory.join("broken.tscn")).unwrap();
    fs::write(
        directory.join("broken.gd"),
        "extends Node\n\nvar health: int = \"full\"\n",
    )
    .unwrap();
    let invalid_script = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .env_remove("NO_COLOR")
        .env("CLICOLOR_FORCE", "1")
        .env_remove("GDKIT_GODOT")
        .args(["check", directory.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(invalid_script.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&invalid_script.stdout).starts_with("\x1b[31mcheck failed:"));
    assert!(String::from_utf8_lossy(&invalid_script.stdout).ends_with(
        "\x1b[0m\nruntime execution: none requested\nvalidation policy: project warning policy\n"
    ));
    assert!(String::from_utf8_lossy(&invalid_script.stderr).contains("broken.gd"));
    let invalid_json = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .env_remove("GDKIT_GODOT")
        .args(["check", directory.to_str().unwrap(), "--output", "json"])
        .output()
        .unwrap();
    assert_eq!(invalid_json.status.code(), Some(1));
    let invalid_report: gdkit::report::CheckReport =
        serde_json::from_slice(&invalid_json.stdout).unwrap();
    assert!(invalid_report.diagnostics.iter().any(|diagnostic| {
        diagnostic.phase.kind == gdkit::report::CheckPhase::Import
            && diagnostic.resource.as_deref() == Some("res://broken.gd")
    }));
    assert_eq!(
        fs::read_to_string(directory.join(".godot/editor/filesystem_cache10")).unwrap(),
        "source cache sentinel"
    );
    fs::write(
        directory.join("broken.gd"),
        "extends Node\n\nvar missing = preload(\"uid://daaaaaaaaaaaa\")\n",
    )
    .unwrap();
    let unresolved = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .env("NO_COLOR", "1")
        .env_remove("GDKIT_GODOT")
        .args(["check", directory.to_str().unwrap(), "--verbose"])
        .output()
        .unwrap();
    assert_eq!(unresolved.status.code(), Some(1));
    let diagnostics = String::from_utf8_lossy(&unresolved.stderr);
    assert!(
        diagnostics.contains("Godot cannot resolve this resource ID to a file."),
        "{diagnostics}"
    );
    assert!(diagnostics.contains("res://broken.gd:3:"), "{diagnostics}");
    assert!(diagnostics.contains("Resource loading full Godot output:"));
    let stopped = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .args(["check", directory.to_str().unwrap(), "--stop-worker"])
        .output()
        .unwrap();
    assert!(
        stopped.status.success(),
        "{}",
        String::from_utf8_lossy(&stopped.stderr)
    );
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn engine_configuration_errors_and_precedence() {
    let directory = std::env::temp_dir().join(format!("gdkit-config-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("project.godot"), "config_version=5\n").unwrap();
    let run = |args: &[&str], environment: Option<&str>| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_gdkit"));
        command
            .current_dir(&directory)
            .env_remove("GDKIT_GODOT")
            .args(args);
        if let Some(value) = environment {
            command.env("GDKIT_GODOT", value);
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        String::from_utf8(output.stderr).unwrap()
    };
    assert!(run(&["check"], None).contains("gdkit init --godot"));
    let json = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .current_dir(&directory)
        .env_remove("GDKIT_GODOT")
        .args(["check", "--output", "json"])
        .output()
        .unwrap();
    assert_eq!(json.status.code(), Some(2));
    let report: gdkit::report::CheckReport = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(report.outcome, gdkit::report::CheckOutcome::ToolFailed);
    assert!(
        report
            .failures
            .iter()
            .any(|failure| failure.kind == gdkit::report::FailureKind::Tool)
    );
    assert!(run(&["init", "--godot", "missing-init"], None).contains("missing-init"));
    assert!(!directory.join("gdkit.toml").exists());
    assert!(
        run(&["init", "--godot", env!("CARGO_BIN_EXE_gdkit")], None).contains("headless editor")
    );
    assert!(!directory.join("gdkit.toml").exists());
    fs::write(
        directory.join("gdkit.toml"),
        "[engine]\nexecutable = '../missing-config'\n",
    )
    .unwrap();
    assert!(run(&["check"], None).contains("missing-config"));
    assert!(run(&["check"], Some("missing-env")).contains("missing-env"));
    assert!(
        run(
            &["check", "--godot", "missing-explicit"],
            Some("missing-env")
        )
        .contains("missing-explicit")
    );
    fs::write(directory.join("gdkit.toml"), "invalid toml [").unwrap();
    assert!(run(&["check"], None).contains("gdkit.toml"));
    fs::remove_file(directory.join("project.godot")).unwrap();
    assert!(run(&["init", "--godot", "missing-init"], None).contains("project"));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT pointing to a Godot 4 editor"]
fn checks_only_explicit_slices_and_reports_missing_dependencies() {
    let engine = std::env::var_os("GDKIT_TEST_GODOT").expect("set GDKIT_TEST_GODOT");
    let directory = std::env::temp_dir().join(format!("gdkit-slice-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    fs::create_dir(directory.join("domain")).unwrap();
    fs::write(
        directory.join("project.godot"),
        "config_version=5\n[autoload]\nBroken=\"*res://broken.gd\"\n",
    )
    .unwrap();
    fs::write(directory.join("broken.gd"), "this is not valid GDScript\n").unwrap();
    fs::write(
        directory.join("domain/base.gd"),
        "class_name SliceBase extends RefCounted\n",
    )
    .unwrap();
    fs::write(directory.join("domain/child.gd"), "extends SliceBase\n").unwrap();
    fs::write(directory.join("single.gd"), "extends RefCounted\n").unwrap();
    let run = |selections: &[&str]| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_gdkit"));
        command
            .env_remove("GDKIT_GODOT")
            .arg("check")
            .arg(&directory)
            .arg("--godot")
            .arg(&engine)
            .args(["--output", "json"]);
        for selection in selections {
            command.args(["--slice", selection]);
        }
        command.output().unwrap()
    };
    for (selections, count) in [
        (vec!["single.gd"], 1),
        (vec!["domain"], 2),
        (vec!["domain/child.gd", "domain/base.gd"], 2),
    ] {
        let output = run(&selections);
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["checked"]["scripts"], count);
    }
    let missing = run(&["domain/child.gd"]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stdout).contains("SliceBase"));
    for selection in ["../escape.gd", ".godot", "missing.gd"] {
        assert_eq!(run(&[selection]).status.code(), Some(2));
    }
    fs::remove_file(directory.join("project.godot")).unwrap();
    assert!(run(&["single.gd"]).status.success());
    fs::remove_dir_all(directory).unwrap();
}
