use gdview::net::{self, CallForm};
use gdview::{Project, ResPath};
use std::fs;

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("project.godot"), "config_version=5\n").unwrap();
    for (path, text) in files {
        fs::write(dir.path().join(path), text).unwrap();
    }
    dir
}
fn analyze(dir: &tempfile::TempDir) -> net::NetReport {
    net::analyze_project(&Project::open(dir.path()).unwrap()).unwrap()
}

#[test]
fn receiver_shadows_include_fields_loops_lambdas_and_match_patterns() {
    let source = r#"extends Node
var Network
var callback = func(Network): Network.rpc("fire")
func send():
    Network.rpc("fire")
    for Network in []:
        Network.rpc("fire")
    match 1:
        var Network:
            Network.rpc("fire")
"#;
    let dir = project(&[
        (
            "project.godot",
            "config_version=5\n[autoload]\nNetwork=\"*res://network.gd\"\n",
        ),
        ("network.gd", "extends Node\n@rpc\nfunc fire(): pass\n"),
        ("caller.gd", source),
    ]);
    let r = analyze(&dir);
    assert_eq!(r.calls.len(), 4);
    assert!(
        r.calls
            .iter()
            .all(|c| c.candidates.is_empty() && c.receiver_is_local),
        "{:#?}",
        r.calls
    );
}

#[test]
fn singleton_registration_is_required_for_global_name_but_not_absolute_path() {
    let dir = project(&[
        (
            "project.godot",
            "config_version=5\n[autoload]\nNetwork=\"res://network.gd\"\n",
        ),
        ("network.gd", "extends Node\n@rpc\nfunc fire(): pass\n"),
        (
            "caller.gd",
            "extends Node\nfunc send():\n    Network.rpc(\"fire\")\n    get_node(\"/root/Network\").rpc(\"fire\")\n",
        ),
    ]);
    let r = analyze(&dir);
    assert!(r.calls[0].candidates.is_empty());
    assert_eq!(r.calls[1].candidates.len(), 1);
}

#[test]
fn duplicate_uids_do_not_choose_an_arbitrary_autoload() {
    let dir = project(&[
        (
            "project.godot",
            "config_version=5\n[autoload]\nNetwork=\"*uid://same\"\n",
        ),
        ("a.gd", "@rpc\nfunc fire(): pass\n"),
        ("b.gd", "@rpc\nfunc fire(): pass\n"),
        ("a.gd.uid", "uid://same\n"),
        ("b.gd.uid", "uid://same\n"),
        ("caller.gd", "func send(): Network.rpc(\"fire\")\n"),
    ]);
    let r = analyze(&dir);
    assert!(r.autoloads[0].autoload.path.is_none());
    assert!(r.calls[0].candidates.is_empty());
    assert!(
        r.unknowns
            .iter()
            .any(|u| u.message.contains("duplicate uid://same"))
    );
}

#[test]
fn callable_shadowing_and_multiline_string_names_do_not_guess_method_names() {
    let source = "extends Node\r\n@rpc\r\nfunc fire(): pass\r\nfunc send():\r\n    self.rpc(\r\n        &\"fi\\u0072e\"\r\n    )\r\n    var fire = func(): pass\r\n    fire.rpc()\r\n";
    let s = net::scan_script(ResPath::parse("res://a.gd").unwrap(), source);
    assert_eq!(s.calls.len(), 2);
    assert_eq!(s.calls[0].location.line, 5);
    assert_eq!(s.calls[0].method.as_deref(), Some("fire"));
    assert_eq!(s.calls[1].form, CallForm::AmbiguousRpc);
    assert!(s.calls[1].method.is_none());
}

