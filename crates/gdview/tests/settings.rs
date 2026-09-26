// Acceptance tests for gdview::settings. Offline unless prefixed real_engine_.
#![allow(unused)]

use gdview::ResPath;
use gdview::autoload::AutoloadTarget;
use gdview::respath::Uid;
use gdview::settings::{
    DirectoryRuleMode, InputEvent, MainScene, Settings, WarningPolicy, WindowSettings,
};
use gdview::uid::UidMap;

const PROJECT: &str = r#"; Engine configuration file.
; It's best edited using the editor UI.

config_version=5

[application]

config/name="Warehouse [beta]"
config/features=PackedStringArray("4.7", "Forward Plus")

[autoload]

Zeta="*res://systems/zeta.gd"
Alpha="res://ui/alpha.tscn"
Net="*uid://d17o2ql1av3dm"

[input]

jump={
"deadzone": 0.2,
"events": [Object(InputEventKey,"resource_local_to_scene":false,"keycode":32,"unicode":0)
]
}
say="a ] \" { string
over lines"

[rendering]
; x=1
"#;

#[test]
fn parses_sections_keys_and_quoted_values() {
    let settings = Settings::parse(PROJECT).unwrap();
    assert_eq!(settings.get("", "config_version"), Some("5"));
    assert_eq!(
        settings.get("application", "config/name"),
        Some("\"Warehouse [beta]\"")
    );
    assert_eq!(
        settings.get("application", "config/features"),
        Some("PackedStringArray(\"4.7\", \"Forward Plus\")")
    );
    let action = settings.get("input", "jump").unwrap();
    assert!(action.starts_with('{') && action.ends_with('}') && action.contains("\n"));
    assert!(action.contains("\"events\": [Object(InputEventKey"));
    assert_eq!(
        settings.get("input", "say"),
        Some("\"a ] \\\" { string\nover lines\"")
    );
    assert!(
        settings.get("rendering", "x").is_none(),
        "comments are skipped"
    );
}

#[test]
fn get_is_section_scoped_not_prefix_matched() {
    let settings = Settings::parse(
        "[debug]\ngdscript/warnings/enable=false\n[other]\nother/gdscript/warnings/enable=true\ngdscript/warnings/enable=true\n[debug]\ngdscript/warnings/enable=true\n",
    )
    .unwrap();
    assert_eq!(
        settings.get("debug", "gdscript/warnings/enable"),
        Some("true")
    );
    assert_eq!(
        settings.get("other", "gdscript/warnings/enable"),
        Some("true")
    );
    assert_eq!(settings.get("debug", "warnings/enable"), None);
    assert_eq!(settings.get("", "gdscript/warnings/enable"), None);
}

#[test]
fn main_scene_reads_application_run_main_scene_as_res_or_uid() {
    let main = |text: &str| Settings::parse(text).unwrap().main_scene();
    assert_eq!(
        main("[application]\nrun/main_scene=\"res://scenes/main.tscn\"\n").unwrap(),
        Some(MainScene::Path(
            ResPath::parse("res://scenes/main.tscn").unwrap()
        ))
    );
    assert_eq!(
        main("[application]\nrun/main_scene=\"uid://b8x3kq\"\n").unwrap(),
        Some(MainScene::Uid(Uid("uid://b8x3kq".into())))
    );
    assert_eq!(main("[application]\nrun/main_scene=\"\"\n").unwrap(), None);
    assert_eq!(main(PROJECT).unwrap(), None);
    for bad in [
        "run/main_scene=res://unquoted.tscn",
        "run/main_scene=\"scenes/x.tscn\"",
    ] {
        match main(&format!("[application]\n{bad}\n")) {
            Err(gdview::Error::Setting { key, .. }) => {
                assert_eq!(key, "application/run/main_scene")
            }
            other => panic!("{bad}: {other:?}"),
        }
    }
}

