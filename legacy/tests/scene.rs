use gdkit::scene::parse;

#[test]
fn renders_literal_nodes_scripts_instances_and_unique_names() {
    let source = r#"[gd_scene format=3]

[ext_resource type="Script" path="res://menu.gd" id="1_script"]
[ext_resource type="PackedScene" path="res://row.tscn" id="2_row"]

[node name="Menu" type="CanvasLayer"]
script = ExtResource("1_script")

[node name="Panel" type="PanelContainer" parent="."]
rows = [
[
"ignored"
],
]

[node name="Rows" type="VBoxContainer" parent="Panel"]
unique_name_in_owner = true

[node name="Row" parent="Panel/Rows" instance=ExtResource("2_row")]
"#;
    let scene = parse(source).unwrap();
    assert_eq!(scene.nodes.len(), 4);
    assert_eq!(scene.external_resources.len(), 2);
    assert_eq!(
        scene.compact_tree().unwrap(),
        "Menu : CanvasLayer [script: res://menu.gd]\n\\- Panel : PanelContainer\n   \\- %Rows : VBoxContainer\n      \\- Row [instance: res://row.tscn]\n"
    );
}

#[test]
fn represents_inherited_nodes_and_missing_inherited_parents() {
    let source = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://weapon.tscn" id="1_weapon"]

[node name="Weapon" instance=ExtResource("1_weapon")]

[node name="Body" parent="." index="0"]

[node name="Muzzle" type="Marker3D" parent="Slide" index="0"]
"#;
    assert_eq!(
        parse(source).unwrap().compact_tree().unwrap(),
        "Weapon [instance: res://weapon.tscn]\n|- Body [inherited]\n\\- Slide [inherited parent]\n   \\- Muzzle : Marker3D\n"
    );
}

#[test]
fn rejects_malformed_scene_structure() {
    assert_eq!(
        parse("[node name=\"Root\" type=\"Node\"]\n")
            .unwrap_err()
            .to_string(),
        "missing gd_scene header"
    );
    assert_eq!(
        parse("[gd_scene format=3]\n").unwrap_err().to_string(),
        "scene contains no nodes"
    );
    assert_eq!(
        parse("[gd_scene format=3]\n[node type=\"Node\"]\n")
            .unwrap_err()
            .to_string(),
        "missing name attribute at line 2"
    );
}
