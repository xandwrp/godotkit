use std::{
    fs,
    process::{Command, Output},
};

fn run(directory: &std::path::Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .current_dir(directory)
        .args(arguments)
        .output()
        .unwrap()
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT pointing to a Godot 4 editor"]
fn orchestrates_late_join_with_isolation_and_controlled_exits() {
    let engine = std::env::var_os("GDKIT_TEST_GODOT").expect("set GDKIT_TEST_GODOT");
    let directory = std::env::temp_dir().join(format!("gdkit-scenario-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("project.godot"),
        "config_version=5\n[application]\nconfig/name=\"gdkit scenario test\"\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .unwrap();
    fs::write(
        directory.join("main.tscn"),
        "[gd_scene load_steps=2 format=3]\n\n[ext_resource type=\"Script\" path=\"res://main.gd\" id=\"1\"]\n\n[node name=\"Main\" type=\"Node\"]\nscript = ExtResource(\"1\")\n",
    )
    .unwrap();
    fs::write(
        directory.join("main.gd"),
        r#"extends Node

var peer := ENetMultiplayerPeer.new()

func _ready() -> void:
	var args := OS.get_cmdline_user_args()
	var port := int(args[1].trim_prefix("--port="))
	var error := peer.create_server(port, 8) if args[0] == "--server" else peer.create_client("127.0.0.1", port)
	if error != OK:
		get_tree().quit(1)
		return
	multiplayer.multiplayer_peer = peer
	print("participant=", OS.get_environment("GDKIT_SCENARIO_PARTICIPANT"), " transport=", OS.get_environment("GDKIT_SCENARIO_TRANSPORT"), " user=", OS.get_user_data_dir())
"#,
    )
    .unwrap();
    fs::write(
        directory.join("checkpoints.gd"),
        r#"extends RefCounted

func collect_checkpoints(tree: SceneTree) -> Dictionary:
	var peer := tree.get_multiplayer().get_multiplayer_peer()
	return {
		"network": {
			"ready": peer != null and peer.get_connection_status() == MultiplayerPeer.CONNECTION_CONNECTED,
			"role": OS.get_environment("GDKIT_SCENARIO_ROLE"),
			"port": peer.get_host().get_local_port() if OS.get_environment("GDKIT_SCENARIO_ROLE") == "server" else 0,
		}
	}
"#,
    )
    .unwrap();
    fs::write(
        directory.join("gdkit.toml"),
        r#"[engine]
executable = 'unused'

[inspect]
checkpoint_adapter = 'res://checkpoints.gd'

[scenarios.late_join]
transport = 'dedicated_enet'
timeout_seconds = 10
ports = { game = { checkpoint = '/network/port' } }

[[scenarios.late_join.participants]]
name = 'server'
role = 'server'
arguments = ['--server', '--port={port.game}']
readiness = { path = '/network/ready', equals = true }

[[scenarios.late_join.participants]]
name = 'client-1'
role = 'client'
arguments = ['--client', '--port={port.game}']
readiness = { path = '/network/ready', equals = true }

[[scenarios.late_join.participants]]
name = 'client-2'
role = 'client'
arguments = ['--client', '--port={port.game}']
readiness = { path = '/network/ready', equals = true }

[[scenarios.late_join.participants]]
name = 'late-client'
role = 'late_client'
arguments = ['--client', '--port={port.game}']
readiness = { path = '/network/ready', equals = true }

[scenarios.failed_start]
transport = 'dedicated_enet'
timeout_seconds = 1
ports = { game = { checkpoint = '/network/port' } }

[[scenarios.failed_start.participants]]
name = 'server'
role = 'server'
arguments = ['--server', '--port={port.game}']
readiness = { path = '/network/missing', equals = true }

[[scenarios.failed_start.participants]]
name = 'late-client'
role = 'late_client'
arguments = ['--client', '--port={port.game}']
readiness = { path = '/network/ready', equals = true }
"#,
    )
    .unwrap();

    let engine = engine.to_string_lossy();
    let started = run(
        &directory,
        &["scenario", "start", "late_join", "--godot", &engine],
    );
    assert!(
        started.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&started.stdout),
        String::from_utf8_lossy(&started.stderr)
    );
    let text = String::from_utf8(started.stdout).unwrap();
    assert!(text.contains("ready server:"));
    assert!(text.contains("ready client-1:"));
    assert!(text.contains("ready client-2:"));
    assert!(text.contains("ready late-client:"));

    let status = run(&directory, &["scenario", "status", "late_join"]);
    assert!(status.status.success());
    let status = String::from_utf8(status.stdout).unwrap();
    assert!(status.contains("transport=dedicated_enet"));
    assert_eq!(status.matches(" running ").count(), 4);
    assert_eq!(status.matches("user-data").count(), 4);
    let logs = fs::read_dir(directory.join(".godot/gdkit/sessions/logs"))
        .unwrap()
        .map(|entry| fs::read_to_string(entry.unwrap().path()).unwrap())
        .collect::<Vec<_>>();
    for participant in ["server", "client-1", "client-2", "late-client"] {
        let log = logs
            .iter()
            .find(|log| log.contains(&format!("participant={participant} ")))
            .unwrap();
        assert!(log.contains("transport=dedicated_enet"));
        assert!(log.contains(&format!("user-data/{participant}")));
    }

    let disconnected = run(
        &directory,
        &["scenario", "disconnect", "late_join", "client-1"],
    );
    assert!(
        disconnected.status.success(),
        "{}",
        String::from_utf8_lossy(&disconnected.stderr)
    );
    let crashed = run(
        &directory,
        &["scenario", "crash", "late_join", "late-client"],
    );
    assert!(crashed.status.success());
    let stopped = run(&directory, &["scenario", "stop", "late_join"]);
    assert!(
        stopped.status.success(),
        "{}",
        String::from_utf8_lossy(&stopped.stderr)
    );
    let final_status = run(&directory, &["scenario", "status", "late_join"]);
    assert_eq!(
        String::from_utf8(final_status.stdout)
            .unwrap()
            .matches(" exited ")
            .count(),
        4
    );

    let failed = run(
        &directory,
        &["scenario", "start", "failed_start", "--godot", &engine],
    );
    assert!(!failed.status.success());
    assert!(
        String::from_utf8_lossy(&failed.stderr).contains("did not reach readiness"),
        "{}",
        String::from_utf8_lossy(&failed.stderr)
    );
    let failed_status = run(&directory, &["scenario", "status", "failed_start"]);
    assert!(failed_status.status.success());
    let failed_status = String::from_utf8(failed_status.stdout).unwrap();
    assert!(failed_status.contains("status=failed"));
    assert!(failed_status.contains("first failure: participant=server phase=readiness"));
    assert!(failed_status.contains("server (server) exited readiness=failed"));
    assert!(failed_status.contains("port game: 0"));
    fs::remove_dir_all(directory).unwrap();
}
