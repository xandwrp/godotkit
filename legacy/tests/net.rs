use std::{fs, process::Command};

#[test]
#[ignore = "requires GDKIT_TEST_GODOT pointing to a Godot 4 editor"]
fn reports_engine_rpc_source_calls_and_replication_nodes() {
    let engine = std::env::var_os("GDKIT_TEST_GODOT").expect("set GDKIT_TEST_GODOT");
    let directory = std::env::temp_dir().join(format!("gdkit-net-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    fs::write(
        directory.join("project.godot"),
        "config_version=5\n[application]\nconfig/name=\"gdkit net test\"\n[autoload]\nNetwork=\"*res://network.gd\"\n",
    )
    .unwrap();
    fs::write(
        directory.join("network.gd"),
        "extends Node\n\n@rpc(\"any_peer\", \"call_remote\", \"reliable\")\nfunc receive(value: int) -> void:\n\tprint(value)\n\nfunc send() -> void:\n\treceive.rpc_id(1, 4)\n",
    )
    .unwrap();
    fs::write(
        directory.join("replicated.tscn"),
        "[gd_scene load_steps=2 format=3]\n\n[sub_resource type=\"SceneReplicationConfig\" id=\"Config\"]\nproperties/0/path = NodePath(\".:position\")\nproperties/0/replication_mode = 2\n\n[node name=\"Root\" type=\"Node\"]\n\n[node name=\"Spawner\" type=\"MultiplayerSpawner\" parent=\".\"]\nspawn_path = NodePath(\"..\")\n_spawnable_scenes = PackedStringArray(\"res://replicated.tscn\")\n\n[node name=\"Sync\" type=\"MultiplayerSynchronizer\" parent=\".\"]\nroot_path = NodePath(\"..\")\nreplication_config = SubResource(\"Config\")\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .current_dir(&directory)
        .env_remove("GDKIT_GODOT")
        .args(["net", ".", "--output", "json", "--godot"])
        .arg(&engine)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema_version"], 2);
    assert_eq!(report["rpc_endpoints"][0]["method"], "receive");
    assert_eq!(report["rpc_endpoints"][0]["rpc_mode"], "any_peer");
    assert_eq!(report["rpc_endpoints"][0]["source"]["line"], 3);
    assert_eq!(report["rpc_calls"][0]["target"], "1");
    assert_eq!(report["replication_nodes"][0]["kind"], "MultiplayerSpawner");
    assert_eq!(report["replication_nodes"][0]["spawn_path"], "..");
    assert_eq!(
        report["replication_nodes"][1]["replication_properties"][0]["mode"],
        "on_change"
    );
    assert_eq!(report["autoloads"][0]["networked"], true);
    assert_eq!(
        report["rpc_contracts"][0]["compatible_endpoints"][0]["receiver_path"],
        "/root/Network"
    );
    assert_eq!(
        report["rpc_contracts"][0]["compatible_endpoints"][0]["recipient"],
        "server peer 1"
    );
    let explanation = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .current_dir(&directory)
        .env_remove("GDKIT_GODOT")
        .args([
            "net",
            "explain",
            "Network.receive",
            "--project",
            ".",
            "--godot",
        ])
        .arg(&engine)
        .output()
        .unwrap();
    assert!(explanation.status.success());
    let explanation = String::from_utf8(explanation.stdout).unwrap();
    assert!(explanation.contains("Client call: Network.receive.rpc_id(1, 4)"));
    assert!(explanation.contains("Receiver: /root/Network"));
    assert!(explanation.contains("Permission: any_peer"));
    assert!(explanation.contains("Delivery: reliable, remote-only, channel 0"));
    assert!(explanation.contains("Stable path: autoload on every participant"));
    fs::remove_dir_all(directory).unwrap();
}
