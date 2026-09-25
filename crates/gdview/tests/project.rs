// Acceptance tests for gdview::project.

use std::fs;

use gdview::{Project, ResPath};

fn project_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("project.godot"), "config_version=5\n").unwrap();
    dir
}

#[test]
fn open_requires_project_godot_as_regular_file() {
    let dir = project_dir();
    assert_eq!(Project::open(dir.path()).unwrap().root(), dir.path());
    let empty = tempfile::tempdir().unwrap();
    assert!(matches!(Project::open(empty.path()), Err(gdview::Error::NotAProject { .. })));
    fs::create_dir(empty.path().join("project.godot")).unwrap();
    assert!(matches!(Project::open(empty.path()), Err(gdview::Error::ConfigNotAFile { .. })));
}

#[test]
fn discover_walks_ancestors_and_stops_at_first_marker() {
    let dir = project_dir();
    let nested = dir.path().join("addons/tool");
    fs::create_dir_all(nested.join("deep/er")).unwrap();
    fs::write(nested.join("project.godot"), "").unwrap();
    assert_eq!(Project::discover(nested.join("deep/er")).unwrap().root(), nested);
    assert_eq!(Project::discover(dir.path().join("addons")).unwrap().root(), dir.path());
    let outside = tempfile::tempdir().unwrap();
    assert!(Project::discover(outside.path()).is_err());
    assert!(Project::discover(dir.path().join("missing")).is_err());
}

#[test]
fn discover_from_file_starts_at_its_parent() {
    let dir = project_dir();
    fs::create_dir(dir.path().join("scripts")).unwrap();
    let script = dir.path().join("scripts/player.gd");
    fs::write(&script, "extends Node\n").unwrap();
    assert_eq!(Project::discover(&script).unwrap().root(), dir.path());
    assert_eq!(Project::discover(dir.path().join("project.godot")).unwrap().root(), dir.path());
}

#[test]
fn localize_maps_os_paths_inside_root_and_rejects_outside() {
    let dir = project_dir();
    let project = Project::open(dir.path()).unwrap();
    let res = project.localize(&dir.path().join("scenes/main.tscn")).unwrap();
    assert_eq!(res.as_str(), "res://scenes/main.tscn");
    assert_eq!(project.localize(std::path::Path::new("a/b.gd")).unwrap().as_str(), "res://a/b.gd");
    assert!(project.localize(&dir.path().join("../elsewhere.gd")).is_err());
    assert!(project.localize(std::path::Path::new("/definitely/elsewhere.gd")).is_err());
}

#[test]
fn localize_rejects_paths_under_dot_godot() {
    let dir = project_dir();
    let project = Project::open(dir.path()).unwrap();
    assert!(project.localize(&dir.path().join(".godot/uid_cache.bin")).is_err());
}

#[test]
fn globalize_is_inverse_of_localize() {
    let dir = project_dir();
    let project = Project::open(dir.path()).unwrap();
    for text in ["res://scenes/main.tscn", "res://a b/c.gd", "res://x"] {
        let res = ResPath::parse(text).unwrap();
        let os = project.globalize(&res);
        assert!(os.starts_with(dir.path()));
        assert_eq!(project.localize(&os).unwrap(), res);
    }
    fs::write(dir.path().join("x"), "hi").unwrap();
    assert_eq!(project.read_to_string(&ResPath::parse("res://x").unwrap()).unwrap(), "hi");
}

#[test]
#[ignore = "scaffold"]
fn settings_reads_project_godot_lazily_each_call() {
    todo!()
}
