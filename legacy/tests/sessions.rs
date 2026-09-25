use std::{
    fs,
    process::{Command, Output},
    thread,
    time::{Duration, Instant},
};

fn run(directory: &std::path::Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .current_dir(directory)
        .env_remove("GDKIT_GODOT")
        .args(arguments)
        .output()
        .unwrap()
}

fn selector(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("started "))
        .and_then(|line| line.split_once(" pid "))
        .map(|(selector, _)| selector.to_owned())
        .expect("started session selector")
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT pointing to a Godot 4 editor"]
fn launches_lists_logs_stops_and_restarts_named_sessions() {
    let engine = std::env::var_os("GDKIT_TEST_GODOT").expect("set GDKIT_TEST_GODOT");
    let directory = std::env::temp_dir().join(format!("gdkit-sessions-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    fs::write(
        directory.join("project.godot"),
        "config_version=5\n[application]\nconfig/name=\"gdkit sessions test\"\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .unwrap();
    fs::write(
        directory.join("gdkit.toml"),
        "[engine]\nexecutable='unused'\n[inspect]\ncheckpoint_adapter='res://checkpoints.gd'\n",
    )
    .unwrap();
    fs::write(
        directory.join("main.tscn"),
        "[gd_scene load_steps=2 format=3]\n\n[ext_resource type=\"Script\" path=\"res://main.gd\" id=\"1\"]\n\n[node name=\"Main\" type=\"Node\"]\nscript = ExtResource(\"1\")\n",
    )
    .unwrap();
    fs::write(
        directory.join("main.gd"),
        "extends Node\n\nfunc _ready() -> void:\n\tprint(\"named session ready\")\n\tawait get_tree().process_frame\n\tawait get_tree().process_frame\n\tmultiplayer.peer_connected.emit(42)\n\tEngineDebugger.profiler_add_frame_data(&\"multiplayer:rpc\", [\"rpc_out\", get_instance_id(), 12])\n",
    )
    .unwrap();
    fs::write(
        directory.join("checkpoints.gd"),
        "extends RefCounted\n\nfunc collect_checkpoints(tree: SceneTree) -> Dictionary:\n\tvar revision := 8 if OS.get_cmdline_user_args().has(\"--variant\") else 7\n\treturn {\n\t\t\"network_session\": {\"peer_id\": tree.get_multiplayer().get_unique_id()},\n\t\t\"lobby_state\": {\"revision\": revision, \"participants\": [1, 42]},\n\t\t\"round_state\": {\"phase\": \"active\"},\n\t\t\"local_player_state\": {\"alive\": true},\n\t}\n",
    )
    .unwrap();
    let engine = engine.to_string_lossy();
    let launched = run(
        &directory,
        &["run", "--name", "server", "--headless", "--godot", &engine],
    );
    assert!(
        launched.status.success(),
        "{}",
        String::from_utf8_lossy(&launched.stderr)
    );
    let first = selector(&launched);

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let log = run(&directory, &["logs", &first]);
        if log.status.success()
            && String::from_utf8_lossy(&log.stdout).contains("named session ready")
        {
            break;
        }
        assert!(Instant::now() < deadline, "session did not become ready");
        thread::sleep(Duration::from_millis(50));
    }

    let deadline = Instant::now() + Duration::from_secs(10);
    let inspected = loop {
        let inspected = run(&directory, &["inspect", &first, "--net"]);
        assert!(
            inspected.status.success(),
            "{}",
            String::from_utf8_lossy(&inspected.stderr)
        );
        let text = String::from_utf8(inspected.stdout).unwrap();
        if text.contains("peer_connected") && text.contains(" rpc ") {
            break text;
        }
        assert!(Instant::now() < deadline, "probe events were not captured");
        thread::sleep(Duration::from_millis(25));
    };
    assert!(inspected.contains(&format!("session: {first}")));
    assert!(inspected.contains("OfflineMultiplayerPeer"));
    assert!(inspected.contains("node authorities:"));
    assert!(inspected.contains("recent events:"));

    let inspected_json = run(
        &directory,
        &["inspect", "server", "--net", "--output", "json"],
    );
    assert!(inspected_json.status.success());
    let inspected_json: serde_json::Value = serde_json::from_slice(&inspected_json.stdout).unwrap();
    assert_eq!(inspected_json["session"], "server");
    assert_eq!(
        inspected_json["generation"],
        first.split_once('@').unwrap().1
    );
    assert!(
        inspected_json["observation"]["process_tick"]
            .as_u64()
            .unwrap()
            > 0
    );

    let checkpoints = run(
        &directory,
        &["inspect", "server", "--checkpoints", "--output", "json"],
    );
    assert!(
        checkpoints.status.success(),
        "{}",
        String::from_utf8_lossy(&checkpoints.stderr)
    );
    let checkpoints: serde_json::Value = serde_json::from_slice(&checkpoints.stdout).unwrap();
    assert_eq!(checkpoints["checkpoints"]["status"], "collected");
    assert_eq!(
        checkpoints["checkpoints"]["adapter"],
        "res://checkpoints.gd"
    );
    assert_eq!(
        checkpoints["checkpoints"]["values"]["lobby_state"]["revision"],
        7
    );
    assert_eq!(
        checkpoints["checkpoints"]["values"]["round_state"]["phase"],
        "active"
    );
    assert_eq!(checkpoints["checkpoints"]["limits"]["checkpoints"], 32);

    let combined = run(
        &directory,
        &[
            "inspect",
            "server",
            "--net",
            "--checkpoints",
            "--output",
            "json",
        ],
    );
    assert!(combined.status.success());
    let combined: serde_json::Value = serde_json::from_slice(&combined.stdout).unwrap();
    assert!(combined["observation"].is_object());
    assert!(combined["checkpoints"].is_object());

    fs::write(
        directory.join("gdkit.toml"),
        "[engine]\nexecutable='unused'\n[inspect]\ncheckpoint_adapter='res://missing.gd'\n",
    )
    .unwrap();
    let invalid_checkpoints = run(
        &directory,
        &["inspect", "server", "--checkpoints", "--output", "json"],
    );
    assert_eq!(invalid_checkpoints.status.code(), Some(1));
    let invalid_checkpoints: serde_json::Value =
        serde_json::from_slice(&invalid_checkpoints.stdout).unwrap();
    assert_eq!(invalid_checkpoints["checkpoints"]["status"], "error");
    assert_eq!(
        invalid_checkpoints["checkpoints"]["error"],
        "checkpoint adapter does not exist"
    );
    fs::write(
        directory.join("gdkit.toml"),
        "[engine]\nexecutable='unused'\n[inspect]\ncheckpoint_adapter='res://checkpoints.gd'\n",
    )
    .unwrap();

    let listed = run(&directory, &["sessions"]);
    assert!(listed.status.success());
    let listed = String::from_utf8(listed.stdout).unwrap();
    assert!(listed.contains("server\t"));
    assert!(listed.contains("\trunning\theadless\t<main>"));

    let client = run(
        &directory,
        &[
            "run",
            "--name",
            "client",
            "--headless",
            "--scene",
            "res://main.tscn",
            "--godot",
            &engine,
            "--",
            "--variant",
        ],
    );
    assert!(
        client.status.success(),
        "{}",
        String::from_utf8_lossy(&client.stderr)
    );
    let client_selector = selector(&client);
    let comparison = run(
        &directory,
        &[
            "inspect",
            &first,
            "--checkpoints",
            "--compare",
            &client_selector,
            "--output",
            "json",
        ],
    );
    assert_eq!(comparison.status.code(), Some(1));
    let comparison: serde_json::Value = serde_json::from_slice(&comparison.stdout).unwrap();
    assert_eq!(comparison["equal"], false);
    assert_eq!(comparison["left"]["session"], "server");
    assert_eq!(comparison["right"]["session"], "client");
    assert_eq!(comparison["differences"].as_array().unwrap().len(), 1);
    assert_eq!(
        comparison["differences"][0]["path"],
        "/lobby_state/revision"
    );
    assert_eq!(comparison["differences"][0]["left"], 7);
    assert_eq!(comparison["differences"][0]["right"], 8);
    let concurrent = run(&directory, &["sessions"]);
    assert_eq!(
        String::from_utf8_lossy(&concurrent.stdout)
            .lines()
            .filter(|line| line.contains("\trunning\theadless\t"))
            .count(),
        2
    );
    assert!(
        String::from_utf8_lossy(&concurrent.stdout)
            .contains("\trunning\theadless\tres://main.tscn")
    );
    assert!(
        run(&directory, &["stop", &client_selector])
            .status
            .success()
    );

    let duplicate = run(
        &directory,
        &["run", "--name", "server", "--headless", "--godot", &engine],
    );
    assert_eq!(duplicate.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&duplicate.stderr).contains("already running"));

    let stopped = run(&directory, &["stop", &first]);
    assert!(
        stopped.status.success(),
        "{}",
        String::from_utf8_lossy(&stopped.stderr)
    );
    let stale = run(&directory, &["stop", &first]);
    assert_eq!(stale.status.code(), Some(1));
    assert_eq!(
        run(&directory, &["inspect", &first, "--net"]).status.code(),
        Some(1)
    );

    let restarted = run(&directory, &["restart", &first]);
    assert!(
        restarted.status.success(),
        "{}",
        String::from_utf8_lossy(&restarted.stderr)
    );
    let second = selector(&restarted);
    assert_ne!(first, second);
    assert!(run(&directory, &["stop", &second]).status.success());

    let history = run(&directory, &["sessions", "--all"]);
    assert!(history.status.success());
    assert_eq!(
        String::from_utf8_lossy(&history.stdout)
            .lines()
            .filter(|line| line.starts_with("server\t"))
            .count(),
        2
    );
    fs::remove_dir_all(directory).unwrap();
}
