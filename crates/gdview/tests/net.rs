use gdview::autoload::{AutoloadKind, ResolvedAutoload};
use gdview::declarations::{RpcMode, TransferMode};
use gdview::net::*;
use gdview::respath::NodePath;
use gdview::uid::UidMap;
use gdview::{ResPath, scene};

fn res(path: &str) -> ResPath {
    ResPath::parse(path).unwrap()
}
fn scan(source: &str) -> ScriptObservations {
    scan_script(res("res://main.gd"), source)
}
fn report(
    scripts: &[(&str, &str)],
    scenes: &[(&str, &str)],
    autoloads: &[ResolvedAutoload],
) -> NetReport {
    let scripts: Vec<_> = scripts
        .iter()
        .map(|(p, s)| scan_script(res(p), s))
        .collect();
    let scenes: Vec<_> = scenes
        .iter()
        .map(|(p, s)| (res(p), scene::parse(s).unwrap()))
        .collect();
    analyze(&NetInput {
        scripts: &scripts,
        scenes: &scenes,
        autoloads,
        uids: &UidMap::default(),
        unknowns: &[],
    })
}
fn autoload(name: &str, path: &str, kind: AutoloadKind) -> ResolvedAutoload {
    ResolvedAutoload {
        order: 0,
        name: name.into(),
        path: Some(res(path)),
        uid: None,
        singleton: true,
        kind,
        exists: true,
    }
}

#[test]
fn finds_rpc_endpoints_from_annotations_with_godot_defaults() {
    let script = scan(
        "extends Node\n@rpc\nfunc ping(): pass\n@rpc(\"any_peer\", \"call_local\", \"reliable\", -1)\nfunc fire(): pass\nfunc helper(): pass\n",
    );
    assert_eq!(script.endpoints.len(), 2);
    let ping = &script.endpoints[0];
    assert_eq!((ping.method.as_str(), ping.location.line), ("ping", 3));
    assert_eq!(ping.config.mode, RpcMode::Authority);
    assert!(!ping.config.call_local);
    assert_eq!(ping.config.transfer, TransferMode::Unreliable);
    assert_eq!(ping.config.channel, 0);
    assert_eq!(script.endpoints[1].config.channel, -1);
    assert!(script.unknowns.is_empty());
}

#[test]
fn finds_rpc_calls_in_every_form() {
    let script = scan(
        r#"extends Node
@rpc
func fire(): pass
func send(node: Node):
    rpc("fire")
    rpc_id(1, &"fire")
    self.rpc("fire")
    node.rpc_id(peer(), "fire")
    fire.rpc()
    fire.rpc_id(2)
    self.fire.rpc()
    node.fire.rpc_id(3)
    Callable(self, &"fire").rpc()
    multiplayer.rpc(1, self, &"fire", [])
    $Player.rpc("fire")
    %Player.fire.rpc_id(2)
    self . get_node("Player").rpc_id(4, "fire")
"#,
    );
    let calls = &script.calls;
    assert_eq!(calls.len(), 13, "{calls:#?}");
    assert!(calls.iter().all(|c| c.method.as_deref() == Some("fire")));
    assert_eq!(calls[0].form, CallForm::Rpc);
    assert_eq!(calls[1].form, CallForm::RpcId);
    assert_eq!(calls[3].receiver.as_deref(), Some("node"));
    assert_eq!(calls[3].target_peer.as_deref(), Some("peer()"));
    assert!(calls[3].receiver_is_local);
    assert_eq!(calls[4].form, CallForm::CallableRpc);
    assert_eq!(calls[5].target_peer.as_deref(), Some("2"));
    assert_eq!(calls[8].receiver.as_deref(), Some("self"));
    assert_eq!(calls[9].form, CallForm::MultiplayerRpc);
    assert_eq!(calls[9].receiver.as_deref(), Some("self"));
    assert_eq!(calls[11].receiver.as_deref(), Some("%Player"));
    assert_eq!(calls[12].location.line, 17);
    assert!(calls.iter().all(|c| c.candidates.is_empty()));
}

#[test]
fn does_not_classify_os_get_unique_id_as_authority_use() {
    let script = scan(
        r#"extends Node
func test():
    OS.get_unique_id()
    multiplayer.get_unique_id()
    self.multiplayer.get_remote_sender_id()
    multiplayer.is_server()
    is_multiplayer_authority()
    $Player.set_multiplayer_authority(1, false)
    unknown_object.get_unique_id()
    # multiplayer.is_server()
    var text = "self.get_multiplayer_authority()"
"#,
    );
    assert_eq!(script.authority_uses.len(), 5);
    assert!(
        !script
            .authority_uses
            .iter()
            .any(|a| a.call.contains("OS") || a.call.contains("unknown_object"))
    );
    assert!(
        script
            .unknowns
            .iter()
            .any(|u| u.message.contains("unproven authority"))
    );
}

