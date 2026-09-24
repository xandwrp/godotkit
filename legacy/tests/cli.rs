use std::{
    fs,
    io::Write,
    process::{Command, Output, Stdio},
};

fn run(args: &[&str], source: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .arg("format")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn run_project(directory: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .arg("format-project")
        .args(args)
        .current_dir(directory)
        .output()
        .unwrap()
}

fn run_scene_tree(path: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .arg("scene-tree")
        .arg(path)
        .args(args)
        .output()
        .unwrap()
}

fn run_autoloads(path: &std::path::Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .arg("autoloads")
        .arg(path)
        .output()
        .unwrap()
}

#[test]
fn prints_autoloads_in_saved_initialization_order() {
    let directory = std::env::temp_dir().join(format!("gdkit-autoloads-{}", std::process::id()));
    let nested = directory.join("scripts");
    fs::create_dir_all(&nested).unwrap();
    let source = r#"config_version=5

[autoload]
Events="*res://autoload/events.gd"
Audio="res://autoload/audio.tscn"
SaveManager="*res://autoload/save_manager.gd"
"#;
    fs::write(directory.join("project.godot"), source).unwrap();

    let output = run_autoloads(&nested);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "Autoloads (initialization order, zero-based):\n  0. Events [singleton] -> res://autoload/events.gd\n  1. Audio [node] -> res://autoload/audio.tscn\n  2. SaveManager [singleton] -> res://autoload/save_manager.gd\n"
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        fs::read_to_string(directory.join("project.godot")).unwrap(),
        source
    );
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn project_formatting_respects_monorepo_and_nested_gitignore_rules() {
    let repository = std::env::temp_dir().join(format!("gdkit-ignore-{}", std::process::id()));
    let project = repository.join("game");
    fs::create_dir_all(repository.join(".git/info")).unwrap();
    fs::create_dir_all(project.join("nested")).unwrap();
    fs::create_dir_all(project.join("local only")).unwrap();
    fs::create_dir_all(project.join("engine-ignored")).unwrap();
    fs::create_dir_all(project.join(".hidden")).unwrap();
    fs::write(project.join("project.godot"), "config_version=5\n").unwrap();
    fs::write(
        repository.join(".gitignore"),
        "/game/local only/\n*.local.gd\n/game/root.gd\n",
    )
    .unwrap();
    fs::write(repository.join(".git/info/exclude"), "excluded.gd\n").unwrap();
    fs::write(project.join(".gitignore"), "!keep.local.gd\n").unwrap();
    fs::write(project.join("nested/.gitignore"), "*.gd\n!keep.gd\n").unwrap();
    fs::write(project.join("engine-ignored/.gdignore"), "").unwrap();
    let ignored = [
        "local only/bad.gd",
        "skip.local.gd",
        "root.gd",
        "excluded.gd",
        "nested/bad.gd",
        "engine-ignored/bad.gd",
        ".hidden/bad.gd",
    ];
    for path in ignored {
        fs::write(project.join(path), "func broken(\n").unwrap();
    }
    let included = ["main.gd", "keep.local.gd", "nested/keep.gd"];
    for path in included {
        fs::write(project.join(path), "var value=1\n").unwrap();
    }
    let before = run_project(&project, &["--check"]);
    assert_eq!(
        before.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&before.stderr)
    );
    let formatted = run_project(&project, &[]);
    assert!(
        formatted.status.success(),
        "{}",
        String::from_utf8_lossy(&formatted.stderr)
    );
    assert!(run_project(&project, &["--check"]).status.success());
    for path in ignored {
        assert_eq!(
            fs::read_to_string(project.join(path)).unwrap(),
            "func broken(\n"
        );
    }
    for path in included {
        assert_eq!(
            fs::read_to_string(project.join(path)).unwrap(),
            "var value = 1\n"
        );
    }
    let explicit = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .args(["format", "--check"])
        .arg(project.join("root.gd"))
        .output()
        .unwrap();
    assert_eq!(explicit.status.code(), Some(2));
    fs::remove_dir_all(repository).unwrap();
}

#[test]
fn stdout_and_check_have_distinct_exit_statuses() {
    let source = "func f():\n    if ready:\n        return\n";
    let output = run(&[], source);
    assert!(output.status.success());
    assert_eq!(output.stdout, b"func f():\n\tif ready: return\n");
    assert!(output.stderr.is_empty());
    let check = run(&["--check"], source);
    assert_eq!(check.status.code(), Some(1));
    assert!(check.stdout.is_empty());
    assert!(
        run(&["--check"], "func f():\n\tif ready: return\n")
            .status
            .success()
    );
    assert_eq!(run(&[], "var x = [").status.code(), Some(2));
    assert_eq!(run(&["--line-width", "0"], source).status.code(), Some(2));
}

