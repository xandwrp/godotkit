// Acceptance tests for gdview::respath.

use gdview::ResPath;
use gdview::respath::NodePath;

fn ok(text: &str) -> ResPath {
    ResPath::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"))
}

#[test]
fn parse_accepts_forward_slash_paths_and_rejects_backslashes() {
    assert_eq!(ok("res://scenes/main.tscn").relative(), "scenes/main.tscn");
    assert_eq!(ok("res://").relative(), "");
    assert!(ResPath::parse("res://scenes\\main.tscn").is_err());
    assert!(ResPath::parse("scenes/main.tscn").is_err());
    assert!(ResPath::parse("user://save.tres").is_err());
    assert!(ResPath::parse("uid://abc").is_err());
    assert_eq!(ResPath::from_relative("a/b.gd").unwrap(), ok("res://a/b.gd"));
}

#[test]
fn parse_rejects_dot_dot_and_empty_segments() {
    for bad in ["res://../x", "res://a/../x", "res://a//b", "res://a/", "res:///a", "res://./a", "res://a/."] {
        assert!(ResPath::parse(bad).is_err(), "{bad}");
    }
    assert!(ResPath::parse("res://a..b/c...d").is_ok());
}

#[test]
fn parse_rejects_paths_into_dot_godot() {
    assert!(ResPath::parse("res://.godot").is_err());
    assert!(ResPath::parse("res://.godot/imported/x.ctex").is_err());
    assert!(ResPath::parse("res://addons/.godot/x").is_ok(), "only the project's own .godot is special");
    assert!(ResPath::parse("res://.godotignore").is_ok());
}

#[test]
fn extension_and_file_name_match_godot_semantics() {
    let scene = ok("res://scenes/main.tscn");
    assert_eq!((scene.file_name(), scene.extension()), ("main.tscn", Some("tscn")));
    let script = ok("res://player.gd");
    assert_eq!((script.file_name(), script.extension()), ("player.gd", Some("gd")));
    let import = ok("res://art/icon.png.import");
    assert_eq!((import.file_name(), import.extension()), ("icon.png.import", Some("import")));
    let bare = ok("res://LICENSE");
    assert_eq!((bare.file_name(), bare.extension()), ("LICENSE", None));
    assert_eq!(ok("res://").file_name(), "");
}

#[test]
fn join_and_parent_never_escape_the_root() {
    let dir = ok("res://scenes");
    assert_eq!(dir.join("main.tscn").unwrap(), ok("res://scenes/main.tscn"));
    assert_eq!(ResPath::root().join("a/b").unwrap(), ok("res://a/b"));
    assert!(dir.join("../x").is_err());
    assert!(dir.join("").is_err());
    assert_eq!(ok("res://a/b/c").parent(), Some(ok("res://a/b")));
    assert_eq!(ok("res://a").parent(), Some(ResPath::root()));
    assert_eq!(ResPath::root().parent(), None);
    assert!(ok("res://a/b").starts_with(&ok("res://a")));
    assert!(!ok("res://ab").starts_with(&ok("res://a")));
    assert!(ok("res://a").starts_with(&ResPath::root()));
}

#[test]
fn display_round_trips_exact_input() {
    for text in ["res://", "res://a b/ü.tscn", "res://x.gd"] {
        let path = ok(text);
        assert_eq!(path.to_string(), text);
        assert_eq!(format!("{path:?}"), text);
        let json = serde_json::to_string(&path).unwrap();
        assert_eq!(serde_json::from_str::<ResPath>(&json).unwrap(), path);
    }
    assert!(serde_json::from_str::<ResPath>("\"res://../x\"").is_err());
}

#[test]
fn node_path_join_and_absolute() {
    assert_eq!(NodePath(".".into()).join("A"), NodePath("A".into()));
    assert_eq!(NodePath("A".into()).join("B"), NodePath("A/B".into()));
    assert!(NodePath("/root/Main".into()).is_absolute());
    assert!(!NodePath("A/B".into()).is_absolute());
}