const REPLICATION: &str = r#"[gd_scene format=3]
[sub_resource type="SceneReplicationConfig" id="Config"]
properties/10/path = NodePath(".:health")
properties/10/spawn = false
properties/10/replication_mode = 2
properties/2/path = NodePath(".:position")
properties/2/replication_mode = 1
[node name="Root" type="Node"]
[node name="Spawner" type="MultiplayerSpawner" parent="."]
spawn_path = NodePath("..")
_spawnable_scenes = PackedStringArray("res://player.tscn")
[node name="Sync" type="MultiplayerSynchronizer" parent="."]
replication_config = SubResource("Config")
"#;

#[test]
fn finds_spawners_and_synchronizers_with_replication_config_properties() {
    let r = report(&[], &[("res://main.tscn", REPLICATION)], &[]);
    assert_eq!(r.spawners.len(), 1);
    assert_eq!(r.spawners[0].spawn_path, Some(NodePath("..".into())));
    assert_eq!(r.spawners[0].auto_spawn_list, [res("res://player.tscn")]);
    let s = &r.synchronizers[0];
    assert_eq!(s.root_path, Some(NodePath("..".into())));
    assert_eq!(s.properties[0].path.0, ".:position");
    assert_eq!(s.properties[0].mode, Some(SyncMode::Always));
    assert_eq!(s.properties[0].spawn, Some(true));
    assert_eq!(s.properties[1].path.0, ".:health");
    assert_eq!(s.properties[1].mode, Some(SyncMode::OnChange));
    assert_eq!(s.properties[1].spawn, Some(false));
    assert!(r.unknowns.is_empty());
}

#[test]
fn old_format_replication_configs_respect_sync_false() {
    let scene = REPLICATION.replace(
        "properties/2/replication_mode = 1",
        "properties/2/sync = false",
    );
    let r = report(&[], &[("res://main.tscn", &scene)], &[]);
    assert_eq!(r.synchronizers[0].properties[0].mode, Some(SyncMode::Never));
    let scene = scene.replace("properties/2/sync = false", "properties/2/watch = true");
    let r = report(&[], &[("res://main.tscn", &scene)], &[]);
    assert_eq!(
        r.synchronizers[0].properties[0].mode,
        Some(SyncMode::OnChange)
    );
    // The shared parser loses assignment order. Never invent a precedence rule.
    let scene = scene.replace(
        "properties/2/watch = true",
        "properties/2/watch = true\nproperties/2/sync = true",
    );
    let r = report(&[], &[("res://main.tscn", &scene)], &[]);
    assert_eq!(r.synchronizers[0].properties[0].mode, None);
    assert!(
        r.unknowns
            .iter()
            .any(|u| u.message.contains("source order"))
    );
}

#[test]
fn external_replication_configs_and_malformed_fields_stay_explicit() {
    let main = r#"[gd_scene format=3]
[ext_resource type="SceneReplicationConfig" path="res://config.tres" id="1"]
[node name="Root" type="Node"]
[node name="Sync" type="MultiplayerSynchronizer" parent="."]
root_path = 42
replication_config = ExtResource("1")
[node name="Spawner" type="MultiplayerSpawner" parent="."]
"#;
    let config = r#"[gd_resource type="SceneReplicationConfig" format=3]
[resource]
properties/0/path = NodePath(".:position")
properties/0/spawn = "false"
properties/0/replication_mode = 99
"#;
    let r = report(
        &[],
        &[("res://main.tscn", main), ("res://config.tres", config)],
        &[],
    );
    assert_eq!(r.synchronizers[0].root_path, None);
    assert_eq!(r.synchronizers[0].properties[0].mode, None);
    assert_eq!(r.synchronizers[0].properties[0].spawn, None);
    assert_eq!(r.spawners[0].spawn_path, Some(NodePath(String::new())));
    assert!(r.unknowns.len() >= 3);
    let r = report(&[], &[("res://main.tscn", main)], &[]);
    assert!(
        r.unknowns
            .iter()
            .any(|u| u.message.contains("replication config is missing"))
    );
}

