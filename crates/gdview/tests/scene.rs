// Acceptance tests for gdview::scene.

use gdview::respath::NodePath;
use gdview::scene::{FileKind, Resolved, ResourceRef, Value, parse};

const SCENE: &str = r#"[gd_scene load_steps=4 format=3 uid="uid://b1player"]

[ext_resource type="Script" uid="uid://c2script" path="res://player.gd" id="1_abc"]
[ext_resource type="PackedScene" path="res://gun.tscn" id="2_gun"]

[sub_resource type="RectangleShape2D" id="Shape_1"]
size = Vector2(10, 20)

[node name="Player" type="CharacterBody2D" groups=["players", "actors"]]
script = ExtResource("1_abc")
speed = 4.5
title = "multi
line \"quoted\""

[node name="Shape" type="CollisionShape2D" parent="."]
shape = SubResource("Shape_1")

[node name="Gun" parent="." instance=ExtResource("2_gun")]

[node name="Muzzle" type="Marker2D" parent="Gun"]
unique_name_in_owner = true

[node name="Later" parent="." instance_placeholder="res://later.tscn"]

[connection signal="fired" from="Gun" to="." method="_on_gun_fired" flags=3 binds=[1, "x"] unbinds=1]
[connection signal="ready" from="." to="Gun" method="arm"]

[editable path="Gun"]
"#;

#[test]
fn parses_header_ext_sub_nodes_connections_and_editable_paths() {
    let scene = parse(SCENE).unwrap();
    assert_eq!(scene.kind, FileKind::Scene);
    assert_eq!((scene.format, scene.load_steps), (3, Some(4)));
    assert_eq!(scene.uid.as_ref().unwrap().0, "uid://b1player");
    assert_eq!(scene.ext_resources.len(), 2);
    let script = scene.ext_resource("1_abc").unwrap();
    assert_eq!(
        (script.type_name.as_str(), script.path.as_str()),
        ("Script", "res://player.gd")
    );
    assert_eq!(script.uid.as_ref().unwrap().0, "uid://c2script");
    assert!(scene.ext_resource("2_gun").unwrap().uid.is_none());
    assert_eq!(
        scene.sub_resource("Shape_1").unwrap().type_name,
        "RectangleShape2D"
    );
    let names: Vec<_> = scene.nodes.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, ["Player", "Shape", "Gun", "Muzzle", "Later"]);
    assert_eq!(scene.connections.len(), 2);
    let fired = &scene.connections[0];
    assert_eq!(
        (
            fired.signal.as_str(),
            fired.from.0.as_str(),
            fired.to.0.as_str(),
            fired.method.as_str()
        ),
        ("fired", "Gun", ".", "_on_gun_fired")
    );
    assert_eq!((fired.flags, fired.unbinds), (Some(3), 1));
    assert_eq!(fired.binds, [Value::Int(1), Value::Str("x".into())]);
    assert_eq!(
        (scene.connections[1].flags, scene.connections[1].unbinds),
        (None, 0)
    );
    assert_eq!(scene.editable_instances, [NodePath("Gun".into())]);
    assert!(scene.resource.is_none());
    let root = scene.root().unwrap();
    assert_eq!(root.script, Some(ResourceRef::Ext("1_abc".into())));
    assert!(
        matches!(scene.resolve(root.script.as_ref().unwrap()), Some(Resolved::Ext(r)) if r.id == "1_abc")
    );
    assert!(scene.resolve(&ResourceRef::Sub("missing".into())).is_none());
}

#[test]
fn node_properties_keep_typed_values() {
    let source = r#"[gd_scene format=3]

[node name="Root" type="Node"]
i = -3
f = 1.5
e = 1e-05
s = "a\nb\\cé"
b = false
n = null
a = [1, 2.0, "three"]
d = {
"k": Vector2(1, 2),
3: [true]
}
v = Vector2(1, -2)
p = NodePath("A/B")
ext = ExtResource("1_x")
sub = SubResource("S_1")
packed = PackedStringArray("a", "b")
sn = &"name"
metadata/_edit_lock_ = true
"#;
    let scene = parse(source).unwrap();
    let props = &scene.root().unwrap().properties;
    let call = |name: &str, args: Vec<Value>| Value::Call {
        name: name.into(),
        args,
    };
    assert_eq!(props["i"], Value::Int(-3));
    assert_eq!(props["f"], Value::Float(1.5));
    assert_eq!(props["e"], Value::Float(1e-05));
    assert_eq!(props["s"], Value::Str("a\nb\\cé".into()));
    assert_eq!(props["b"], Value::Bool(false));
    assert_eq!(props["n"], Value::Null);
    assert_eq!(
        props["a"],
        Value::Array(vec![
            Value::Int(1),
            Value::Float(2.0),
            Value::Str("three".into())
        ])
    );
    assert_eq!(
        props["d"],
        Value::Dict(vec![
            (
                Value::Str("k".into()),
                call("Vector2", vec![Value::Int(1), Value::Int(2)])
            ),
            (Value::Int(3), Value::Array(vec![Value::Bool(true)])),
        ])
    );
    assert_eq!(
        props["v"],
        call("Vector2", vec![Value::Int(1), Value::Int(-2)])
    );
    assert_eq!(props["p"], call("NodePath", vec![Value::Str("A/B".into())]));
    assert_eq!(
        props["ext"].as_resource_ref(),
        Some(ResourceRef::Ext("1_x".into()))
    );
    assert_eq!(
        props["sub"].as_resource_ref(),
        Some(ResourceRef::Sub("S_1".into()))
    );
    assert_eq!(
        props["packed"],
        call(
            "PackedStringArray",
            vec![Value::Str("a".into()), Value::Str("b".into())]
        )
    );
    assert_eq!(props["sn"], Value::StringName("name".into()));
    assert_eq!(props["sn"].as_str(), Some("name"));
    assert_eq!(props["metadata/_edit_lock_"], Value::Bool(true));
}

