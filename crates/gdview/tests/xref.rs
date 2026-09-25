// Acceptance tests for gdview::xref.

use std::fs;

use gdview::Project;
use gdview::declarations::index_project;
use gdview::uid::UidMap;
use gdview::xref::{Finding, FindingKind, ProjectGraph, analyze};

/// Writes a project and returns every finding, as `(kind, "path:line", target)`.
fn findings(files: &[(&str, &str)]) -> Vec<Finding> {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("project.godot"), "config_version=5\n").unwrap();
    for (path, contents) in files {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    let project = Project::open(dir.path()).unwrap();
    let declarations = index_project(&project).unwrap();
    let uids = UidMap::build(&project).unwrap();
    let graph = ProjectGraph::load(&project, &declarations, &uids).unwrap();
    analyze(&graph)
}

fn summary(findings: &[Finding]) -> Vec<(FindingKind, String, String)> {
    findings
        .iter()
        .map(|f| {
            (
                f.kind,
                format!("{}:{}", f.at.path, f.at.line),
                f.target.clone(),
            )
        })
        .collect()
}

fn scene(root_type: &str, script: Option<&str>, body: &str) -> String {
    let mut text = String::from("[gd_scene format=3]\n\n");
    if let Some(script) = script {
        text.push_str(&format!(
            "[ext_resource type=\"Script\" path=\"{script}\" id=\"1_s\"]\n\n"
        ));
    }
    text.push_str(&format!("[node name=\"Root\" type=\"{root_type}\"]\n"));
    if script.is_some() {
        text.push_str("script = ExtResource(\"1_s\")\n");
    }
    text.push_str(body);
    text
}

#[test]
fn connection_to_missing_method_is_reported_with_scene_line_and_script() {
    let main = scene(
        "Node2D",
        Some("res://player.gd"),
        r#"
[node name="Timer" type="Timer" parent="."]

[node name="Plain" type="Node" parent="."]

[connection signal="timeout" from="Timer" to="." method="_on_timer_timout"]
[connection signal="timeout" from="Timer" to="." method="_on_timer_timeout"]
[connection signal="timeout" from="Timer" to="." method="_on_base_thing"]
[connection signal="timeout" from="Timer" to="." method="queue_free"]
[connection signal="timeout" from="Timer" to="Plain" method="_on_anything"]
[connection signal="timeout" from="Timer" to="Ghost" method="_on_timer_timeout"]
[connection signal="timeout" from="Nobody" to="." method="_on_timer_timeout"]
"#,
    );
    let found = findings(&[
        ("base.gd", "extends Node2D\nfunc _on_base_thing(): pass\n"),
        (
            "player.gd",
            "extends \"res://base.gd\"\n\nfunc _on_timer_timeout():\n\tpass\n",
        ),
        ("main.tscn", &main),
    ]);
    assert_eq!(
        summary(&found),
        [
            (
                FindingKind::MissingMethod,
                "res://main.tscn:12".into(),
                "_on_timer_timout".into()
            ),
            (
                FindingKind::MissingNode,
                "res://main.tscn:17".into(),
                "Ghost".into()
            ),
            (
                FindingKind::MissingNode,
                "res://main.tscn:18".into(),
                "Nobody".into()
            ),
        ]
    );
    let missing = &found[0];
    assert!(
        missing.message.contains("res://player.gd"),
        "{}",
        missing.message
    );
    assert_eq!(missing.suggestions, ["_on_timer_timeout"]);
}

