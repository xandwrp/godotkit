use gdkit::scene::{TreeOptions, parse};
use std::{fs, process::Command};

const SCENE: &str = r#"[gd_scene format=3]
[node name="PlayerInstance" type="Node" groups=["replicated"]]
[node name="Health" type="Node" parent="." groups=["damageable", "replicated"]]
unique_name_in_owner = true
[node name="Hud" type="Control" parent="."]
unique_name_in_owner = true
[node name="Button" type="Button" parent="Hud"]
[connection signal="died" from="Health" to="." method="_on_health_died"]
[connection signal="damaged" from="Health" to="Hud" method="_on_damaged" flags=3 binds=[1, Vector2(2, 3)] unbinds=1]
[connection signal="ready" from="." to="Hud/Button" method="show"]
"#;

#[test]
fn renders_relationships_under_their_source_nodes() {
    let scene = parse(SCENE).unwrap();
    assert_eq!(scene.connections.len(), 3);
    assert_eq!(scene.nodes[1].groups, ["damageable", "replicated"]);
    assert_eq!(
        scene
            .compact_tree_with_options(TreeOptions {
                connections: true,
                groups: true
            })
            .unwrap(),
        "PlayerInstance : Node\n   signals:\n      ready -> Hud/Button.show\n   groups: replicated\n|- %Health : Node\n|  signals:\n|     died -> PlayerInstance._on_health_died\n|     damaged -> %Hud._on_damaged [flags: 3] [binds: [1, Vector2(2, 3)]] [unbinds: 1]\n|  groups: damageable, replicated\n\\- %Hud : Control\n   \\- Button : Button\n"
    );
    let plain = scene.compact_tree().unwrap();
    assert!(!plain.contains("signals:"));
    assert!(!plain.contains("groups:"));
}

#[test]
fn parses_group_names_and_reports_malformed_relationships() {
    for value in [
        r#"["with, comma", "quote\"here", &"named"]"#,
        r#"PackedStringArray("with, comma", "quote\"here", "named")"#,
    ] {
        let source =
            format!("[gd_scene format=3]\n[node name=\"Root\" type=\"Node\" groups={value}]\n");
        assert_eq!(
            parse(&source).unwrap().nodes[0].groups,
            ["with, comma", "quote\"here", "named"]
        );
    }
    for value in ["[1]", r#"["a" "b"]"#, "invalid"] {
        let source = format!("[gd_scene format=3]\n[node name=\"Root\" groups={value}]\n");
        assert!(parse(&source).unwrap_err().to_string().contains("line 2"));
    }
    let source = "[gd_scene format=3]\n[node name=\"Root\"]\n[connection signal=\"ready\" from=\".\" to=\".\"]\n";
    assert_eq!(
        parse(source).unwrap_err().to_string(),
        "missing method attribute at line 3"
    );
}

#[test]
fn cli_switches_are_independent_and_preserve_scene_files() {
    let directory =
        std::env::temp_dir().join(format!("gdkit-relationships-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join("player.tscn");
    fs::write(&path, SCENE).unwrap();
    for (args, signals, groups) in [
        (vec![], false, false),
        (vec!["--connections"], true, false),
        (vec!["--groups"], false, true),
        (vec!["--connections", "--groups"], true, true),
        (vec!["--expand", "--connections", "--groups"], true, true),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_gdkit"))
            .arg("scene-tree")
            .arg(&path)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        assert_eq!(text.contains("signals:"), signals);
        assert_eq!(text.contains("groups:"), groups);
        if signals {
            assert!(text.contains("died -> PlayerInstance._on_health_died"));
        }
    }
    assert_eq!(fs::read_to_string(&path).unwrap(), SCENE);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn expansion_merges_groups_and_rebases_connection_targets() {
    let directory = std::env::temp_dir().join(format!(
        "gdkit-expanded-relationships-{}",
        std::process::id()
    ));
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("base.tscn"), SCENE).unwrap();
    let path = directory.join("world.tscn");
    fs::write(
        &path,
        r#"[gd_scene format=3]
[ext_resource type="PackedScene" path="base.tscn" id="1"]
[node name="World" type="Node"]
[node name="Hero" parent="." instance=ExtResource("1") groups=["player"]]
[node name="Health" parent="Hero" groups=["local", "damageable"]]
[connection signal="died" from="Hero/Health" to="." method="game_over"]
[connection signal="pressed" from="Hero/Hud/Button" to="." method="start"]
"#,
    )
    .unwrap();
    let output = gdkit::scene::compact_tree_expanded_with_options(
        &path,
        1,
        TreeOptions {
            connections: true,
            groups: true,
        },
    )
    .unwrap();
    assert!(output.contains("groups: replicated, player"), "{output}");
    assert!(
        output.contains("groups: damageable, replicated, local"),
        "{output}"
    );
    assert!(output.contains("died -> Hero._on_health_died"), "{output}");
    assert!(output.contains("died -> World.game_over"), "{output}");
    assert!(output.contains("damaged -> %Hud._on_damaged"), "{output}");
    assert!(output.contains("ready -> Hero/Hud/Button.show"), "{output}");
    assert!(output.contains("pressed -> World.start"), "{output}");
    let scene = parse(&fs::read_to_string(&path).unwrap()).unwrap();
    assert!(!scene.compact_tree().unwrap().contains("Button"));
    let literal = scene
        .compact_tree_with_options(TreeOptions {
            connections: true,
            groups: false,
        })
        .unwrap();
    assert!(literal.contains("Button [inherited parent]"), "{literal}");
    assert!(literal.contains("pressed -> World.start"), "{literal}");
    fs::remove_dir_all(directory).unwrap();
}
