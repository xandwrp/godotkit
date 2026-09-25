// Drives the built binary. Offline; engine-backed paths use gdproject's fake-godot
// (path via env FAKE_GODOT, set by the test harness from CARGO_BIN_EXE in gdproject's build).
#![allow(unused)]

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

/// Runs the binary with an engine variable that points nowhere, so any
/// accidental engine use fails loudly.
fn gdkit(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .args(args)
        .env("GDKIT_GODOT", "/nonexistent/godot")
        .output()
        .unwrap()
}

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("project.godot"), "config_version=5\n").unwrap();
    for (path, contents) in files {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    dir
}

const MISSING_PRELOAD: (&str, &str) = ("scripts/a.gd", "extends Node\nconst X = preload(\"res://gone.tscn\")\n");

#[test]
#[ignore = "scaffold"]
fn no_arguments_prints_help_and_exits_zero() { todo!() }

#[test]
#[ignore = "scaffold"]
fn every_project_command_accepts_project_godot_and_output_flags_uniformly() { todo!() }

#[test]
fn json_mode_writes_exactly_one_json_document_to_stdout_and_nothing_else() {
    let dir = project(&[MISSING_PRELOAD]);
    let root = dir.path().to_str().unwrap();
    let output = gdkit(&["check", "--static-only", "--project", root, "--output", "json"]);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let mut documents = serde_json::Deserializer::from_str(&stdout).into_iter::<serde_json::Value>();
    let report = documents.next().unwrap().unwrap();
    assert!(documents.next().is_none(), "more than one JSON document on stdout");
    assert_eq!(report["outcome"], "failed");
    assert_eq!(report["phases"][0]["diagnostics"][0]["resource"], "res://scripts/a.gd");
    assert!(output.stderr.is_empty(), "JSON mode printed progress: {}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn tool_errors_exit_2_with_error_prefix_on_stderr() {
    let outside = tempfile::tempdir().unwrap();
    let dir = project(&[]);
    let root = dir.path().to_str().unwrap();
    for args in [
        vec!["check", "--static-only", "--project", outside.path().to_str().unwrap()],
        vec!["check", "--static-only", "--project", root, "--slice", "../escape"],
        vec!["check", "--static-only", "--project", root, "--baseline", "/nonexistent/report.json"],
        vec!["check", "--project", root],
        vec!["check", "--static-only", "--project", root, "--output", "json", "--slice", "../escape"],
    ] {
        let output = gdkit(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.starts_with("error: "), "{args:?}: {stderr}");
        assert!(output.stdout.is_empty(), "{args:?}: tool errors must not print a report");
    }
}

#[test]
fn check_exit_code_follows_report_outcome() {
    let dir = project(&[MISSING_PRELOAD, ("scripts/ok.gd", "extends Node\n")]);
    let root = dir.path().to_str().unwrap();
    let failed = gdkit(&["check", "--static-only", "--project", root]);
    assert_eq!(failed.status.code(), Some(1));
    let stdout = String::from_utf8(failed.stdout).unwrap();
    assert!(stdout.contains("res://scripts/a.gd:2: error: preload of res://gone.tscn, which does not exist"), "{stdout}");
    assert!(stdout.contains("check FAILED"), "{stdout}");
    // Discovery walks up from any path inside the project.
    let passed = gdkit(&["check", "--static-only", "--project", &format!("{root}/scripts"), "--slice", "scripts/ok.gd"]);
    assert_eq!(passed.status.code(), Some(0), "{}", String::from_utf8_lossy(&passed.stdout));
    assert!(String::from_utf8(passed.stdout).unwrap().contains("check passed"));
}

#[test]
fn static_check_baseline_file_isolates_new_findings() {
    let dir = project(&[MISSING_PRELOAD]);
    let root = dir.path().to_str().unwrap();
    let before = gdkit(&["check", "--static-only", "--project", root, "--output", "json"]);
    let baseline = dir.path().join("baseline.json");
    fs::write(&baseline, &before.stdout).unwrap();
    fs::write(dir.path().join("scripts/b.gd"), "extends Node\nconst Y = load(\"res://also_gone.tres\")\n").unwrap();
    let after = gdkit(&["check", "--static-only", "--project", root, "--output", "json", "--baseline", baseline.to_str().unwrap()]);
    assert_eq!(after.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&after.stdout).unwrap();
    let new = report["baseline"]["new"].as_array().unwrap();
    assert_eq!(new.len(), 1);
    assert_eq!(new[0]["resource"], "res://scripts/b.gd");
    assert_eq!(report["baseline"]["carried"].as_array().unwrap().len(), 1);
}

#[test]
#[ignore = "scaffold"]
fn scene_tree_autoloads_refs_settings_net_and_static_check_need_no_engine() { todo!() }

#[test]
#[ignore = "scaffold"]
fn init_writes_config_and_refuses_to_overwrite() { todo!() }