#[test]
fn writes_files_in_place_and_preserves_failed_input() {
    let directory = std::env::temp_dir().join(format!("gdkit-test-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let path = directory.join("guard.gd");
    let path_arg = path.to_str().unwrap();
    let source = "func f():\n    if ready:\n        return\n";
    fs::write(&path, source).unwrap();
    let output = run(&[path_arg], "");
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    let formatted = "func f():\n\tif ready: return\n";
    assert_eq!(fs::read_to_string(&path).unwrap(), formatted);
    let unchanged = run(&[path_arg], "");
    assert!(unchanged.status.success());
    assert!(unchanged.stdout.is_empty());
    assert!(unchanged.stderr.is_empty());
    assert_eq!(fs::read_to_string(&path).unwrap(), formatted);
    fs::write(&path, source).unwrap();
    assert_eq!(run(&[path_arg, "--check"], "").status.code(), Some(1));
    assert_eq!(fs::read_to_string(&path).unwrap(), source);
    for invalid in [
        "var x = [",
        "func f():\n    if ready:\n        return\n    var value =\n",
    ] {
        fs::write(&path, invalid).unwrap();
        for args in [[path_arg].as_slice(), [path_arg, "--check"].as_slice()] {
            let output = run(args, "");
            assert_eq!(output.status.code(), Some(2));
            assert!(output.stdout.is_empty());
            assert!(String::from_utf8_lossy(&output.stderr).contains("at byte"));
            assert_eq!(fs::read_to_string(&path).unwrap(), invalid);
        }
    }
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
    fs::remove_dir_all(directory).unwrap();
}

#[cfg(unix)]
#[test]
fn writes_preserve_permissions_and_refuse_symlinks() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let directory = std::env::temp_dir().join(format!("gdkit-permissions-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let path = directory.join("guard.gd");
    let link = directory.join("link.gd");
    let source = "func f():\n    if ready:\n        return\n";
    fs::write(&path, source).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    symlink(&path, &link).unwrap();
    assert_eq!(run(&[link.to_str().unwrap()], "").status.code(), Some(2));
    assert_eq!(fs::read_to_string(&path).unwrap(), source);
    assert!(run(&[path.to_str().unwrap()], "").status.success());
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn formats_project_scripts_recursively_and_checks_without_writing() {
    let directory = std::env::temp_dir().join(format!("gdkit-project-{}", std::process::id()));
    let nested = directory.join("scripts/nested");
    fs::create_dir_all(&nested).unwrap();
    fs::write(directory.join("project.godot"), "config_version=5\n").unwrap();
    let first = directory.join("player.gd");
    let second = nested.join("enemy.gd");
    let ignored = nested.join("notes.txt");
    let source = "func f():\n    if ready:\n        return\n";
    let formatted = "func f():\n\tif ready: return\n";
    fs::write(&first, source).unwrap();
    fs::write(&second, source).unwrap();
    fs::write(&ignored, source).unwrap();

    let check = run_project(&directory, &["--check"]);
    assert_eq!(check.status.code(), Some(1));
    assert!(check.stdout.is_empty());
    assert!(check.stderr.is_empty());
    assert_eq!(fs::read_to_string(&first).unwrap(), source);
    assert_eq!(fs::read_to_string(&second).unwrap(), source);

    let output = run_project(&directory, &[]);
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    assert_eq!(fs::read_to_string(&first).unwrap(), formatted);
    assert_eq!(fs::read_to_string(&second).unwrap(), formatted);
    assert_eq!(fs::read_to_string(&ignored).unwrap(), source);
    assert!(run_project(&directory, &["--check"]).status.success());
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn rejects_non_projects_and_validates_every_script_before_writing() {
    let directory =
        std::env::temp_dir().join(format!("gdkit-project-error-{}", std::process::id()));
    let nested = directory.join("scripts");
    fs::create_dir_all(&nested).unwrap();
    let valid = directory.join("valid.gd");
    let invalid = nested.join("invalid.gd");
    let source = "func f():\n    if ready:\n        return\n";
    fs::write(&valid, source).unwrap();

    let missing_project = run_project(&directory, &[]);
    assert_eq!(missing_project.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&missing_project.stderr).contains("project.godot"));
    assert_eq!(fs::read_to_string(&valid).unwrap(), source);

    fs::write(directory.join("project.godot"), "config_version=5\n").unwrap();
    fs::write(&invalid, "var value = [").unwrap();
    let failed = run_project(&directory, &[]);
    assert_eq!(failed.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&failed.stderr).contains("invalid.gd"));
    assert_eq!(fs::read_to_string(&valid).unwrap(), source);
    assert_eq!(fs::read_to_string(&invalid).unwrap(), "var value = [");
    fs::remove_dir_all(directory).unwrap();
}

#[cfg(unix)]
#[test]
fn project_formatting_does_not_follow_symlinks() {
    use std::os::unix::fs::symlink;
    let directory =
        std::env::temp_dir().join(format!("gdkit-project-links-{}", std::process::id()));
    let external =
        std::env::temp_dir().join(format!("gdkit-project-external-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    fs::create_dir(&external).unwrap();
    fs::write(directory.join("project.godot"), "config_version=5\n").unwrap();
    let source = "func f():\n    if ready:\n        return\n";
    let external_script = external.join("external.gd");
    fs::write(&external_script, source).unwrap();
    symlink(&external_script, directory.join("linked.gd")).unwrap();
    symlink(&external, directory.join("linked_directory")).unwrap();

    assert!(run_project(&directory, &[]).status.success());
    assert_eq!(fs::read_to_string(&external_script).unwrap(), source);
    fs::remove_dir_all(directory).unwrap();
    fs::remove_dir_all(external).unwrap();
}

#[test]
fn prints_compact_scene_trees_without_writing() {
    let directory = std::env::temp_dir().join(format!("gdkit-scene-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let path = directory.join("menu.tscn");
    let source = "[gd_scene format=3]\n\n[node name=\"Menu\" type=\"Control\"]\n\n[node name=\"Label\" type=\"Label\" parent=\".\"]\n";
    fs::write(&path, source).unwrap();
    let output = run_scene_tree(&path, &[]);
    assert!(output.status.success());
    assert_eq!(output.stdout, b"Menu : Control\n\\- Label : Label\n");
    assert!(output.stderr.is_empty());
    assert_eq!(fs::read_to_string(&path).unwrap(), source);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn expands_scene_instances_to_the_requested_depth_and_resolves_inheritance() {
    let directory =
        std::env::temp_dir().join(format!("gdkit-scene-expansion-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("project.godot"), "config_version=5\n").unwrap();
    fs::write(
        directory.join("spark.tscn"),
        "[gd_scene format=3]\n\n[node name=\"Spark\" type=\"GPUParticles3D\"]\n\n[node name=\"Light\" type=\"OmniLight3D\" parent=\".\"]\n",
    )
    .unwrap();
    fs::write(
        directory.join("weapon.tscn"),
        "[gd_scene format=3]\n\n[ext_resource type=\"PackedScene\" path=\"res://spark.tscn\" id=\"1_spark\"]\n\n[node name=\"Weapon\" type=\"Node3D\"]\n\n[node name=\"Barrel\" type=\"MeshInstance3D\" parent=\".\"]\n\n[node name=\"Spark\" parent=\".\" instance=ExtResource(\"1_spark\")]\n",
    )
    .unwrap();
    let path = directory.join("player.tscn");
    fs::write(
        &path,
        "[gd_scene format=3]\n\n[ext_resource type=\"PackedScene\" path=\"res://weapon.tscn\" id=\"1_weapon\"]\n\n[node name=\"PlayerWeapon\" instance=ExtResource(\"1_weapon\")]\n\n[node name=\"Barrel\" parent=\".\" index=\"0\"]\n\n[node name=\"Socket\" type=\"Marker3D\" parent=\"Barrel\"]\n",
    )
    .unwrap();

    let depth_one = run_scene_tree(&path, &["--expand-depth", "1"]);
    assert!(depth_one.status.success());
    assert_eq!(
        String::from_utf8(depth_one.stdout).unwrap(),
        "PlayerWeapon : Node3D [instance: res://weapon.tscn]\n|- Barrel : MeshInstance3D [origin: res://weapon.tscn]\n|  \\- Socket : Marker3D\n\\- Spark [instance: res://spark.tscn] [origin: res://weapon.tscn]\n"
    );

    let depth_two = run_scene_tree(&path, &["--expand-depth", "2"]);
    assert!(depth_two.status.success());
    let fully_expanded = "PlayerWeapon : Node3D [instance: res://weapon.tscn]\n|- Barrel : MeshInstance3D [origin: res://weapon.tscn]\n|  \\- Socket : Marker3D\n\\- Spark : GPUParticles3D [instance: res://spark.tscn] [origin: res://weapon.tscn]\n   \\- Light : OmniLight3D [origin: res://spark.tscn]\n";
    assert_eq!(String::from_utf8(depth_two.stdout).unwrap(), fully_expanded);
    assert_eq!(
        String::from_utf8(run_scene_tree(&path, &["--expand"]).stdout).unwrap(),
        fully_expanded
    );
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn detects_scene_instance_cycles_and_validates_expansion_options() {
    let directory = std::env::temp_dir().join(format!("gdkit-scene-cycle-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("project.godot"), "config_version=5\n").unwrap();
    let first = directory.join("first.tscn");
    fs::write(
        &first,
        "[gd_scene format=3]\n[ext_resource type=\"PackedScene\" path=\"res://second.tscn\" id=\"1\"]\n[node name=\"First\" instance=ExtResource(\"1\")]\n",
    )
    .unwrap();
    fs::write(
        directory.join("second.tscn"),
        "[gd_scene format=3]\n[ext_resource type=\"PackedScene\" path=\"res://first.tscn\" id=\"1\"]\n[node name=\"Second\" instance=ExtResource(\"1\")]\n",
    )
    .unwrap();

    let cycle = run_scene_tree(&first, &["--expand"]);
    assert_eq!(cycle.status.code(), Some(2));
    assert!(
        String::from_utf8(cycle.stderr)
            .unwrap()
            .contains("scene instance cycle detected")
    );
    assert!(
        run_scene_tree(&first, &["--expand", "--expand-depth", "2"])
            .status
            .code()
            == Some(2)
    );
    assert_eq!(
        run_scene_tree(&first, &["--expand-depth", "65"])
            .status
            .code(),
        Some(2)
    );
    fs::remove_dir_all(directory).unwrap();
}