#[test]
fn connection_arity_mismatch_is_reported_when_binds_are_known() {
    let main = scene(
        "Node",
        Some("res://receiver.gd"),
        r#"
[ext_resource type="Script" path="res://emitter.gd" id="2_e"]

[node name="Emitter" type="Node" parent="."]
script = ExtResource("2_e")

[node name="Button" type="Button" parent="."]

[connection signal="hit" from="Emitter" to="." method="_on_one"]
[connection signal="hit" from="Emitter" to="." method="_on_one" unbinds=1]
[connection signal="hit" from="Emitter" to="." method="_on_three" binds=[7]]
[connection signal="hit" from="Emitter" to="." method="_on_optional"]
[connection signal="hit" from="Emitter" to="." method="_on_rest" binds=[1, 2, 3]]
[connection signal="hit" from="Emitter" to="." method="_on_three"]
[connection signal="pressed" from="Button" to="." method="_on_one"]
"#,
    );
    let found = findings(&[
        (
            "emitter.gd",
            "extends Node\nsignal hit(amount: int, source)\n",
        ),
        (
            "receiver.gd",
            "extends Node\nfunc _on_one(a): pass\nfunc _on_three(a, b, c): pass\nfunc _on_optional(a, b = 1, c = 2): pass\nfunc _on_rest(a, ...more): pass\n",
        ),
        ("main.tscn", &main),
    ]);
    assert_eq!(
        summary(&found),
        [
            (
                FindingKind::MethodArity,
                "res://main.tscn:15".into(),
                "_on_one".into()
            ),
            (
                FindingKind::MethodArity,
                "res://main.tscn:20".into(),
                "_on_three".into()
            ),
        ]
    );
    assert!(
        found[0].message.contains("passes 2 argument(s)"),
        "{}",
        found[0].message
    );
    assert!(found[0].message.contains("takes 1"), "{}", found[0].message);
}

#[test]
fn node_path_in_onready_is_checked_against_every_owning_scene() {
    let panel = "[gd_scene format=3]\n\n[node name=\"Panel\" type=\"Control\"]\n\n[node name=\"Label\" type=\"Label\" parent=\".\"]\n";
    let with_instance = scene(
        "Control",
        Some("res://ui.gd"),
        "\n[ext_resource type=\"PackedScene\" path=\"res://panel.tscn\" id=\"2_p\"]\n\n[node name=\"Panel\" parent=\".\" instance=ExtResource(\"2_p\")]\n",
    );
    let without_panel = scene("Control", Some("res://ui.gd"), "");
    let subclass_scene = scene(
        "Control",
        Some("res://ui_child.gd"),
        "\n[node name=\"Only\" type=\"Node\" parent=\".\"]\n\n[node name=\"Here\" type=\"Node\" parent=\"Only\"]\n",
    );
    let inherited = "[gd_scene format=3]\n\n[ext_resource type=\"PackedScene\" path=\"res://without_panel.tscn\" id=\"1_b\"]\n\n[node name=\"Root\" instance=ExtResource(\"1_b\")]\n\n[node name=\"Extra\" type=\"Node\" parent=\".\"]\n";
    let found = findings(&[
        (
            "ui.gd",
            "extends Control\n@onready var label = $Panel/Label\n@onready var typo = $Panel/Lable\n@onready var deep = $Only/Here\n@onready var extra = get_node(\"Extra\")\n@onready var everywhere_missing = $Nowhere\n",
        ),
        ("ui_child.gd", "extends \"res://ui.gd\"\n"),
        ("panel.tscn", panel),
        ("with_instance.tscn", &with_instance),
        ("without_panel.tscn", &without_panel),
        ("subclass.tscn", &subclass_scene),
        ("inherited.tscn", inherited),
    ]);
    assert_eq!(
        summary(&found),
        [
            (
                FindingKind::MissingNode,
                "res://ui.gd:3".into(),
                "Panel/Lable".into()
            ),
            (
                FindingKind::MissingNode,
                "res://ui.gd:6".into(),
                "Nowhere".into()
            ),
        ]
    );
    assert_eq!(found[0].suggestions, ["Label"]);
    for scene in [
        "res://with_instance.tscn",
        "res://without_panel.tscn",
        "res://subclass.tscn",
        "res://inherited.tscn",
    ] {
        assert!(
            found[1].message.contains(scene),
            "{scene} missing from {}",
            found[1].message
        );
    }
}