#[test]
fn autoloads_preserve_declaration_order_and_singleton_marker() {
    let settings = Settings::parse(PROJECT).unwrap();
    let autoloads = settings.autoloads().unwrap();
    let target = |target: &AutoloadTarget| match target {
        AutoloadTarget::Path(path) => path.as_str().to_owned(),
        AutoloadTarget::Uid(uid) => uid.0.clone(),
    };
    let listed: Vec<_> = autoloads
        .iter()
        .map(|a| (a.name.as_str(), target(&a.target), a.singleton))
        .collect();
    assert_eq!(
        listed,
        [
            ("Zeta", "res://systems/zeta.gd".to_owned(), true),
            ("Alpha", "res://ui/alpha.tscn".to_owned(), false),
            ("Net", "uid://d17o2ql1av3dm".to_owned(), true),
        ]
    );
    let uids = UidMap::from_claims([(
        Uid("uid://d17o2ql1av3dm".into()),
        ResPath::parse("res://net.gd").unwrap(),
    )]);
    assert_eq!(autoloads.0[2].path(&uids).unwrap().as_str(), "res://net.gd");
    assert_eq!(
        autoloads.0[0].path(&UidMap::default()).unwrap().as_str(),
        "res://systems/zeta.gd"
    );
    assert_eq!(autoloads.0[2].path(&UidMap::default()), None);
    assert!(
        Settings::parse("")
            .unwrap()
            .autoloads()
            .unwrap()
            .0
            .is_empty()
    );
    assert!(
        Settings::parse("[autoload]\nBad=res://unquoted.gd\n")
            .unwrap()
            .autoloads()
            .is_err()
    );
}

fn rules(policy: &WarningPolicy) -> Vec<(&str, DirectoryRuleMode)> {
    policy
        .directory_rules
        .iter()
        .map(|rule| (rule.path.as_str(), rule.mode))
        .collect()
}

#[test]
fn warnings_reports_defaults_when_keys_absent() {
    // Keys outside [debug], or merely sharing a suffix, do not count.
    let settings =
        Settings::parse("[other]\ngdscript/warnings/enable=false\n[debug]\nsettings/x=1\n")
            .unwrap();
    let policy = settings.warnings().unwrap();
    assert!(policy.enabled);
    assert_eq!(
        rules(&policy),
        [("res://addons", DirectoryRuleMode::Exclude)]
    );
    assert!(policy.overrides.is_empty());
}

// Each expectation was read back from Godot 4.7.2's ProjectSettings.
#[test]
fn warnings_read_directory_rules_and_migrate_exclude_addons_like_the_engine() {
    use DirectoryRuleMode::{Exclude, Include};
    let policy = |debug: &str| {
        Settings::parse(&format!("config_version=5\n[debug]\n{debug}"))
            .unwrap()
            .warnings()
            .unwrap()
    };
    let written = policy(
        "gdscript/warnings/enable=false\ngdscript/warnings/directory_rules={\n\"res://addons\": 1,\n\"res://vendor\": 0\n}\ngdscript/warnings/unused_variable=2\ngdscript/warnings/unsafe_method_access=true\ngdscript/warnings/unused_variable=0\n",
    );
    assert!(!written.enabled);
    assert_eq!(
        rules(&written),
        [("res://addons", Include), ("res://vendor", Exclude)]
    );
    assert_eq!(
        written.overrides,
        [
            ("unsafe_method_access".to_owned(), "true".to_owned()),
            ("unused_variable".to_owned(), "0".to_owned()),
        ]
        .into()
    );
    assert_eq!(
        rules(&policy("gdscript/warnings/exclude_addons=false\n")),
        [("res://addons", Include)]
    );
    assert_eq!(
        rules(&policy("gdscript/warnings/exclude_addons=true\n")),
        [("res://addons", Exclude)]
    );
    // The legacy key wins for res://addons and moves it first, in either order.
    for debug in [
        "gdscript/warnings/directory_rules={\n\"res://vendor\": 0,\n\"res://addons\": 0\n}\ngdscript/warnings/exclude_addons=false\n",
        "gdscript/warnings/exclude_addons=false\ngdscript/warnings/directory_rules={\n\"res://vendor\": 0,\n\"res://addons\": 0\n}\n",
    ] {
        assert_eq!(
            rules(&policy(debug)),
            [("res://addons", Include), ("res://vendor", Exclude)],
            "{debug}"
        );
    }
    assert!(
        policy("gdscript/warnings/directory_rules={}\n")
            .directory_rules
            .is_empty()
    );
    assert_eq!(
        rules(&policy(
            "gdscript/warnings/directory_rules={ \"res://a \\\"q\\\"\": 1 }\n"
        )),
        [("res://a \"q\"", Include)]
    );
}

