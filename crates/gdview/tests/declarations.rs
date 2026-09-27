// Acceptance tests for gdview::declarations.

use std::fs;

use gdview::declarations::{
    MemberKind, ResourceUseKind, RpcConfig, RpcMode, TransferMode, index_project, index_script,
};
use gdview::{Project, ResPath};

fn res(text: &str) -> ResPath {
    ResPath::parse(text).unwrap()
}

const PLAYER: &str = r#"@tool
class_name Player
extends CharacterBody2D

signal died
signal hit(amount: int, source = null)

const SPEED := 5.0
@export var health: int = 3
static var instances := 0
var _secret = 1
enum State { IDLE, RUN }
@onready var camera: Camera3D = $Head/Camera3D
@onready var label = get_node("UI/%Score")

@rpc("any_peer", "call_local", "reliable", 2)
func take_damage(amount: int, from_peer := 0) -> void:
	var fx = preload("res://fx/hit.tscn")
	var data = load("sub/data.tres")
	$Body.visible = false

static func create() -> Player:
	return null

func _private(...rest) -> void:
	pass

class Weapon extends Node:
	var ammo = 3
	class Barrel:
		func heat(): pass
"#;

#[test]
fn indexes_class_name_extends_and_members_with_line_numbers() {
    let indexed = index_script(res("res://player.gd"), PLAYER);
    assert_eq!(indexed.parse_error, None);
    let decl = &indexed.declaration;
    assert_eq!(decl.path, res("res://player.gd"));
    assert_eq!(
        (
            decl.class_name.as_ref().unwrap().name.as_str(),
            decl.class_name.as_ref().unwrap().line
        ),
        ("Player", 2)
    );
    let extends = decl.extends.as_ref().unwrap();
    assert_eq!(
        (extends.name.as_str(), extends.line),
        ("CharacterBody2D", 3)
    );
    assert!(extends.quoted_path().is_none());
    assert!(decl.is_tool);
    let members: Vec<_> = decl
        .members
        .iter()
        .map(|m| (m.kind, m.name.as_str(), m.line))
        .collect();
    assert_eq!(
        members,
        [
            (MemberKind::Signal, "died", 5),
            (MemberKind::Signal, "hit", 6),
            (MemberKind::Const, "SPEED", 8),
            (MemberKind::Var, "health", 9),
            (MemberKind::Var, "instances", 10),
            (MemberKind::Var, "_secret", 11),
            (MemberKind::Enum, "State", 12),
            (MemberKind::Var, "camera", 13),
            (MemberKind::Var, "label", 14),
            (MemberKind::Func, "take_damage", 17),
            (MemberKind::Func, "create", 22),
            (MemberKind::Func, "_private", 25),
        ]
    );
    let hit = &decl.members[1];
    assert_eq!(hit.arity(), (1, Some(2)));
    let take_damage = &decl.members[9];
    assert_eq!(take_damage.parameters[0].type_text.as_deref(), Some("int"));
    assert_eq!(take_damage.parameters[1].default.as_deref(), Some("0"));
    assert_eq!(take_damage.type_text.as_deref(), Some("void"));
    assert_eq!(decl.members[11].arity(), (0, None));

    let uses: Vec<_> = indexed
        .resource_uses
        .iter()
        .map(|u| (u.kind, u.path.as_str(), u.line))
        .collect();
    assert_eq!(
        uses,
        [
            (ResourceUseKind::Preload, "res://fx/hit.tscn", 18),
            (ResourceUseKind::Load, "sub/data.tres", 19)
        ]
    );
    let paths: Vec<_> = indexed
        .node_path_uses
        .iter()
        .map(|u| (u.path.as_str(), u.line, u.onready))
        .collect();
    assert_eq!(
        paths,
        [
            ("Head/Camera3D", 13, true),
            ("UI/%Score", 14, true),
            ("Body", 20, false)
        ]
    );
}