#[test]
fn unique_name_paths_resolve_through_unique_name_in_owner() {
    let hud = scene(
        "Control",
        Some("res://hud.gd"),
        r#"
[node name="Margin" type="MarginContainer" parent="."]

[node name="Score" type="Label" parent="Margin"]
unique_name_in_owner = true

[node name="Icon" type="TextureRect" parent="Margin/Score"]

[node name="Lives" type="Label" parent="Margin"]
"#,
    );
    let found = findings(&[
        (
            "hud.gd",
            "extends Control\n@onready var score = %Score\n@onready var icon = %Score/Icon\n@onready var via_dollar = $%Score\n@onready var lives = %Lives\n@onready var missing = %Scroe\n",
        ),
        ("hud.tscn", &hud),
    ]);
    assert_eq!(
        summary(&found),
        [
            (
                FindingKind::MissingNode,
                "res://hud.gd:5".into(),
                "%Lives".into()
            ),
            (
                FindingKind::MissingNode,
                "res://hud.gd:6".into(),
                "%Scroe".into()
            ),
        ]
    );
    // `Lives` exists but is not unique; `Score` is the unique name one edit away.
    assert_eq!(found[1].suggestions, ["%Score"]);
}

#[test]
fn dynamic_or_unowned_node_paths_produce_no_finding() {
    let owned = scene(
        "Node",
        Some("res://owned.gd"),
        "\n[node name=\"Child\" type=\"Node\" parent=\".\"]\n",
    );
    let unknown_instance = scene(
        "Node",
        Some("res://owned_by_unreadable.gd"),
        "\n[ext_resource type=\"PackedScene\" path=\"res://missing.tscn\" id=\"2_m\"]\n\n[node name=\"Inst\" parent=\".\" instance=ExtResource(\"2_m\")]\n",
    );
    let found = findings(&[
        (
            "unowned.gd",
            "extends Node\n@onready var a = $Does/Not/Exist\n",
        ),
        (
            "owned.gd",
            "extends Node\nvar path = \"X\"\n@onready var up = $\"../Sibling\"\n@onready var absolute = get_node(\"/root/Game\")\n@onready var dynamic = get_node(path)\n@onready var ok = $Child\nfunc _ready():\n\tvar later = $AddedAtRuntime\n",
        ),
        ("owned.tscn", &owned),
        (
            "owned_by_unreadable.gd",
            "extends Node\n@onready var through = $Inst/Anything\n",
        ),
        ("unknown.tscn", &unknown_instance),
    ]);
    // The only finding is the unreadable instance itself; nothing about node paths.
    assert_eq!(
        summary(&found),
        [(
            FindingKind::MissingResource,
            "res://unknown.tscn:8".into(),
            "res://missing.tscn".into()
        )]
    );
}

#[test]
fn missing_preload_ext_resource_instance_and_uid_targets_are_reported() {
    let main = r#"[gd_scene format=3]

[ext_resource type="Texture2D" path="res://art/gone.png" id="1_a"]
[ext_resource type="Texture2D" uid="uid://moved" path="res://art/old_name.png" id="2_b"]
[ext_resource type="Texture2D" path="res://art/icon.png" id="3_c"]
[ext_resource type="Texture2D" uid="uid://nothing" path="res://art/also_gone.png" id="4_d"]

[node name="Root" type="Node"]

[node name="Inst" parent="." instance=ExtResource("9_undeclared")]

[node name="Later" parent="." instance_placeholder="res://later.tscn"]
"#;
    let found = findings(&[
        ("art/icon.png", ""),
        ("art/new_name.png", ""),
        (
            "art/new_name.png.import",
            "[remap]\nimporter=\"texture\"\nuid=\"uid://moved\"\n",
        ),
        (
            "levels/level.tscn",
            "[gd_scene format=3]\n[node name=\"L\" type=\"Node\"]\n",
        ),
        ("scripts/sibling.gd", "extends Node\n"),
        (
            "scripts/loader.gd",
            "extends Node\nconst A = preload(\"res://level.tscn\")\nconst B = preload(\"sibling.gd\")\nconst C = preload(\"../levels/level.tscn\")\nfunc f():\n\tload(\"uid://unknown\")\n\tload(\"user://save.tres\")\n\tload(\"res://levels/%s.tscn\" % 1)\n\tload(\"res://nope.tres\")\n",
        ),
        ("main.tscn", main),
    ]);
    assert_eq!(
        summary(&found),
        [
            (
                FindingKind::MissingResource,
                "res://main.tscn:3".into(),
                "res://art/gone.png".into()
            ),
            (
                FindingKind::MissingResource,
                "res://main.tscn:6".into(),
                "res://art/also_gone.png".into()
            ),
            (
                FindingKind::MissingResource,
                "res://main.tscn:10".into(),
                "9_undeclared".into()
            ),
            (
                FindingKind::MissingResource,
                "res://main.tscn:12".into(),
                "res://later.tscn".into()
            ),
            (
                FindingKind::MissingResource,
                "res://scripts/loader.gd:2".into(),
                "res://level.tscn".into()
            ),
            (
                FindingKind::UnresolvedUid,
                "res://scripts/loader.gd:6".into(),
                "uid://unknown".into()
            ),
            (
                FindingKind::MissingResource,
                "res://scripts/loader.gd:9".into(),
                "res://nope.tres".into()
            ),
        ]
    );
    // A file with the same name elsewhere is the likeliest fix for a move.
    assert_eq!(found[4].suggestions, ["res://levels/level.tscn"]);
}

