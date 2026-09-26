//! Differential validation only. Production `net` never invokes the engine.
use gdproject::{
    config::SelectionSource,
    engine::Engine,
    runner::{self, Invocation},
};
use gdview::net::{self, SyncMode};
use std::{fs, time::Duration};

const ACTOR: &str = include_str!("../../gdview/tests/fixtures/net/actor.gd");
const SCENE: &str = include_str!("../../gdview/tests/fixtures/net/replication.tscn");
const LEGACY: &str = include_str!("../../gdview/tests/fixtures/net/legacy.tscn");

#[test]
#[ignore = "requires GDKIT_TEST_GODOT"]
fn real_engine_net_rpc_and_replication_observations_match_shared_fixtures() {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in [
        ("project.godot", "config_version=5\n"),
        ("actor.gd", ACTOR),
        ("replication.tscn", SCENE),
        ("legacy.tscn", LEGACY),
    ] {
        fs::write(dir.path().join(path), text).unwrap();
    }
    // Analyze before the probe exists, preserving exactly the offline fixture set.
    let project = gdview::Project::open(dir.path()).unwrap();
    let report = net::analyze_project(&project).unwrap();
    assert!(report.unknowns.is_empty(), "{:#?}", report.unknowns);
    fs::write(dir.path().join("probe.gd"), r#"extends SceneTree
func _initialize():
    var script = load("res://actor.gd")
    var result = {"rpc": script.get_rpc_config(), "scenes": {}}
    for path in ["res://replication.tscn", "res://legacy.tscn"]:
        var scene = load(path).instantiate()
        var sync = scene.get_node("Sync")
        var props = []
        for property in sync.replication_config.get_properties():
            props.append({"path": str(property), "spawn": sync.replication_config.property_get_spawn(property), "mode": sync.replication_config.property_get_replication_mode(property)})
        var observation = {"root_path": str(sync.root_path), "properties": props}
        if scene.has_node("Spawner"):
            observation["spawn_path"] = str(scene.get_node("Spawner").spawn_path)
        result.scenes[path] = observation
        scene.free()
    print("NET_FIXTURE:" + JSON.stringify(result))
    quit()
"#).unwrap();
    let engine = Engine {
        executable: std::env::var_os("GDKIT_TEST_GODOT")
            .expect("set GDKIT_TEST_GODOT")
            .into(),
        version: "real".into(),
        fingerprint: "net-fixture-test".into(),
        source: SelectionSource::Environment,
    };
    let mut invocation = Invocation::new(&engine, dir.path(), Duration::from_secs(30));
    invocation.engine_args = vec!["--script".into(), "res://probe.gd".into()];
    let (captured, diagnostics) = runner::run_engine(&invocation).unwrap();
    assert!(
        captured.success() && !captured.timed_out && !captured.output_limit_exceeded,
        "{captured:?}"
    );
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    let stdout = String::from_utf8(captured.stdout()).unwrap();
    let lines: Vec<_> = stdout
        .lines()
        .filter_map(|line| line.strip_prefix("NET_FIXTURE:"))
        .collect();
    assert_eq!(lines.len(), 1, "{stdout}");
    let engine: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    for endpoint in &report.endpoints {
        let config = &engine["rpc"][&endpoint.method];
        assert!(config.is_object());
        let mode = match endpoint.config.mode {
            gdview::declarations::RpcMode::AnyPeer => 1,
            gdview::declarations::RpcMode::Authority => 2,
        };
        let transfer = match endpoint.config.transfer {
            gdview::declarations::TransferMode::Unreliable => 0,
            gdview::declarations::TransferMode::UnreliableOrdered => 1,
            gdview::declarations::TransferMode::Reliable => 2,
        };
        assert_eq!(config["rpc_mode"].as_i64().unwrap_or(2), mode);
        assert_eq!(config["transfer_mode"].as_i64().unwrap_or(0), transfer);
        assert_eq!(
            config["channel"].as_i64().unwrap_or(0),
            endpoint.config.channel
        );
        assert_eq!(
            config["call_local"].as_bool().unwrap_or(false),
            endpoint.config.call_local
        );
    }
    for sync in &report.synchronizers {
        let observed = &engine["scenes"][sync.scene.as_str()];
        assert_eq!(observed["root_path"], sync.root_path.as_ref().unwrap().0);
        let properties = observed["properties"].as_array().unwrap();
        assert_eq!(properties.len(), sync.properties.len());
        for (actual, expected) in properties.iter().zip(&sync.properties) {
            let mode = match expected.mode.unwrap() {
                SyncMode::Never => 0,
                SyncMode::Always => 1,
                SyncMode::OnChange => 2,
            };
            assert_eq!(actual["path"], expected.path.0);
            assert_eq!(actual["spawn"], expected.spawn.unwrap());
            assert_eq!(actual["mode"], mode);
        }
    }
    assert_eq!(
        engine["scenes"]["res://replication.tscn"]["spawn_path"],
        report.spawners[0].spawn_path.as_ref().unwrap().0
    );
}