#[test]
fn scene_anchors_match_set_multiplayer_subtree_roots() {
    let source = r#"extends Node
func setup(api):
    self . get_tree().set_multiplayer(api, ^"/root/Match")
    get_tree().set_multiplayer(api, "/root/Mat")
    get_tree().set_multiplayer(api)
    get_tree().set_multiplayer(api, dynamic_root)
    other.set_multiplayer(api)
"#;
    let mut autos = vec![
        autoload("Match", "res://main.tscn", AutoloadKind::Scene),
        autoload("MatchExtra", "res://main.gd", AutoloadKind::Script),
    ];
    autos[1].order = 1;
    let main = "[gd_scene format=3]\n[ext_resource type=\"Script\" path=\"res://main.gd\" id=\"1\"]\n[node name=\"Root\" type=\"Node\"]\nscript = ExtResource(\"1\")\n";
    let r = report(
        &[("res://main.gd", source)],
        &[("res://main.tscn", main)],
        &autos,
    );
    assert_eq!(r.contexts.len(), 4);
    assert!(
        r.unknowns
            .iter()
            .any(|u| u.message.contains("unproven SceneTree receiver"))
    );
    assert_eq!(r.contexts[2].root, Some(NodePath(String::new())));
    let matched = r
        .anchors
        .iter()
        .find(|a| {
            a.runtime_path
                .as_ref()
                .is_some_and(|p| p.0 == "/root/Match")
        })
        .unwrap();
    assert_eq!(matched.context_candidates, [0, 2]);
    let extra = r
        .anchors
        .iter()
        .find(|a| {
            a.runtime_path
                .as_ref()
                .is_some_and(|p| p.0 == "/root/MatchExtra")
        })
        .unwrap();
    assert_eq!(extra.context_candidates, [2]);
    assert!(
        r.anchors
            .iter()
            .filter(|a| a.runtime_path.is_none())
            .all(|a| a.context_candidates.is_empty())
    );
}

const CALLER: &str = "extends Node\n@onready var target = $Target\nfunc send():\n    target.rpc(\"fire\")\n    $Target.fire.rpc()\n";
const PLAYER: &str = "extends Node\n@rpc\nfunc fire(): pass\n";
fn linked_scene(target: &str) -> String {
    format!(
        r#"[gd_scene format=3]
[ext_resource type="Script" path="res://main.gd" id="1"]
[ext_resource type="Script" path="{target}" id="2"]
[node name="Root" type="Node"]
script = ExtResource("1")
[node name="Target" type="Node" parent="."]
unique_name_in_owner = true
script = ExtResource("2")
[node name="Sync" type="MultiplayerSynchronizer" parent="Target"]
"#
    )
}

#[test]
fn receiver_resolution_prefers_same_scene_before_global_labels() {
    let a = linked_scene("res://player.gd");
    let b = linked_scene("res://wrong.gd").replace("res://main.gd", "res://other.gd");
    let r = report(
        &[
            ("res://main.gd", CALLER),
            ("res://player.gd", PLAYER),
            ("res://wrong.gd", PLAYER),
        ],
        &[("res://a.tscn", &a), ("res://b.tscn", &b)],
        &[],
    );
    for call in &r.calls {
        assert_eq!(call.candidates.len(), 1, "{call:#?}");
        assert_eq!(
            r.endpoints[call.candidates[0]].script,
            res("res://player.gd")
        );
    }
}

#[test]
fn inner_class_rpcs_are_reported() {
    let source = r#"class_name Outer
extends Node
@rpc
func ping(): pass
class Inner extends Node:
    @rpc
    func ping(): pass
    func send(): ping.rpc()
    class Nested extends Node:
        @rpc
        func ping(): pass
        func send(): self.rpc("ping")
"#;
    let r = report(&[("res://main.gd", source)], &[], &[]);
    assert_eq!(r.endpoints.len(), 3);
    assert_eq!(r.endpoints[1].class.as_deref(), Some("Outer.Inner"));
    assert_eq!(r.endpoints[2].class.as_deref(), Some("Outer.Inner.Nested"));
    for call in &r.calls {
        assert_eq!(call.candidates.len(), 1);
        assert_eq!(r.endpoints[call.candidates[0]].class, call.class);
    }
}

#[test]
fn report_json_is_deterministic_across_runs() {
    let a = linked_scene("res://player.gd");
    let b = linked_scene("res://other.gd");
    let mut scripts = vec![
        ("res://main.gd", CALLER),
        ("res://player.gd", PLAYER),
        ("res://other.gd", PLAYER),
    ];
    let mut scenes = vec![("res://a.tscn", a.as_str()), ("res://b.tscn", b.as_str())];
    let first = serde_json::to_string(&report(&scripts, &scenes, &[])).unwrap();
    scripts.reverse();
    scenes.reverse();
    let second = serde_json::to_string(&report(&scripts, &scenes, &[])).unwrap();
    assert_eq!(first, second);
    let r: serde_json::Value = serde_json::from_str(&first).unwrap();
    assert_eq!(r["schema_version"], 3);
    assert_eq!(r["calls"][0]["location"]["resource"], "res://main.gd");
    assert!(r["calls"][0]["location"].get("path").is_none());
}