#[test]
fn extends_path_and_class_name_references_are_checked() {
    let main = scene(
        "Node",
        Some("res://enemy.gd"),
        "\n[node name=\"T\" type=\"Timer\" parent=\".\"]\n\n[connection signal=\"timeout\" from=\"T\" to=\".\" method=\"_on_base_timeout\"]\n",
    );
    let found = findings(&[
        ("orphan.gd", "extends \"res://gone.gd\"\n"),
        ("relative/child.gd", "extends \"../base_actor.gd\"\n"),
        (
            "base_actor.gd",
            "class_name Actor\nextends Node\nfunc _on_base_timeout(): pass\n",
        ),
        ("enemy.gd", "extends Actor\n"),
        ("dupe_a.gd", "class_name Pickup\nextends Node\n"),
        ("dupe_b.gd", "extends Node\nclass_name Pickup\n"),
        ("main.tscn", &main),
    ]);
    assert_eq!(
        summary(&found),
        [
            (
                FindingKind::DuplicateClassName,
                "res://dupe_b.gd:2".into(),
                "Pickup".into()
            ),
            (
                FindingKind::MissingBaseScript,
                "res://orphan.gd:1".into(),
                "res://gone.gd".into()
            ),
        ]
    );
}

#[test]
fn duplicate_uids_and_unparseable_scenes_are_reported() {
    let found = findings(&[
        ("a.gd", "extends Node\n"),
        ("a.gd.uid", "uid://same\n"),
        ("b.gd", "extends Node\n"),
        ("b.gd.uid", "uid://same\n"),
        (
            "broken.tscn",
            "[gd_scene format=3]\n\n[node name=\"A\" type=\"Node\"]\nx = Vector2(1,\n",
        ),
    ]);
    assert_eq!(
        summary(&found),
        [
            (
                FindingKind::DuplicateUid,
                "res://b.gd:1".into(),
                "uid://same".into()
            ),
            (
                FindingKind::UnparseableScene,
                "res://broken.tscn:5".into(),
                "res://broken.tscn".into()
            ),
        ]
    );
}

#[test]
#[ignore = "scaffold: lands with `gdkit refs`"]
fn references_to_lists_every_inbound_reference_for_a_path() {
    todo!()
}

#[test]
fn findings_are_sorted_by_location_and_deterministic() {
    let files = [
        (
            "z.gd",
            "extends Node\nconst A = preload(\"res://missing_z.tscn\")\n",
        ),
        (
            "a.gd",
            "extends Node\nconst B = preload(\"res://missing_b.tscn\")\nconst A = preload(\"res://missing_a.tscn\")\n",
        ),
        (
            "m.tscn",
            "[gd_scene format=3]\n[ext_resource type=\"Script\" path=\"res://gone.gd\" id=\"1\"]\n[node name=\"M\" type=\"Node\"]\n",
        ),
    ];
    let first = findings(&files);
    assert_eq!(first, findings(&files));
    let locations: Vec<_> = first
        .iter()
        .map(|f| (f.at.path.to_string(), f.at.line))
        .collect();
    assert_eq!(
        locations,
        [
            ("res://a.gd".into(), 2),
            ("res://a.gd".into(), 3),
            ("res://m.tscn".into(), 2),
            ("res://z.gd".into(), 2)
        ]
    );
}
