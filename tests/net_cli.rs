//! Net's end-to-end offline contract. No Godot installation is required.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn command(root: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_gdkit"));
    c.env_remove("GDKIT_ALLOW_STUBS")
        .env("GDKIT_GODOT", root.join("engine-must-not-run"))
        .env(
            "GDKIT_CONFIG_DIR",
            root.join("global-config-must-not-be-created"),
        );
    c.args(["net", "--project"])
        .arg(root)
        .arg("--godot")
        .arg(root.join("also-must-not-run"));
    c
}
fn write(root: &Path, path: &str, text: &str) {
    let p = root.join(path);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in [
        (
            "project.godot",
            "config_version=5\n[autoload]\nNetwork=\"*uid://network\"\n",
        ),
        (
            "gdkit.toml",
            "this is deliberately invalid engine configuration [",
        ),
        ("network.gd.uid", "uid://network\n"),
        (
            "network.gd",
            "extends Node\n@rpc\nfunc fire(): pass\nfunc send():\n    fire.rpc()\n    multiplayer.is_server()\n",
        ),
        (
            "main.tscn",
            "[gd_scene format=3]\n[node name=\"Root\" type=\"Node\"]\n[node name=\"Sync\" type=\"MultiplayerSynchronizer\" parent=\".\"]\n",
        ),
        ("ignored/.gdignore", ""),
        (
            "ignored/ignored.gd",
            "@rpc\nfunc ignored_endpoint(): pass\n",
        ),
        (".godot/do-not-touch", "preexisting cache"),
    ] {
        write(dir.path(), path, text);
    }
    dir
}
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, result: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                result.insert(path.strip_prefix(root).unwrap().to_owned(), vec![]);
                walk(root, &path, result);
            } else {
                result.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    std::fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut result = BTreeMap::new();
    walk(root, root, &mut result);
    result
}
fn json(output: Output, code: i32) -> serde_json::Value {
    assert_eq!(
        output.status.code(),
        Some(code),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("exactly one JSON document")
}

#[test]
fn net_needs_no_engine_or_valid_engine_config_and_never_writes() {
    let dir = fixture();
    let before = snapshot(dir.path());
    let report = json(
        command(dir.path())
            .args(["--output", "json"])
            .output()
            .unwrap(),
        0,
    );
    assert_eq!(report["schema_version"], 3);
    assert_eq!(report["coverage"]["scripts_scanned"], 1);
    assert_eq!(report["endpoints"].as_array().unwrap().len(), 1);
    assert_eq!(report["calls"][0]["candidates"], serde_json::json!([0]));
    assert_eq!(report["autoloads"][0]["path"], "res://network.gd");
    assert_eq!(report["autoloads"][0]["networked"], true);
    assert_eq!(report["synchronizers"][0]["root_path"], "..");
    let human = command(dir.path()).output().unwrap();
    assert!(human.status.success());
    assert!(human.stderr.is_empty());
    let human = String::from_utf8(human.stdout).unwrap();
    for text in [
        "offline",
        "RPC endpoints (1)",
        "res://network.gd:3",
        "candidates=[0]",
        "Synchronizers (1)",
        "Unknowns",
    ] {
        assert!(human.contains(text), "{text}: {human}");
    }
    assert_eq!(before, snapshot(dir.path()));
}

#[test]
fn net_explanation_exit_codes_and_json_references_are_consistent() {
    let dir = fixture();
    for query in ["fire", "Network.fire", "res://network.gd", "Sync"] {
        let result = json(
            command(dir.path())
                .args(["--explain", query, "--output", "json"])
                .output()
                .unwrap(),
            0,
        );
        assert_eq!(result["matched"], true);
        for call in result["calls"].as_array().unwrap() {
            for candidate in call["candidates"].as_array().unwrap() {
                assert!(
                    candidate.as_u64().unwrap()
                        < result["endpoints"].as_array().unwrap().len() as u64
                );
            }
        }
    }
    let miss = json(
        command(dir.path())
            .args(["--explain", "absent", "--output", "json"])
            .output()
            .unwrap(),
        1,
    );
    assert_eq!(miss["matched"], false);
    assert!(
        miss["notes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n.as_str().unwrap().contains("no observations"))
    );
    let empty = command(dir.path())
        .args(["--explain", " "])
        .output()
        .unwrap();
    assert_eq!(empty.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&empty.stderr).contains("non-empty query"));
}

#[test]
fn net_empty_partial_and_invalid_projects_have_distinct_outcomes() {
    let dir = tempfile::tempdir().unwrap();
    let invalid = command(dir.path()).output().unwrap();
    assert_eq!(invalid.status.code(), Some(2));
    assert!(invalid.stdout.is_empty());
    write(dir.path(), "project.godot", "config_version=5\n");
    let empty = json(
        command(dir.path())
            .args(["--output", "json"])
            .output()
            .unwrap(),
        0,
    );
    assert!(empty["endpoints"].as_array().unwrap().is_empty());
    write(dir.path(), "bad.gd", "func broken(:\n pass\n");
    write(dir.path(), "bad.tscn", "broken scene");
    write(dir.path(), "net.cs", "unsupported");
    let partial = json(
        command(dir.path())
            .args(["--output", "json"])
            .output()
            .unwrap(),
        0,
    );
    assert!(partial["unknowns"].as_array().unwrap().len() >= 3);
    assert!(!dir.path().join(".godot").exists());
}

#[test]
fn net_discovers_from_project_file_and_help_is_not_a_stub() {
    let dir = fixture();
    let result = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .env("GDKIT_GODOT", "engine-must-not-run")
        .args(["net", "--output", "json", "--project"])
        .arg(dir.path().join("project.godot"))
        .output()
        .unwrap();
    json(result, 0);
    let help = command(dir.path()).arg("--help").output().unwrap();
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    assert!(help.contains("--explain"));
    assert!(!help.contains("[stub]"));
}