#[test]
fn explanation_context_indexes_are_remapped_and_runtime_roots_are_not_invented() {
    let dir = project(&[
        (
            "project.godot",
            "config_version=5\n[autoload]\nNetwork=\"*res://network.gd\"\n",
        ),
        (
            "network.gd",
            "extends Node\n@rpc\nfunc fire(): pass\nfunc setup(api):\n    get_tree().set_multiplayer(api, ^\"/root/Elsewhere\")\n    get_tree().set_multiplayer(api, ^\"/root/Network\")\n",
        ),
        (
            "main.tscn",
            "[gd_scene format=3]\n[ext_resource type=\"Script\" path=\"res://network.gd\" id=\"1\"]\n[node name=\"Main\" type=\"Node\"]\nscript = ExtResource(\"1\")\n",
        ),
    ]);
    let r = analyze(&dir);
    assert_eq!(r.contexts.len(), 2);
    let e = net::explain(&r, "fire");
    assert_eq!(e.contexts.len(), 1);
    assert_eq!(e.contexts[0].root.as_ref().unwrap().0, "/root/Network");
    assert!(
        e.anchors
            .iter()
            .filter(|a| a.runtime_path.is_some())
            .all(|a| a.context_candidates == [0])
    );
    assert!(
        e.anchors
            .iter()
            .filter(|a| a.scene.is_some())
            .all(|a| a.runtime_path.is_none() && a.context_candidates.is_empty())
    );
}

#[test]
fn instance_and_inheritance_limits_are_explicit_without_fake_expansion() {
    let dir = project(&[
        (
            "scene.tscn",
            "[gd_scene format=3]\n[ext_resource type=\"PackedScene\" path=\"res://base.tscn\" id=\"1\"]\n[node name=\"Inherited\" instance=ExtResource(\"1\")]\n",
        ),
        (
            "base.tscn",
            "[gd_scene format=3]\n[node name=\"Root\" type=\"Node\"]\n[node name=\"Sync\" type=\"MultiplayerSynchronizer\" parent=\".\"]\n",
        ),
    ]);
    let r = analyze(&dir);
    assert_eq!(r.synchronizers.len(), 1);
    assert_eq!(r.synchronizers[0].scene.as_str(), "res://base.tscn");
    assert!(
        r.unknowns
            .iter()
            .any(|u| u.message.contains("not expanded"))
    );
}

#[test]
fn onready_fields_take_precedence_over_same_named_singletons() {
    let dir = project(&[
        (
            "project.godot",
            "config_version=5\n[autoload]\nNetwork=\"*res://wrong.gd\"\n",
        ),
        ("wrong.gd", "extends Node\n@rpc\nfunc fire(): pass\n"),
        ("right.gd", "extends Node\n@rpc\nfunc fire(): pass\n"),
        (
            "caller.gd",
            "extends Node\n@onready var Network = $Target\nfunc send():\n    Network.rpc(\"fire\")\n    self.Network.rpc(\"fire\")\n",
        ),
        (
            "scene.tscn",
            "[gd_scene format=3]\n[ext_resource type=\"Script\" path=\"res://caller.gd\" id=\"1\"]\n[ext_resource type=\"Script\" path=\"res://right.gd\" id=\"2\"]\n[node name=\"Root\" type=\"Node\"]\nscript = ExtResource(\"1\")\n[node name=\"Target\" type=\"Node\" parent=\".\"]\nscript = ExtResource(\"2\")\n",
        ),
    ]);
    let r = analyze(&dir);
    assert_eq!(r.calls.len(), 2);
    for c in &r.calls {
        assert_eq!(c.candidates.len(), 1, "{c:#?}");
        assert_eq!(
            r.endpoints[c.candidates[0]].script.as_str(),
            "res://right.gd"
        );
    }
}

const PLAYER_SCENE: &str = r#"[gd_scene format=3]
[ext_resource type="Script" path="res://main.gd" id="1"]
[ext_resource type="Script" path="res://player.gd" id="2"]
[node name="Main" type="Node"]
script = ExtResource("1")
[node name="Player" type="Node" parent="."]
script = ExtResource("2")
"#;
const PLAYER: &str = "extends Node\nvar weapon\n@rpc\nfunc fire(): pass\n@rpc\nfunc hit(): pass\nfunc check():\n    is_multiplayer_authority()\n";

