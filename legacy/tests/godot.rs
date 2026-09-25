use std::{fs, process::Command};

use gdkit::formatter::{Options, format_source};

#[test]
#[ignore = "requires a local Godot 4.7.2 executable"]
fn engine_accepts_guards_and_preserves_execution() {
    let version = Command::new("godot").arg("--version").output().unwrap();
    assert!(String::from_utf8_lossy(&version.stdout).starts_with("4.7.2."));
    let directory = std::env::temp_dir().join(format!("gdkit-engine-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("project.godot"), "config_version=5\n").unwrap();
    let source = "extends SceneTree\n\nfunc guard(value: bool):\n\tif value:\n\t\treturn\n\tprint(\"passed\")\n\nfunc nested(a: bool, b: bool):\n\tif a:\n\t\tif b:\n\t\t\treturn\n\tprint(\"nested passed\")\n\nfunc _initialize():\n\tguard(true)\n\tguard(false)\n\tnested(true, true)\n\tnested(true, false)\n\tnested(false, true)\n\tquit()\n";
    let formatted = format_source(source, &Options::default()).unwrap();
    assert_ne!(formatted, source);
    let mut outputs = Vec::new();
    for text in [source, &formatted] {
        fs::write(directory.join("guard.gd"), text).unwrap();
        for check_only in [true, false] {
            let mut command = Command::new("godot");
            command
                .args(["--headless", "--path"])
                .arg(&directory)
                .args(["--script", "guard.gd"]);
            if check_only {
                command.arg("--check-only");
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(!String::from_utf8_lossy(&output.stderr).contains("SCRIPT ERROR"));
            if !check_only {
                outputs.push(output.stdout);
            }
        }
    }
    assert_eq!(outputs[0], outputs[1]);
    assert!(String::from_utf8_lossy(&outputs[0]).contains("passed\nnested passed\nnested passed"));
    fs::remove_dir_all(directory).unwrap();
}
