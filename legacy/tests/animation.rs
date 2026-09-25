use std::{fs, path::Path, process::Command};

fn glb(document: &str) -> Vec<u8> {
    let mut json = document.as_bytes().to_vec();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let length = 20 + json.len();
    let mut bytes = Vec::with_capacity(length);
    bytes.extend_from_slice(b"glTF");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&u32::try_from(length).unwrap().to_le_bytes());
    bytes.extend_from_slice(&u32::try_from(json.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(&0x4e4f_534au32.to_le_bytes());
    bytes.extend_from_slice(&json);
    bytes
}

fn run(path: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .args(["animation", "list"])
        .arg(path)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn lists_and_filters_packed_animations() {
    let path = std::env::temp_dir().join(format!("gdkit-animations-{}.glb", std::process::id()));
    fs::write(
        &path,
        glb(
            r#"{"asset":{"version":"2.0"},"accessors":[{"min":[0],"max":[1.25]}],"animations":[{"name":"Standing","samplers":[{"input":0}],"channels":[{"target":{"node":1,"path":"rotation"}}]},{"name":"Running","samplers":[{"input":0}],"channels":[{"target":{"node":1,"path":"rotation"}},{"target":{"node":2,"path":"translation"}}]}]}"#,
        ),
    )
    .unwrap();

    let human = run(&path, &[]);
    assert!(human.status.success());
    let human = String::from_utf8(human.stdout).unwrap();
    assert!(human.contains("2 animations"));
    assert!(human.contains("Standing"));
    assert!(human.contains("Running"));
    assert!(human.contains("1.250s"));

    let names = run(&path, &["--names", "--filter", "run"]);
    assert!(names.status.success());
    assert_eq!(String::from_utf8(names.stdout).unwrap(), "Running\n");

    let json = run(&path, &["--output", "json", "--filter", "stand"]);
    assert!(json.status.success());
    let report: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["animations"].as_array().unwrap().len(), 1);
    assert_eq!(report["animations"][0]["name"], "Standing");
    assert_eq!(report["animations"][0]["duration_seconds"], 1.25);
    assert_eq!(report["animations"][0]["target_nodes"], 1);

    fs::remove_file(path).unwrap();
}

#[test]
fn reports_malformed_glb_files_as_tooling_errors() {
    let path = std::env::temp_dir().join(format!("gdkit-broken-{}.glb", std::process::id()));
    fs::write(&path, b"not glb").unwrap();
    let output = run(&path, &[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("file is too short to be a GLB")
    );
    fs::remove_file(path).unwrap();
}

#[test]
fn inspects_effective_animation_tree_graphs_with_godot() {
    let Some(engine) = std::env::var_os("GDKIT_TEST_GODOT") else {
        eprintln!("ignored, requires GDKIT_TEST_GODOT pointing to a Godot 4 editor");
        return;
    };
    let directory =
        std::env::temp_dir().join(format!("gdkit-animation-tree-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("project.godot"), "config_version=5\n").unwrap();
    fs::write(
        directory.join("tree.tscn"),
        r#"[gd_scene load_steps=5 format=3]

[sub_resource type="Animation" id="Animation_run"]
resource_name = "Run"
length = 1.25

[sub_resource type="AnimationLibrary" id="AnimationLibrary_main"]
_data = {&"Run": SubResource("Animation_run")}

[sub_resource type="AnimationNodeAnimation" id="AnimationNodeAnimation_run"]
animation = &"Run"

[sub_resource type="AnimationNodeStateMachineTransition" id="Transition_start"]
advance_mode = 2

[sub_resource type="AnimationNodeStateMachine" id="StateMachine_root"]
states/Run/node = SubResource("AnimationNodeAnimation_run")
states/Run/position = Vector2(200, 100)
transitions = ["Start", "Run", SubResource("Transition_start")]

[node name="Root" type="Node"]

[node name="AnimationPlayer" type="AnimationPlayer" parent="."]
libraries = {&"": SubResource("AnimationLibrary_main")}

[node name="AnimationTree" type="AnimationTree" parent="."]
tree_root = SubResource("StateMachine_root")
anim_player = NodePath("../AnimationPlayer")
"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .args(["animation", "inspect", "res://tree.tscn", "--project"])
        .arg(&directory)
        .args(["--godot"])
        .arg(engine)
        .args(["--tree", "AnimationTree", "--output", "json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let tree = &report["trees"][0];
    assert_eq!(tree["path"], "AnimationTree");
    assert_eq!(tree["source_line"], 26);
    assert_eq!(tree["animation_player"]["resolved"], true);
    assert_eq!(tree["animations"][0]["name"], "Run");
    assert_eq!(tree["graph"]["source_line"], 16);
    assert_eq!(tree["graph"]["children"][1]["animation_found"], true);

    fs::remove_dir_all(directory).unwrap();
}
