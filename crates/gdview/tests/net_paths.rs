use gdview::{ResPath, net, scene, uid::UidMap};

fn res(path: &str) -> ResPath {
    ResPath::parse(path).unwrap()
}
fn authored_scene(target: &str) -> scene::SceneFile {
    scene::parse(&format!(
        r#"[gd_scene format=3]
[ext_resource type="Script" path="res://caller.gd" id="1"]
[ext_resource type="Script" path="{target}" id="2"]
[node name="Root" type="Node"]
[node name="Sender" type="Node" parent="."]
script = ExtResource("1")
[node name="Target" type="Node" parent="."]
script = ExtResource("2")
"#
    ))
    .unwrap()
}

#[test]
fn relative_receiver_paths_traverse_existing_nodes_without_escaping_scene_roots() {
    let scripts = [
        net::scan_script(
            res("res://caller.gd"),
            "extends Node\nfunc send():\n    get_node(\"../Target\").rpc(\"fire\")\n    get_node(\"Missing/../../Target\").rpc(\"fire\")\n    get_node(\"../../Root/Target\").rpc(\"fire\")\n",
        ),
        net::scan_script(res("res://target.gd"), "@rpc\nfunc fire(): pass\n"),
    ];
    let scenes = [(res("res://main.tscn"), authored_scene("res://target.gd"))];
    let r = net::analyze(&net::NetInput {
        scripts: &scripts,
        scenes: &scenes,
        autoloads: &[],
        uids: &UidMap::default(),
        unknowns: &[],
    });
    assert_eq!(r.calls[0].candidates, [0]);
    assert!(r.calls[1].candidates.is_empty());
    assert!(r.calls[2].candidates.is_empty());
}

#[test]
fn repeated_script_attachments_retain_all_candidates_and_explain_ambiguity() {
    let scripts = [
        net::scan_script(
            res("res://caller.gd"),
            "extends Node\nfunc send(): get_node(\"../Target\").rpc(\"fire\")\n",
        ),
        net::scan_script(res("res://a.gd"), "@rpc\nfunc fire(): pass\n"),
        net::scan_script(res("res://b.gd"), "@rpc\nfunc fire(): pass\n"),
    ];
    let scenes = [
        (res("res://first.tscn"), authored_scene("res://a.gd")),
        (res("res://second.tscn"), authored_scene("res://b.gd")),
    ];
    let r = net::analyze(&net::NetInput {
        scripts: &scripts,
        scenes: &scenes,
        autoloads: &[],
        uids: &UidMap::default(),
        unknowns: &[],
    });
    assert_eq!(r.calls[0].candidates, [0, 1]);
    assert!(
        r.unknowns
            .iter()
            .any(|u| u.message.contains("multiple authored"))
    );
    let e = net::explain(&r, "fire");
    assert_eq!(e.calls[0].candidates, [0, 1]);
    assert!(
        e.unknowns
            .iter()
            .any(|u| u.message.contains("multiple authored"))
    );
}

#[test]
fn explanation_includes_spawners_that_reference_the_endpoint_scene() {
    let scripts = [net::scan_script(
        res("res://actor.gd"),
        "extends Node\n@rpc\nfunc fire(): pass\n",
    )];
    let actor = scene::parse("[gd_scene format=3]\n[ext_resource type=\"Script\" path=\"res://actor.gd\" id=\"1\"]\n[node name=\"Actor\" type=\"Node\"]\nscript = ExtResource(\"1\")\n").unwrap();
    let host = scene::parse("[gd_scene format=3]\n[node name=\"Host\" type=\"Node\"]\n[node name=\"Spawner\" type=\"MultiplayerSpawner\" parent=\".\"]\n_spawnable_scenes = PackedStringArray(\"res://actor.tscn\")\n").unwrap();
    let scenes = [
        (res("res://actor.tscn"), actor),
        (res("res://host.tscn"), host),
    ];
    let r = net::analyze(&net::NetInput {
        scripts: &scripts,
        scenes: &scenes,
        autoloads: &[],
        uids: &UidMap::default(),
        unknowns: &[],
    });
    let e = net::explain(&r, "fire");
    assert_eq!(e.spawners.len(), 1);
    assert_eq!(e.spawners[0].scene.as_str(), "res://host.tscn");
}

#[test]
fn shadowed_builtin_method_names_are_not_confirmed_network_operations() {
    let scripts = [net::scan_script(
        res("res://actor.gd"),
        "extends Node\n@rpc\nfunc fire(): pass\nfunc send(rpc, is_multiplayer_authority):\n    rpc(\"fire\")\n    is_multiplayer_authority()\n",
    )];
    let r = net::analyze(&net::NetInput {
        scripts: &scripts,
        scenes: &[],
        autoloads: &[],
        uids: &UidMap::default(),
        unknowns: &[],
    });
    assert_eq!(r.calls.len(), 1);
    assert_eq!(r.calls[0].form, net::CallForm::AmbiguousRpc);
    assert!(r.calls[0].candidates.is_empty());
    assert!(r.authority_uses.is_empty());
    assert!(
        r.unknowns
            .iter()
            .any(|u| u.message.contains("unproven authority"))
    );
}