#[test]
fn res_and_uid_string_literals_are_resource_uses_once() {
    let source = r#"extends "res://base.gd"

const LEVEL := "res://levels/one.tscn"
const SAVE_DIR = "user://saves"
const NAME = "resolution"

func _ready() -> void:
	var scene = preload("res://ui/hud.tscn")
	get_tree().change_scene_to_file("uid://c4x8")
	var icon = load('res://icon.svg')
"#;
    let indexed = index_script(res("res://main.gd"), source);
    let uses: Vec<_> = indexed
        .resource_uses
        .iter()
        .map(|u| (u.kind, u.path.as_str(), u.line))
        .collect();
    assert_eq!(
        uses,
        [
            (ResourceUseKind::Extends, "res://base.gd", 1),
            (ResourceUseKind::String, "res://levels/one.tscn", 3),
            (ResourceUseKind::Preload, "res://ui/hud.tscn", 8),
            (ResourceUseKind::String, "uid://c4x8", 9),
            (ResourceUseKind::Load, "res://icon.svg", 10),
        ]
    );
}

#[test]
fn records_annotations_per_member_including_rpc_arguments() {
    let decl = index_script(res("res://player.gd"), PLAYER).declaration;
    let health = decl.members.iter().find(|m| m.name == "health").unwrap();
    assert_eq!(
        health
            .annotations
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>(),
        ["export"]
    );
    let take_damage = decl
        .members
        .iter()
        .find(|m| m.name == "take_damage")
        .unwrap();
    assert_eq!(
        take_damage.annotations[0].arguments,
        ["\"any_peer\"", "\"call_local\"", "\"reliable\"", "2"]
    );
    assert_eq!(
        take_damage.rpc,
        Some(RpcConfig {
            mode: RpcMode::AnyPeer,
            call_local: true,
            transfer: TransferMode::Reliable,
            channel: 2
        })
    );
    assert!(
        decl.members
            .iter()
            .filter(|m| m.name != "take_damage")
            .all(|m| m.rpc.is_none())
    );
    let bare = index_script(res("res://a.gd"), "@rpc\nfunc ping(): pass\n").declaration;
    assert_eq!(
        bare.members[0].rpc,
        Some(RpcConfig {
            mode: RpcMode::Authority,
            call_local: false,
            transfer: TransferMode::Unreliable,
            channel: 0
        })
    );
}

#[test]
fn distinguishes_static_private_and_export_members() {
    let decl = index_script(res("res://player.gd"), PLAYER).declaration;
    let find = |name: &str| decl.members.iter().find(|m| m.name == name).unwrap();
    assert!(find("instances").is_static && !find("instances").is_private);
    assert!(find("create").is_static);
    assert!(!find("take_damage").is_static);
    assert!(find("_secret").is_private && find("_private").is_private);
    assert!(
        find("health")
            .annotations
            .iter()
            .any(|a| a.name == "export")
    );
    assert!(find("_secret").annotations.is_empty());
}

#[test]
fn indexes_inner_classes_recursively_with_qualified_names() {
    let decl = index_script(res("res://player.gd"), PLAYER).declaration;
    assert_eq!(decl.inner_classes.len(), 1);
    let weapon = &decl.inner_classes[0];
    assert_eq!(
        (weapon.qualified_name.as_str(), weapon.line),
        ("Player.Weapon", 28)
    );
    assert_eq!(weapon.extends.as_ref().unwrap().name, "Node");
    assert_eq!(weapon.members[0].name, "ammo");
    let barrel = &weapon.inner_classes[0];
    assert_eq!(barrel.qualified_name, "Player.Weapon.Barrel");
    assert_eq!(barrel.members[0].name, "heat");
    let anonymous = index_script(res("res://x.gd"), "class A:\n\tpass\n").declaration;
    assert_eq!(anonymous.inner_classes[0].qualified_name, "A");
}

#[test]
fn unparseable_scripts_are_reported_not_skipped() {
    let indexed = index_script(
        res("res://broken.gd"),
        "extends \"res://base.gd\"\nfunc ok(): pass\nfunc broken(:\n\tpass\nvar after = 1\n",
    );
    assert!(
        indexed
            .parse_error
            .as_deref()
            .unwrap()
            .starts_with("line 3:"),
        "{:?}",
        indexed.parse_error
    );
    let names: Vec<_> = indexed
        .declaration
        .members
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    assert!(
        names.contains(&"ok") && names.contains(&"after"),
        "{names:?}"
    );
    assert_eq!(
        indexed.declaration.extends.as_ref().unwrap().quoted_path(),
        Some("res://base.gd")
    );
    assert_eq!(indexed.resource_uses[0].kind, ResourceUseKind::Extends);
}

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

