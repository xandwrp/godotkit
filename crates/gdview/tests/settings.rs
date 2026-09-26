// Acceptance tests for gdview::settings. Offline unless prefixed real_engine_.
#![allow(unused)]

use gdview::ResPath;
use gdview::autoload::AutoloadTarget;
use gdview::respath::Uid;
use gdview::settings::{DirectoryRuleMode, Settings, WarningPolicy};
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
#[ignore = "scaffold"]
fn main_scene_reads_application_run_main_scene_as_res_or_uid() {
    todo!()
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

#[test]
#[ignore = "scaffold"]
fn input_actions_parse_deadzone_and_key_joypad_mouse_events() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn layer_names_cover_2d_3d_render_physics_navigation_and_avoidance() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn window_reports_size_mode_and_stretch_with_godot_defaults() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn config_version_and_features_are_typed() {
    todo!()
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
