use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};

struct Project(PathBuf);

impl Project {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("gdkit-cache-{name}-{}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("project.godot"), "config_version=5\n").unwrap();
        Self(path)
    }

    fn run(&self, command: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_gdkit"))
            .args(command)
            .arg(&self.0)
            .env_remove("GDKIT_GODOT")
            .output()
            .unwrap()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT pointing to a Godot editor"]
fn refresh_persists_uid_mapping_after_move() {
    let project = Project::new("refresh");
    let engine = std::env::var("GDKIT_TEST_GODOT").unwrap();
    fs::write(
        project.0.join("actor.gd"),
        "class_name CacheActor\nextends RefCounted\n",
    )
    .unwrap();
    let first = project.run(&["cache", "refresh", "--godot", &engine]);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let uid = fs::read_to_string(project.0.join("actor.gd.uid")).unwrap();
    fs::rename(project.0.join("actor.gd"), project.0.join("moved.gd")).unwrap();
    fs::rename(
        project.0.join("actor.gd.uid"),
        project.0.join("moved.gd.uid"),
    )
    .unwrap();
    let second = project.run(&["import", "--godot", &engine]);
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(
        fs::read_to_string(project.0.join("moved.gd.uid")).unwrap(),
        uid
    );
    fs::write(project.0.join("verify.gd"), format!("extends SceneTree\nfunc _init():\n\tvar id = ResourceUID.text_to_id(\"{}\")\n\tquit(0 if ResourceUID.get_id_path(id) == \"res://moved.gd\" else 1)\n", uid.trim())).unwrap();
    let verified = Command::new(engine)
        .args(["--headless", "--path"])
        .arg(&project.0)
        .args(["--script", "res://verify.gd"])
        .output()
        .unwrap();
    assert!(
        verified.status.success(),
        "{}",
        String::from_utf8_lossy(&verified.stderr)
    );
    fs::write(project.0.join(".godot/uid_cache.bin"), b"broken").unwrap();
    let rebuilt = project.run(&[
        "cache",
        "rebuild",
        "--godot",
        &std::env::var("GDKIT_TEST_GODOT").unwrap(),
    ]);
    assert!(
        rebuilt.status.success(),
        "{}",
        String::from_utf8_lossy(&rebuilt.stderr)
    );
    assert_eq!(
        fs::read_to_string(project.0.join("moved.gd.uid")).unwrap(),
        uid
    );
    let verified = Command::new(std::env::var("GDKIT_TEST_GODOT").unwrap())
        .args(["--headless", "--path"])
        .arg(&project.0)
        .args(["--script", "res://verify.gd"])
        .output()
        .unwrap();
    assert!(
        verified.status.success(),
        "{}",
        String::from_utf8_lossy(&verified.stderr)
    );
}

#[test]
fn clean_preserves_source_metadata_editor_state_and_gdkit_records() {
    let project = Project::new("clean");
    let removed = [
        ".godot/uid_cache.bin",
        ".godot/global_script_class_cache.cfg",
        ".godot/editor/filesystem_cache10",
        ".godot/editor/filesystem_update4",
        ".godot/imported/texture.ctex",
        ".godot/shader_cache/shader.bin",
    ];
    let preserved = [
        "actor.gd.uid",
        "texture.png.import",
        ".godot/editor/editor_layout.cfg",
        ".godot/editor/filesystem_cache_notes",
        ".godot/gdkit/api-index.json",
        ".godot/gdkit/checks/report.json",
    ];
    for name in removed.iter().chain(preserved.iter()) {
        let path = project.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "retained content").unwrap();
    }
    let preview = project.run(&["cache", "clean", "--dry-run"]);
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    assert!(removed.iter().all(|name| project.0.join(name).exists()));
    let clean = project.run(&["cache", "clean"]);
    assert!(
        clean.status.success(),
        "{}",
        String::from_utf8_lossy(&clean.stderr)
    );
    assert!(removed.iter().all(|name| !project.0.join(name).exists()));
    for name in preserved {
        assert_eq!(
            fs::read_to_string(project.0.join(name)).unwrap(),
            "retained content"
        );
    }
    assert!(project.run(&["cache", "clean"]).status.success());
}

#[cfg(windows)]
#[test]
fn clean_rejects_junctions_without_touching_the_target() {
    let project = Project::new("junction");
    let outside = Project::new("outside");
    fs::write(outside.0.join("sentinel"), "keep").unwrap();
    let junction = project.0.join(".godot");
    let result = Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&junction)
        .arg(&outside.0)
        .output()
        .unwrap();
    assert!(result.status.success());
    let result = project.run(&["cache", "clean"]);
    fs::remove_dir(&junction).unwrap();
    assert_eq!(result.status.code(), Some(2));
    assert_eq!(
        fs::read_to_string(outside.0.join("sentinel")).unwrap(),
        "keep"
    );
}

#[test]
fn status_and_stop_need_no_engine_and_status_creates_no_project_cache() {
    let project = Project::new("status");
    let status = project.run(&["cache", "status", "--output", "json"]);
    assert!(status.status.success());
    let report: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["freshness"], "unknown");
    assert!(
        report["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["present"] == false)
    );
    assert!(!project.0.join(".godot").exists());
    assert!(project.run(&["cache", "stop"]).status.success());
    assert!(!project.0.join(".godot").exists());
}
