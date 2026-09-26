// Acceptance tests for gdview::autoload.

use std::fs;

use gdview::Project;
use gdview::ResPath;
use gdview::autoload::{AutoloadKind, ResolvedAutoload};
use gdview::respath::Uid;
use gdview::uid::UidMap;

fn resolve(settings: &str, files: &[(&str, &str)]) -> Vec<ResolvedAutoload> {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("project.godot"), settings).unwrap();
    for (path, contents) in files {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    let project = Project::open(dir.path()).unwrap();
    let uids = UidMap::build(&project).unwrap();
    project
        .settings()
        .unwrap()
        .autoloads()
        .unwrap()
        .resolve(&project, &uids)
}

#[test]
fn resolve_lists_zero_based_order_kind_and_path() {
    let listed = resolve(
        "[autoload]\nZeta=\"*res://systems/zeta.gd\"\nAlpha=\"res://ui/alpha.tscn\"\nNet=\"*uid://d17o2ql1av3dm\"\nGone=\"*uid://nothing\"\n",
        &[
            ("systems/zeta.gd", "extends Node\n"),
            ("net/net.gd", "extends Node\n"),
            ("net/net.gd.uid", "uid://d17o2ql1av3dm\n"),
        ],
    );
    let path = |p: &str| Some(ResPath::parse(p).unwrap());
    assert_eq!(
        listed,
        [
            ResolvedAutoload {
                order: 0,
                name: "Zeta".into(),
                singleton: true,
                kind: AutoloadKind::Script,
                path: path("res://systems/zeta.gd"),
                uid: None,
                exists: true,
            },
            ResolvedAutoload {
                order: 1,
                name: "Alpha".into(),
                singleton: false,
                kind: AutoloadKind::Scene,
                path: path("res://ui/alpha.tscn"),
                uid: None,
                exists: false,
            },
            ResolvedAutoload {
                order: 2,
                name: "Net".into(),
                singleton: true,
                kind: AutoloadKind::Script,
                path: path("res://net/net.gd"),
                uid: Some(Uid("uid://d17o2ql1av3dm".into())),
                exists: true,
            },
            ResolvedAutoload {
                order: 3,
                name: "Gone".into(),
                singleton: true,
                kind: AutoloadKind::Unresolved,
                path: None,
                uid: Some(Uid("uid://nothing".into())),
                exists: false,
            },
        ]
    );
    assert!(resolve("config_version=5\n", &[]).is_empty());
}

#[test]
fn scripts_vs_scenes_are_distinguished_by_extension() {
    for (path, kind) in [
        ("res://a.gd", AutoloadKind::Script),
        ("res://a.cs", AutoloadKind::Script),
        ("res://a.GD", AutoloadKind::Script),
        ("res://a.tscn", AutoloadKind::Scene),
        ("res://a.scn", AutoloadKind::Scene),
        ("res://a.tres", AutoloadKind::Other),
        ("res://a", AutoloadKind::Other),
    ] {
        assert_eq!(
            AutoloadKind::of(&ResPath::parse(path).unwrap()),
            kind,
            "{path}"
        );
    }
}