#[test]
fn index_project_uses_file_query_and_is_sorted_by_path() {
    let (_dir, project) = project(&[
        ("z.gd", "extends Node\n"),
        ("a/b.gd", "extends Node\n"),
        ("a.gd", "extends Node\n"),
        ("vendor/.gdignore", ""),
        ("vendor/skip.gd", ""),
        (".hidden/skip.gd", ""),
        ("notes.txt", ""),
    ]);
    let declarations = index_project(&project).unwrap();
    let paths: Vec<_> = declarations
        .scripts
        .iter()
        .map(|s| s.declaration.path.as_str())
        .collect();
    assert_eq!(paths, ["res://a.gd", "res://a/b.gd", "res://z.gd"]);
    assert!(declarations.by_path(&res("res://a/b.gd")).is_some());
    assert!(declarations.by_path(&res("res://vendor/skip.gd")).is_none());
}

#[test]
fn by_class_name_detects_duplicate_declarations() {
    let (_dir, project) = project(&[
        ("a.gd", "class_name Weapon\nextends Node\n"),
        ("b.gd", "class_name Weapon\nextends Node\n"),
        ("c.gd", "class_name Armor\n"),
    ]);
    let declarations = index_project(&project).unwrap();
    let classes = declarations.by_class_name();
    assert_eq!(
        classes["Weapon"]
            .iter()
            .map(|d| d.path.as_str())
            .collect::<Vec<_>>(),
        ["res://a.gd", "res://b.gd"]
    );
    assert_eq!(classes["Armor"].len(), 1);
}

#[test]
fn rpc_config_rejects_unknown_arguments() {
    assert!(RpcConfig::from_arguments(&["\"any_peer\"", "\"sometimes\""]).is_err());
    assert!(RpcConfig::from_arguments(&["\"any_peer\"", "\"unreliable_ordered\""]).is_ok());
}

// Differential cases checked against Godot 4.7.2; see docs/NET_RESEARCH.md.
#[test]
fn rpc_config_enforces_categories_and_channel_position() {
    for args in [
        vec!["\"authority\"", "\"any_peer\""],
        vec!["\"any_peer\"", "\"any_peer\""],
        vec!["\"call_local\"", "\"call_remote\""],
        vec!["\"call_local\"", "\"call_local\""],
        vec!["\"reliable\"", "\"unreliable\""],
        vec!["\"unreliable_ordered\"", "\"unreliable_ordered\""],
        vec!["2"],
        vec!["2", "\"reliable\""],
        vec!["\"reliable\"", "2"],
        vec!["\"any_peer\"", "\"unreliable_ordered\"", "1"],
        vec!["\"any_peer\"", "\"call_local\"", "\"reliable\"", "\"3\""],
        vec!["\"any_peer\"", "\"call_local\"", "\"reliable\"", "3", "4"],
    ] {
        assert!(RpcConfig::from_arguments(&args).is_err(), "{args:?}");
    }
    for args in [
        ["\"any_peer\"", "\"call_local\"", "\"reliable\""],
        ["\"any_peer\"", "\"reliable\"", "\"call_local\""],
        ["\"call_local\"", "\"any_peer\"", "\"reliable\""],
        ["\"call_local\"", "\"reliable\"", "\"any_peer\""],
        ["\"reliable\"", "\"any_peer\"", "\"call_local\""],
        ["\"reliable\"", "\"call_local\"", "\"any_peer\""],
    ] {
        assert_eq!(
            RpcConfig::from_arguments(&args).unwrap(),
            RpcConfig {
                mode: RpcMode::AnyPeer,
                call_local: true,
                transfer: TransferMode::Reliable,
                channel: 0,
            }
        );
    }
    let defaults = RpcConfig {
        mode: RpcMode::Authority,
        call_local: false,
        transfer: TransferMode::Unreliable,
        channel: 0,
    };
    assert_eq!(RpcConfig::from_arguments(&[]).unwrap(), defaults);
    assert_eq!(
        RpcConfig::from_arguments(&["\"unreliable_ordered\""]).unwrap(),
        RpcConfig {
            transfer: TransferMode::UnreliableOrdered,
            ..defaults
        }
    );
}