#[test]
fn every_entry_carries_its_line_number() {
    let scene = parse(SCENE).unwrap();
    assert_eq!(
        scene
            .ext_resources
            .iter()
            .map(|r| r.line)
            .collect::<Vec<_>>(),
        [3, 4]
    );
    assert_eq!(scene.sub_resources[0].line, 6);
    assert_eq!(
        scene.nodes.iter().map(|n| n.line).collect::<Vec<_>>(),
        [9, 15, 18, 20, 23]
    );
    assert_eq!(
        scene.connections.iter().map(|c| c.line).collect::<Vec<_>>(),
        [25, 26]
    );
}

#[test]
fn instance_and_instance_placeholder_are_distinguished() {
    let scene = parse(SCENE).unwrap();
    let gun = scene.node(&NodePath("Gun".into())).unwrap();
    assert_eq!(gun.instance, Some(ResourceRef::Ext("2_gun".into())));
    assert!(gun.instance_placeholder.is_none());
    let later = scene.node(&NodePath("Later".into())).unwrap();
    assert!(later.instance.is_none());
    assert_eq!(
        later.instance_placeholder.as_ref().unwrap().as_str(),
        "res://later.tscn"
    );
    assert!(scene.inherited_base().is_none());
    let inherited = parse("[gd_scene format=3]\n[ext_resource type=\"PackedScene\" path=\"res://base.tscn\" id=\"1\"]\n[node name=\"Base\" instance=ExtResource(\"1\")]\n[node name=\"Extra\" type=\"Node\" parent=\".\"]\n").unwrap();
    assert_eq!(
        inherited.inherited_base().unwrap().path.as_str(),
        "res://base.tscn"
    );
}

#[test]
fn groups_and_unique_names_are_read() {
    let scene = parse(SCENE).unwrap();
    assert_eq!(scene.root().unwrap().groups, ["players", "actors"]);
    assert!(
        scene
            .node(&NodePath("Gun/Muzzle".into()))
            .unwrap()
            .unique_name_in_owner
    );
    assert!(!scene.root().unwrap().unique_name_in_owner);
}

#[test]
fn parent_of_root_is_none_and_paths_resolve_from_root_name() {
    let scene = parse(SCENE).unwrap();
    let root = scene.root().unwrap();
    assert!(root.parent.is_none());
    assert_eq!(root.path(), NodePath(".".into()));
    let paths: Vec<_> = scene.nodes.iter().map(|n| n.path().0).collect();
    assert_eq!(paths, [".", "Shape", "Gun", "Gun/Muzzle", "Later"]);
    assert_eq!(scene.node(&NodePath(".".into())).unwrap().name, "Player");
    assert_eq!(scene.node(&NodePath("./Gun".into())).unwrap().name, "Gun");
    let children: Vec<_> = scene
        .children_of(&NodePath(".".into()))
        .map(|n| n.name.as_str())
        .collect();
    assert_eq!(children, ["Shape", "Gun", "Later"]);
    assert_eq!(scene.children_of(&NodePath("Gun".into())).count(), 1);
}

#[test]
fn malformed_sections_error_with_line_number() {
    let line_of = |source: &str| match parse(source) {
        Err(gdview::Error::Parse { line, .. }) => line,
        other => panic!("expected a parse error, got {other:?}"),
    };
    assert_eq!(
        line_of("[gd_scene format=3]\n\n[node name=\"A\" type=\"Node\"\n"),
        3
    );
    assert_eq!(
        line_of(
            "[gd_scene format=3]\n[node name=\"A\" type=\"Node\"]\n[node name=\"B\" type=\"Node\"]\n"
        ),
        3
    );
    assert_eq!(
        line_of("[gd_scene format=3]\n[node name=\"A\" type=\"Node\"]\nx = Vector2(1,\n"),
        4
    );
    assert_eq!(
        line_of(
            "[gd_scene format=3]\n[ext_resource type=\"Script\" path=\"scripts\\\\a.gd\" id=\"1\"]\n"
        ),
        2
    );
    assert_eq!(
        line_of("[gd_scene format=3]\n[node name=\"A\" type=\"Node\"]\nkey = \"unterminated\n"),
        3
    );
    assert!(parse("").is_err());
    assert!(parse("[node name=\"A\"]\n").is_err());
}

#[test]
fn tres_parses_with_the_same_grammar() {
    let source = r#"[gd_resource type="Resource" script_class="WeaponDefinition" load_steps=3 format=3 uid="uid://w"]

[ext_resource type="Script" path="res://weapon_definition.gd" id="1"]
[ext_resource type="Texture2D" uid="uid://tex" path="res://icon.png" id="2"]

[sub_resource type="Curve" id="Curve_1"]
_data = [Vector2(0, 0), 0.0, 0.0, 0, 0]

[resource]
script = ExtResource("1")
icon = ExtResource("2")
falloff = SubResource("Curve_1")
damage = 12
"#;
    let resource = parse(source).unwrap();
    assert_eq!(resource.kind, FileKind::Resource);
    assert!(resource.nodes.is_empty());
    assert_eq!(resource.ext_resources.len(), 2);
    let props = resource.resource.as_ref().unwrap();
    assert_eq!(props["damage"], Value::Int(12));
    assert_eq!(
        props["script"].as_resource_ref(),
        Some(ResourceRef::Ext("1".into()))
    );
    assert_eq!(resource.sub_resources[0].properties.len(), 1);
}