#[test]
fn explain_matches_method_receiver_method_scene_and_node_queries() {
    let a = linked_scene("res://player.gd");
    let r = report(
        &[
            ("res://main.gd", CALLER),
            ("res://player.gd", PLAYER),
            ("res://a.gd", "@rpc\nfunc aaa(): pass\n"),
        ],
        &[("res://a.tscn", &a)],
        &[],
    );
    for query in [
        "fire",
        "target.fire",
        "Target.fire",
        "res://a.tscn",
        "Target/Sync",
    ] {
        let e = explain(&r, query);
        assert!(e.matched, "{query}: {e:#?}");
        for call in &e.calls {
            for &index in &call.candidates {
                assert!(index < e.endpoints.len(), "{query}: {e:#?}");
                assert_eq!(e.endpoints[index].method, "fire");
            }
        }
        if query == "fire" {
            assert_eq!(e.endpoints.len(), 1);
            assert_eq!(e.calls.len(), 2);
            assert_eq!(e.synchronizers.len(), 1);
            assert_eq!(e.calls[0].candidates, [0]); // full report had aaa at index 0
        }
    }
    let miss = explain(&r, "does_not_exist");
    assert!(!miss.matched);
    assert!(miss.notes.iter().any(|n| n.contains("no observations")));
}

#[test]
fn dynamic_calls_and_invalid_annotations_are_not_guessed() {
    let script = scan(
        r#"extends Node
@rpc("authority", "any_peer")
func bad(): pass
@rpc(MODE)
func unresolved(): pass
@rpc
func fire(): pass
func send(object, method):
    rpc(method)
    object.rpc("fire")
    Callable(object, method).rpc_id(peer())
    var text = "fire.rpc()"
    # fire.rpc()
"#,
    );
    assert_eq!(script.endpoints.len(), 1);
    assert_eq!(script.calls.len(), 3);
    assert!(script.calls.iter().all(|c| c.method.is_none()));
    assert_eq!(script.calls[1].form, CallForm::AmbiguousRpc);
    assert!(
        script
            .unknowns
            .iter()
            .any(|u| u.message.contains("permission"))
    );
    assert!(
        script
            .unknowns
            .iter()
            .any(|u| u.message.contains("cannot resolve"))
    );
}

#[test]
fn local_names_do_not_resolve_to_autoloads_or_onready_fields() {
    let source = "extends Node\n@onready var target = $Target\nfunc send(target: Node, Net: Node):\n    target.rpc(\"fire\")\n    Net.rpc(\"fire\")\n";
    let scene = linked_scene("res://player.gd");
    let r = report(
        &[("res://main.gd", source), ("res://player.gd", PLAYER)],
        &[("res://a.tscn", &scene)],
        &[autoload("Net", "res://player.gd", AutoloadKind::Script)],
    );
    assert_eq!(r.calls.len(), 2);
    assert!(r.calls.iter().all(|c| c.candidates.is_empty()));
}

#[test]
fn autoloads_and_unique_node_receivers_link_without_global_method_fallback() {
    let source = "extends Node\nfunc send():\n    Net.rpc(\"fire\")\n    %Target.rpc(\"fire\")\n    get_node(\"/root/Net\").rpc(\"fire\")\n    mystery.rpc(\"fire\")\n";
    let scene = linked_scene("res://player.gd");
    let r = report(
        &[("res://main.gd", source), ("res://player.gd", PLAYER)],
        &[("res://a.tscn", &scene)],
        &[autoload("Net", "res://player.gd", AutoloadKind::Script)],
    );
    assert_eq!(r.calls.len(), 4);
    assert!(
        r.calls[..3].iter().all(|c| c.candidates.len() == 1),
        "{:#?}",
        r.calls
    );
    assert!(r.calls[3].candidates.is_empty());
    assert!(r.autoloads[0].networked);
}

#[test]
fn damaged_and_unsupported_project_inputs_remain_visible() {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in [
        (
            "project.godot",
            "config_version=5\n[autoload]\nNet=\"*uid://missing\"\n",
        ),
        ("good.gd", PLAYER),
        ("bad.gd", "func broken(:\n pass\n"),
        ("bad.tscn", "not a scene"),
        ("code.cs", "class Network {}"),
        ("binary.scn", "binary"),
    ] {
        std::fs::write(dir.path().join(path), text).unwrap();
    }
    std::fs::write(dir.path().join("not_utf8.gd"), [0xff]).unwrap();
    let project = gdview::Project::open(dir.path()).unwrap();
    let r = analyze_project(&project).unwrap();
    assert_eq!(r.endpoints.len(), 1);
    assert!(!r.autoloads[0].autoload.exists);
    for message in [
        "incomplete script parse",
        "scene/resource parse failed",
        "unsupported language",
        "not valid UTF-8",
        "unresolved target",
    ] {
        assert!(
            r.unknowns.iter().any(|u| u.message.contains(message)),
            "{message}: {:#?}",
            r.unknowns
        );
    }
    assert!(!dir.path().join(".godot").exists());
}