#[test]
fn node_members_are_callables_only_when_the_target_declares_the_function() {
    let main = "extends Node\nfunc send():\n    $Player.weapon.rpc(\"fire\")\n    $Player.fire.rpc()\n    $Missing.fire.rpc()\n";
    let dir = project(&[
        ("main.gd", main),
        ("player.gd", PLAYER),
        ("main.tscn", PLAYER_SCENE),
    ]);
    let r = analyze(&dir);
    let at = |line| r.calls.iter().find(|c| c.location.line == line).unwrap();
    let property = at(3);
    assert_eq!(property.form, CallForm::AmbiguousRpc, "{property:#?}");
    assert_eq!(property.method, None);
    assert_eq!(property.receiver.as_deref(), Some("$Player.weapon"));
    assert!(property.candidates.is_empty());
    let method = at(4);
    assert_eq!(method.form, CallForm::CallableRpc);
    assert_eq!(method.candidates.len(), 1);
    assert_eq!(r.endpoints[method.candidates[0]].method, "fire");
    let messages = |line| {
        r.unknowns
            .iter()
            .filter(move |u| u.location.as_ref().unwrap().line == line)
            .map(|u| u.message.as_str())
            .collect::<Vec<_>>()
    };
    assert!(
        messages(3)
            .iter()
            .any(|m| m.contains("weapon is not a function"))
    );
    assert!(messages(5).iter().any(|m| m.contains("fire is unverified")));
}

#[test]
fn linking_withdraws_scan_unknowns_only_for_calls_it_resolved() {
    let main = "extends Node\n@onready var player = $Player\nfunc send():\n    player.rpc(\"hit\")\n    player.rpc(\"hit\"); mystery.rpc(\"hit\")\n";
    let dir = project(&[
        ("main.gd", main),
        ("player.gd", PLAYER),
        ("main.tscn", PLAYER_SCENE),
    ]);
    let r = analyze(&dir);
    let resolved = r.calls.iter().find(|c| c.location.line == 4).unwrap();
    assert_eq!(resolved.form, CallForm::Rpc);
    assert_eq!(resolved.candidates.len(), 1);
    let on = |line| {
        r.unknowns
            .iter()
            .filter(|u| u.location.as_ref().unwrap().line == line)
            .count()
    };
    assert_eq!(on(4), 0, "{:#?}", r.unknowns);
    // `mystery` on the same line is still ambiguous, so its unknowns stay.
    assert!(
        r.unknowns
            .iter()
            .any(|u| u.location.as_ref().unwrap().line == 5
                && u.message.contains("unproven receiver"))
    );
}

#[test]
fn explaining_a_node_includes_its_scripts_authority_uses() {
    let dir = project(&[
        ("main.gd", "extends Node\n"),
        ("player.gd", PLAYER),
        ("main.tscn", PLAYER_SCENE),
    ]);
    let r = analyze(&dir);
    let by_node = net::explain(&r, "Player");
    let by_script = net::explain(&r, "res://player.gd");
    assert_eq!(by_node.authority_uses.len(), 1);
    assert_eq!(by_node.authority_uses, by_script.authority_uses);
}

#[test]
fn replication_entries_after_a_gap_or_rejected_path_are_dropped() {
    let config = |entries: &str| {
        format!(
            "[gd_scene format=3]\n[sub_resource type=\"SceneReplicationConfig\" id=\"c\"]\n{entries}[node name=\"Root\" type=\"Node\"]\n[node name=\"Sync\" type=\"MultiplayerSynchronizer\" parent=\".\"]\nreplication_config = SubResource(\"c\")\n"
        )
    };
    let paths = |entries: &str| {
        let dir = project(&[("main.tscn", &config(entries))]);
        let r = analyze(&dir);
        let paths: Vec<_> = r.synchronizers[0]
            .properties
            .iter()
            .map(|p| p.path.0.clone())
            .collect();
        (paths, r.unknowns)
    };
    // Eleven entries: numeric, not lexicographic, index order.
    let eleven: String = (0..11)
        .map(|i| format!("properties/{i}/path = NodePath(\".:p{i}\")\n"))
        .collect();
    let (all, unknowns) = paths(&eleven);
    assert_eq!(all.len(), 11);
    assert_eq!(all[2], ".:p2");
    assert_eq!(all[10], ".:p10");
    assert!(unknowns.is_empty(), "{unknowns:#?}");
    // Engine-verified (Godot 4.7.2): both drop the entry and everything after it.
    let (gap, unknowns) = paths(
        "properties/0/path = NodePath(\".:position\")\nproperties/2/path = NodePath(\".:rotation\")\n",
    );
    assert_eq!(gap, [".:position"]);
    assert!(unknowns.iter().any(|u| u.message.contains("follows a gap")));
    let (no_subname, unknowns) =
        paths("properties/0/path = NodePath(\".\")\nproperties/1/path = NodePath(\".:scale\")\n");
    assert!(no_subname.is_empty());
    assert!(
        unknowns
            .iter()
            .any(|u| u.message.contains("property subname"))
    );
}