#[test]
fn rpc_config_preserves_signed_channels_and_decodes_string_literals() {
    for literal in [
        "\"any_peer\"",
        "'any_peer'",
        "r\"any_peer\"",
        "&\"any_peer\"",
        "\"any_\\u0070eer\"",
        "\"\"\"any_peer\"\"\"",
    ] {
        assert_eq!(
            RpcConfig::from_arguments(&[literal]).unwrap().mode,
            RpcMode::AnyPeer
        );
    }
    for (text, channel) in [
        ("0", 0),
        ("+2", 2),
        (" -1 ", -1),
        ("9223372036854775807", i64::MAX),
        ("-9223372036854775808", i64::MIN),
    ] {
        assert_eq!(
            RpcConfig::from_arguments(&[
                "\"authority\"",
                "\"call_remote\"",
                "\"unreliable\"",
                text
            ])
            .unwrap()
            .channel,
            channel
        );
    }
    for malformed in [
        "\"any_peer'",
        "\"any_peer\" junk",
        "\"any_peer\"; var other = 1",
    ] {
        assert!(
            RpcConfig::from_arguments(&[malformed]).is_err(),
            "{malformed}"
        );
    }
}

#[test]
fn rpc_config_keeps_unresolved_expressions_explicit() {
    // Several of these ARE accepted by Godot when constant. The source index
    // must preserve its inability to evaluate them, not invent a default config.
    for expression in ["MODE", "any_peer", "\"any_\" + \"peer\""] {
        let error = RpcConfig::from_arguments(&[expression]).unwrap_err();
        assert!(error.contains("cannot resolve"), "{error}");
    }
    for expression in [
        "CHANNEL",
        "1 + 2",
        "2.5",
        "0x10",
        "1_000",
        "9223372036854775808",
    ] {
        let error = RpcConfig::from_arguments(&[
            "\"any_peer\"",
            "\"call_remote\"",
            "\"reliable\"",
            expression,
        ])
        .unwrap_err();
        assert!(error.contains("cannot resolve @rpc channel"), "{error}");
    }
}

#[test]
fn rpc_annotation_errors_survive_indexing_including_inner_classes() {
    let source = r#"extends Node
@rpc("authority", "any_peer")
func invalid(): pass
@rpc(MODE)
func unresolved(): pass
@rpc("any_peer", "call_remote", "reliable", -1)
func valid(): pass
func plain(): pass
class Inner extends Node:
    @rpc("reliable", 2)
    func invalid_inner(): pass
    @rpc
    func valid_inner(): pass
@rpc
@rpc
func repeated(): pass
@rpc
var not_a_function = 0
"#;
    let indexed = index_script(res("res://rpc.gd"), source);
    assert_eq!(indexed.parse_error, None, "syntactically valid source");
    let decl = indexed.declaration;
    let invalid = &decl.members[0];
    assert!(invalid.rpc.is_none());
    assert!(invalid.rpc_error.as_deref().unwrap().contains("permission"));
    assert_eq!(
        invalid.annotations[0].arguments,
        ["\"authority\"", "\"any_peer\""]
    );
    assert_eq!(invalid.line, 3);
    let unresolved = &decl.members[1];
    assert!(unresolved.rpc.is_none());
    assert!(
        unresolved
            .rpc_error
            .as_deref()
            .unwrap()
            .contains("cannot resolve")
    );
    assert_eq!(decl.members[2].rpc.as_ref().unwrap().channel, -1);
    assert!(decl.members[2].rpc_error.is_none());
    assert!(decl.members[3].rpc.is_none());
    assert!(decl.members[3].rpc_error.is_none());
    let inner = &decl.inner_classes[0];
    assert_eq!(inner.qualified_name, "Inner");
    assert!(inner.members[0].rpc_error.is_some());
    assert!(inner.members[0].rpc.is_none());
    assert_eq!(inner.members[1].rpc.as_ref().unwrap().channel, 0);
    assert!(inner.members[1].rpc_error.is_none());
    assert!(
        decl.members[4]
            .rpc_error
            .as_deref()
            .unwrap()
            .contains("once per member")
    );
    assert!(decl.members[4].rpc.is_none());
    assert!(
        decl.members[5]
            .rpc_error
            .as_deref()
            .unwrap()
            .contains("requires a function")
    );
    assert!(decl.members[5].rpc.is_none());

    let json = serde_json::to_value(&decl).unwrap();
    assert!(json["members"][0]["rpc_error"].is_string());
    assert_eq!(json["members"][0]["rpc"], serde_json::Value::Null);
    assert_eq!(json["members"][2]["rpc"]["channel"], -1);
    assert!(json["members"][2].get("rpc_error").is_none());
}