#[test]
fn malformed_warning_settings_are_errors_naming_the_key() {
    for (debug, key) in [
        ("gdscript/warnings/enable=yes", "enable"),
        ("gdscript/warnings/exclude_addons=1", "exclude_addons"),
        (
            "gdscript/warnings/directory_rules={\"res://a\": 2}",
            "directory_rules",
        ),
        (
            "gdscript/warnings/directory_rules={res://a: 0}",
            "directory_rules",
        ),
        ("gdscript/warnings/directory_rules=[]", "directory_rules"),
    ] {
        let settings = Settings::parse(&format!("[debug]\n{debug}\n")).unwrap();
        match settings.warnings() {
            Err(gdview::Error::Setting { key: found, .. }) => {
                assert_eq!(found, format!("debug/gdscript/warnings/{key}"), "{debug}")
            }
            other => panic!("{debug}: {other:?}"),
        }
    }
}

/// `[input]` as the Godot 4.7 editor writes it, trimmed of fields gdkit ignores.
const INPUT: &str = r#"[input]

move_left={
"deadzone": 0.5,
"events": [Object(InputEventKey,"resource_local_to_scene":false,"device":-1,"window_id":0,"alt_pressed":false,"shift_pressed":true,"ctrl_pressed":false,"meta_pressed":false,"pressed":false,"keycode":0,"physical_keycode":65,"key_label":0,"unicode":97,"location":0,"echo":false,"script":null)
, Object(InputEventJoypadMotion,"resource_local_to_scene":false,"device":-1,"axis":0,"axis_value":-1.0,"script":null)
, Object(InputEventJoypadButton,"resource_local_to_scene":false,"device":1,"button_index":13,"pressure":0.0,"pressed":true,"script":null)
, Object(InputEventMouseButton,"resource_local_to_scene":false,"device":-1,"window_id":0,"command_or_control_autoremap":true,"alt_pressed":false,"shift_pressed":false,"button_mask":0,"position":Vector2(0, 0),"global_position":Vector2(0, 0),"factor":1.0,"button_index":1,"canceled":false,"pressed":true,"double_click":false,"script":null)
, Object(InputEventScreenTouch,"resource_local_to_scene":false,"device":-1,"index":0,"script":null)
]
}
no_deadzone={
"events": []
}
ui_accept={
"deadzone": 0.3,
"events": [Object(InputEventKey,"keycode":4194309,"physical_keycode":0,"key_label":0,"unicode":0)
]
}
"#;

