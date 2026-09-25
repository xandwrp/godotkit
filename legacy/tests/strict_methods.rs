use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};

struct Project(PathBuf);

#[test]
fn smoke_options_require_explicit_scene_execution() {
    for args in [
        vec!["--smoke-frames", "3"],
        vec!["--smoke-timeout", "1"],
        vec!["--scene", "lobby.tscn", "--smoke-frames", "0"],
        vec!["--scene", "lobby.tscn", "--smoke-timeout", "0"],
        vec!["--scene", "lobby.tscn", "--stop-worker"],
        vec!["--strict-methods", "--stop-worker"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_gdkit"))
            .arg("check")
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        assert!(diagnostic.contains("--help"), "{args:?}: {diagnostic}");
    }
}

impl Project {
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_gdkit"))
            .env_remove("GDKIT_GODOT")
            .args(["check"])
            .arg(&self.0)
            .arg("--godot")
            .arg(std::env::var_os("GDKIT_TEST_GODOT").unwrap())
            .args(args)
            .output()
            .unwrap()
    }

    fn expect(&self, args: &[&str], code: i32, message: &str) {
        let output = self.run(args);
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.status.code(), Some(code), "{text}");
        assert!(text.contains(message), "{text}");
    }

    fn write(&self, name: &str, text: &str) {
        fs::write(self.0.join(name), text).unwrap();
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = self.run(&["--stop-worker"]);
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT pointing to a Godot 4 editor"]
fn smoke_checks_execute_ready_and_enforce_timeout() {
    let project = Project(std::env::temp_dir().join(format!("gdkit-smoke-{}", std::process::id())));
    fs::create_dir(&project.0).unwrap();
    project.write("project.godot", "config_version=5\n");
    project.write("lobby.gd", "extends Node\n@onready var label: RichTextLabel = $Title\nfunc _ready() -> void:\n\tlabel.push_tornado(1.0, 5.0)\n");
    project.write("lobby.tscn", "[gd_scene load_steps=2 format=3]\n[ext_resource type=\"Script\" path=\"res://lobby.gd\" id=\"1\"]\n[node name=\"Lobby\" type=\"Node\"]\nscript = ExtResource(\"1\")\n[node name=\"Title\" type=\"RichTextLabel\" parent=\".\"]\n");
    project.expect(&[], 0, "check passed");
    let raw = Command::new(std::env::var_os("GDKIT_TEST_GODOT").unwrap())
        .args(["--headless", "--path"])
        .arg(&project.0)
        .args(["res://lobby.tscn", "--quit-after", "2"])
        .output()
        .unwrap();
    assert!(raw.status.success());
    assert!(String::from_utf8_lossy(&raw.stderr).contains("SCRIPT ERROR:"));
    project.expect(&["--scene", "res://lobby.tscn"], 1, "push_tornado");
    project.write(
        "lobby.gd",
        "extends Node\nfunc _ready() -> void:\n\tprint(\"smoke ready\")\n",
    );
    project.expect(&["--scene", "lobby.tscn"], 0, "smoke ready");
    project.write(
        "lobby.gd",
        "extends Node\nfunc _ready() -> void:\n\twhile true:\n\t\tOS.delay_msec(10)\n",
    );
    let start = std::time::Instant::now();
    project.expect(
        &["--scene", "lobby.tscn", "--smoke-timeout", "1"],
        1,
        "exceeded 1 seconds",
    );
    assert!(start.elapsed().as_secs() < 15);
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT pointing to a Godot 4 editor"]
fn strict_methods_cover_uncalled_code_dependencies_and_autoloads() {
    let project =
        Project(std::env::temp_dir().join(format!("gdkit-strict-{}", std::process::id())));
    fs::create_dir(&project.0).unwrap();
    let settings = "config_version=5\n[debug]\ngdscript/warnings/enable=false\n";
    project.write("project.godot", settings);
    let broken = "extends Node\nvar label: RichTextLabel\nfunc never_called() -> void:\n\tlabel.push_tornado(1.0, 5.0)\n";
    project.write("lobby.gd", broken);
    project.write("lobby.tscn", "[gd_scene load_steps=2 format=3]\n[ext_resource type=\"Script\" path=\"res://lobby.gd\" id=\"1\"]\n[node name=\"Lobby\" type=\"Node\"]\nscript = ExtResource(\"1\")\n");
    project.expect(&[], 0, "check passed");
    for args in [
        &["--strict-methods"][..],
        &["--strict-methods"][..],
        &["--strict-methods", "--fresh"][..],
    ] {
        project.expect(args, 1, "push_tornado");
    }
    assert_eq!(
        fs::read_to_string(project.0.join("project.godot")).unwrap(),
        settings
    );
    project.write(
        "lobby.gd",
        &broken.replace("push_tornado(1.0, 5.0)", "clear()"),
    );
    project.expect(&["--strict-methods"], 0, "strict method validation");
    project.write(
        "lobby.gd",
        &broken.replace(
            "\tlabel.push_tornado",
            "\t@warning_ignore(\"unsafe_method_access\")\n\tlabel.push_tornado",
        ),
    );
    project.expect(&["--strict-methods"], 0, "check passed");
    project.write("lobby.gd", "extends Node\nclass CustomLabel extends RichTextLabel:\n\tfunc push_tornado(_a: float, _b: float) -> void:\n\t\tpass\nvar label: CustomLabel\nfunc never_called() -> void:\n\tlabel.push_tornado(1.0, 5.0)\n");
    project.expect(&["--strict-methods"], 0, "check passed");
    project.write("lobby.gd", broken);
    project.write(
        "entry.gd",
        "extends Node\nconst Lobby = preload(\"res://lobby.gd\")\n",
    );
    project.write(
        "project.godot",
        "config_version=5\n[autoload]\nEntry=\"*res://entry.gd\"\n",
    );
    project.expect(&["--strict-methods"], 1, "push_tornado");
    project.write(
        "gdkit.toml",
        "[engine]\nexecutable='unused'\n[check]\nstrict_methods=true\n",
    );
    project.expect(&[], 1, "push_tornado");
}
