// Acceptance tests for gdview::uid.

use std::fs;

use gdview::respath::Uid;
use gdview::uid::UidMap;
use gdview::{Project, ResPath};

fn project(files: &[(&str, &str)]) -> (tempfile::TempDir, Project) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("project.godot"), "config_version=5\n").unwrap();
    for (path, contents) in files {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    let project = Project::open(dir.path()).unwrap();
    (dir, project)
}

fn res(text: &str) -> ResPath {
    ResPath::parse(text).unwrap()
}

fn uid(text: &str) -> Uid {
    Uid(text.into())
}

#[test]
fn builds_map_from_sidecars_import_files_and_scene_headers() {
    let (_dir, project) = project(&[
        ("scripts/player.gd", "extends Node\n"),
        ("scripts/player.gd.uid", "uid://bplayer\n"),
        (
            "art/icon.png.import",
            "[remap]\n\nimporter=\"texture\"\ntype=\"CompressedTexture2D\"\nuid=\"uid://cicon\"\npath=\"res://.godot/imported/icon.png-x.ctex\"\n",
        ),
        (
            "scenes/main.tscn",
            "[gd_scene load_steps=2 format=3 uid=\"uid://dmain\"]\n\n[node name=\"Main\" type=\"Node\"]\n",
        ),
        (
            "data/item.tres",
            "[gd_resource type=\"Resource\" format=3 uid=\"uid://eitem\"]\n\n[resource]\n",
        ),
        (
            "scenes/no_uid.tscn",
            "[gd_scene format=3]\n\n[node name=\"A\" type=\"Node\"]\n",
        ),
        ("ignored/.gdignore", ""),
        ("ignored/x.gd.uid", "uid://fignored\n"),
    ]);
    fs::write(project.root().join(".gitignore"), "data/\n").unwrap();
    let map = UidMap::build(&project).unwrap();
    let expected = [
        ("uid://bplayer", "res://scripts/player.gd"),
        ("uid://cicon", "res://art/icon.png"),
        ("uid://dmain", "res://scenes/main.tscn"),
        ("uid://eitem", "res://data/item.tres"),
    ];
    for (u, p) in expected {
        assert_eq!(map.resolve(&uid(u)), Some(&res(p)), "{u}");
    }
    assert_eq!(map.by_uid.len(), 4, "{:?}", map.by_uid);
    assert!(map.duplicates.is_empty());
}

#[test]
fn resolve_returns_none_for_unknown_uid_not_error() {
    let (_dir, project) = project(&[
        ("a.gd.uid", "uid://aaa\n"),
        ("broken.gd.uid", "not a uid\n"),
    ]);
    let map = UidMap::build(&project).unwrap();
    assert_eq!(map.resolve(&uid("uid://zzz")), None);
    assert_eq!(map.uid_of(&res("res://broken.gd")), None);
    assert_eq!(map.by_uid.len(), 1);
}

#[test]
fn duplicate_uids_are_reported_with_both_paths() {
    let (_dir, project) = project(&[("a.gd.uid", "uid://same\n"), ("b.gd.uid", "uid://same\n")]);
    let map = UidMap::build(&project).unwrap();
    assert_eq!(
        map.duplicates,
        [(
            uid("uid://same"),
            vec![res("res://a.gd"), res("res://b.gd")]
        )]
    );
    assert_eq!(map.resolve(&uid("uid://same")), Some(&res("res://a.gd")));
}

#[test]
fn uid_of_path_is_inverse_of_resolve() {
    let (_dir, project) = project(&[
        ("a.gd.uid", "uid://aaa\n"),
        (
            "s.tscn",
            "[gd_scene format=3 uid=\"uid://sss\"]\n[node name=\"S\" type=\"Node\"]\n",
        ),
    ]);
    let map = UidMap::build(&project).unwrap();
    for (u, path) in &map.by_uid {
        assert_eq!(map.uid_of(path), Some(u));
    }
    assert_eq!(map.uid_of(&res("res://s.tscn")), Some(&uid("uid://sss")));
}