#[test]
fn input_actions_parse_deadzone_and_key_joypad_mouse_events() {
    let actions = Settings::parse(INPUT).unwrap().input_actions().unwrap();
    let names: Vec<_> = actions.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(&names[..3], ["move_left", "no_deadzone", "ui_accept"]);

    let move_left = &actions[0];
    assert!(!move_left.builtin && move_left.in_project);
    assert_eq!(move_left.deadzone, 0.5);
    assert_eq!(
        move_left.events,
        [
            InputEvent::Key {
                keycode: None,
                physical_keycode: Some("KEY_A".into()),
                key_label: None,
                modifiers: vec!["shift".into()],
            },
            InputEvent::JoypadMotion {
                device: -1,
                axis: "JOY_AXIS_LEFT_X".into(),
                axis_value: -1.0,
            },
            InputEvent::JoypadButton {
                device: 1,
                button: "JOY_BUTTON_DPAD_LEFT".into(),
            },
            InputEvent::MouseButton {
                button: "MOUSE_BUTTON_LEFT".into(),
                modifiers: vec!["command_or_control".into()],
                double_click: false,
            },
            InputEvent::Other {
                type_name: "InputEventScreenTouch".into(),
            },
        ]
    );
    assert_eq!(actions[1].deadzone, 0.2, "InputMap's default");

    // A project entry replaces the built-in whole, and stays marked built-in.
    let accept = &actions[2];
    assert!(accept.builtin && accept.in_project);
    assert_eq!(accept.deadzone, 0.3);
    assert_eq!(accept.events.len(), 1);
    assert_eq!(actions.iter().filter(|a| a.name == "ui_accept").count(), 1);

    // Untouched built-ins follow, sorted, without the `.macos` feature overrides.
    let builtins: Vec<_> = actions[3..].iter().map(|a| a.name.as_str()).collect();
    assert!(actions[3..].iter().all(|a| a.builtin && !a.in_project));
    assert!(builtins.is_sorted());
    assert!(builtins.contains(&"ui_cancel") && builtins.contains(&"ui_text_backspace_word"));
    assert!(
        builtins
            .iter()
            .all(|name| name.starts_with("ui_") && !name.contains('.'))
    );

    let defaults = Settings::parse("").unwrap().input_actions().unwrap();
    let accept = defaults.iter().find(|a| a.name == "ui_accept").unwrap();
    let keys: Vec<_> = accept
        .events
        .iter()
        .map(|event| match event {
            InputEvent::Key { keycode, .. } => keycode.clone().unwrap(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(keys, ["KEY_ENTER", "KEY_KP_ENTER", "KEY_SPACE"]);
    assert_eq!(accept.deadzone, 0.5);

    for bad in [
        "jump=5",
        "jump={\"events\": 1}",
        "jump={\"events\": [1]}",
        "jump={\"deadzone\": \"x\"}",
    ] {
        match Settings::parse(&format!("[input]\n{bad}\n"))
            .unwrap()
            .input_actions()
        {
            Err(gdview::Error::Setting { key, .. }) => assert_eq!(key, "input/jump"),
            other => panic!("{bad}: {other:?}"),
        }
    }
}

#[test]
fn layer_names_cover_2d_3d_render_physics_navigation_and_avoidance() {
    let names = Settings::parse(
        r#"[layer_names]

2d_render/layer_1="Background"
2d_physics/layer_1="World"
2d_physics/layer_3="Enemies"
2d_navigation/layer_2="Air"
3d_render/layer_20="Minimap"
3d_physics/layer_2="Player"
3d_navigation/layer_1="Ground"
avoidance/layer_4="Crowd"
3d_physics/layer_5=""
2d_physics/layer_3="Foes"
unknown/layer_1="Ignored"
2d_physics/layer_0="Out of range"
2d_physics/layer_33="Out of range"
"#,
    )
    .unwrap()
    .layer_names()
    .unwrap();
    let list = |map: &std::collections::BTreeMap<u32, String>| {
        map.iter()
            .map(|(i, n)| format!("{i}={n}"))
            .collect::<Vec<_>>()
    };
    assert_eq!(list(&names.render_2d), ["1=Background"]);
    assert_eq!(
        list(&names.physics_2d),
        ["1=World", "3=Foes"],
        "last value wins"
    );
    assert_eq!(list(&names.navigation_2d), ["2=Air"]);
    assert_eq!(list(&names.render_3d), ["20=Minimap"]);
    assert_eq!(
        list(&names.physics_3d),
        ["2=Player"],
        "empty names are unnamed"
    );
    assert_eq!(list(&names.navigation_3d), ["1=Ground"]);
    assert_eq!(list(&names.avoidance), ["4=Crowd"]);
    assert!(
        Settings::parse("[layer_names]\n2d_physics/layer_1=World\n")
            .unwrap()
            .layer_names()
            .is_err()
    );
}

#[test]
fn window_reports_size_mode_and_stretch_with_godot_defaults() {
    let defaults = Settings::parse(PROJECT).unwrap().window().unwrap();
    assert_eq!(
        defaults,
        WindowSettings {
            width: 1152,
            height: 648,
            width_override: 0,
            height_override: 0,
            mode: "windowed".into(),
            resizable: true,
            stretch_mode: "disabled".into(),
            stretch_aspect: "keep".into(),
            stretch_scale: 1.0,
            stretch_scale_mode: "fractional".into(),
        }
    );
    let set = Settings::parse(
        r#"[display]

window/size/viewport_width=320
window/size/viewport_height=180
window/size/window_width_override=1280
window/size/window_height_override=720
window/size/mode=3
window/size/resizable=false
window/stretch/mode="viewport"
window/stretch/aspect="expand"
window/stretch/scale=2.0
window/stretch/scale_mode="integer"
"#,
    )
    .unwrap()
    .window()
    .unwrap();
    assert_eq!(
        set,
        WindowSettings {
            width: 320,
            height: 180,
            width_override: 1280,
            height_override: 720,
            mode: "fullscreen".into(),
            resizable: false,
            stretch_mode: "viewport".into(),
            stretch_aspect: "expand".into(),
            stretch_scale: 2.0,
            stretch_scale_mode: "integer".into(),
        }
    );
    for bad in [
        "window/size/mode=9",
        "window/size/viewport_width=wide",
        "window/stretch/mode=viewport",
    ] {
        match Settings::parse(&format!("[display]\n{bad}\n"))
            .unwrap()
            .window()
        {
            Err(gdview::Error::Setting { key, .. }) => {
                assert_eq!(key, format!("display/{}", bad.split('=').next().unwrap()))
            }
            other => panic!("{bad}: {other:?}"),
        }
    }
}

#[test]
fn config_version_and_features_are_typed() {
    let settings = Settings::parse(PROJECT).unwrap();
    assert_eq!(settings.config_version().unwrap(), Some(5));
    assert_eq!(settings.features().unwrap(), ["4.7", "Forward Plus"]);

    let empty = Settings::parse("").unwrap();
    assert_eq!(empty.config_version().unwrap(), None);
    assert!(empty.features().unwrap().is_empty());

    assert!(
        Settings::parse("config_version=five\n")
            .unwrap()
            .config_version()
            .is_err()
    );
    assert!(
        Settings::parse("[application]\nconfig/features=[\"4.7\"]\n")
            .unwrap()
            .features()
            .is_err()
    );
}

#[test]
fn malformed_lines_produce_line_numbered_parse_errors() {
    for (source, line) in [
        ("config_version=5\n\nnot a pair\n", 3),
        ("[application\n", 1),
        ("a=1\nb={\n\"x\": [1,\n", 2),
        ("=1\n", 1),
    ] {
        match Settings::parse(source) {
            Err(gdview::Error::Parse { line: at, .. }) => assert_eq!(at, line, "{source:?}"),
            other => panic!("{source:?}: {other:?}"),
        }
    }
}

/// The built-in action table is generated from one engine build. When the
/// engine gdkit targets changes, this says whether the table must be regenerated
/// (see `src/settings/builtin_input.gd`).
#[test]
#[ignore = "requires GDKIT_TEST_GODOT"]
fn real_engine_builtin_input_actions_match_the_table() {
    let godot = std::env::var_os("GDKIT_TEST_GODOT").expect("set GDKIT_TEST_GODOT to opt in");
    let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let scratch = tempfile::tempdir().unwrap();
    std::fs::write(scratch.path().join("project.godot"), "config_version=5\n").unwrap();
    let output = std::process::Command::new(&godot)
        .args(["--headless", "--path"])
        .arg(scratch.path())
        .arg("--script")
        .arg(crate_dir.join("src/settings/builtin_input.gd"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    // Past the engine's banner and the version line, which names the build.
    let body = |text: &str| {
        let table = &text[text.find("; Godot").expect("the table's header")..];
        table.split_once('\n').unwrap().1.trim_end().to_owned()
    };
    let checked_in =
        std::fs::read_to_string(crate_dir.join("src/settings/builtin_input.godot")).unwrap();
    assert!(
        body(&stdout) == body(&checked_in),
        "this engine's built-in input actions differ from src/settings/builtin_input.godot; regenerate it"
    );
}
