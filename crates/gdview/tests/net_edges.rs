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
