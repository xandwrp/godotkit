// Acceptance tests for gdview::syntax.

use gdview::syntax::ast::{self, Member, SourceFile};
use gdview::syntax::{SyntaxKind, parse};

#[test]
fn round_trips_exact_source_including_crlf_bom_tabs_and_trailing_garbage() {
    let sources = [
        "\u{feff}extends Node\r\n\r\nfunc _ready():\r\n\tpass\r\n",
        "extends Node\n\tfunc broken(:\n\t\t)))) ]] @@ \"unterminated\n",
        "var x = 1 # comment\n\n\n   \t\n",
        "",
        "\u{1F600} = ((((",
    ];
    for source in sources {
        let parsed = parse(source);
        assert_eq!(parsed.root().text(), source);
        assert_eq!(parsed.source(), source);
    }
}

#[test]
fn recovers_from_errors_and_reports_byte_ranges() {
    let source = "func a(:\n\tpass\n\nfunc b():\n\tpass\n";
    let parsed = parse(source);
    assert!(!parsed.is_valid());
    let first = &parsed.diagnostics()[0];
    assert!(first.range.start >= source.find('(').unwrap());
    assert!(first.range.end <= source.len());
    let file = SourceFile::cast(parsed.root()).unwrap();
    let names: Vec<_> = file.members().filter_map(|m| m.name()).collect();
    assert!(names.contains(&"b"), "recovered members: {names:?}");
}

#[test]
fn diagnostics_are_ordered_by_offset() {
    let parsed = parse("var = 1\nfunc (:\nvar y = )\nconst\n");
    let starts: Vec<_> = parsed.diagnostics().iter().map(|d| d.range.start).collect();
    assert!(starts.len() >= 3, "{starts:?}");
    let mut sorted = starts.clone();
    sorted.sort();
    assert_eq!(starts, sorted);
}

#[test]
fn deep_nesting_does_not_overflow_the_stack() {
    let source = format!("var x = {}1{}\n", "(".repeat(10_000), ")".repeat(10_000));
    let parsed = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || {
            let parsed = parse(&source);
            (parsed.root().text() == source, parsed.is_valid())
        })
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(parsed, (true, false));
}

const SCRIPT: &str = r#"@tool
class_name Player extends "res://actors/actor.gd"

signal died
signal hit(amount: int, source)
const SPEED := 5.0
var health: int = 3
static var count = 0
enum State { IDLE, RUN }
enum { ANON }

func move(delta: float, scale := 1.0) -> void:
	pass

static func make() -> Player:
	return null

class Inner extends Node2D:
	var inside = 1
	class Deeper:
		func deep(): pass
"#;

#[test]
fn ast_views_expose_class_name_extends_signals_vars_consts_funcs_enums_inner_classes() {
    let parsed = parse(SCRIPT);
    assert!(parsed.is_valid(), "{:?}", parsed.diagnostics());
    let file = SourceFile::cast(parsed.root()).unwrap();
    assert_eq!(file.class_name().unwrap().name(), Some("Player"));
    let extends = file.extends().unwrap();
    assert_eq!(extends.base_text(), "\"res://actors/actor.gd\"");
    assert_eq!(
        extends.base_path().as_deref(),
        Some("res://actors/actor.gd")
    );
    assert_eq!(
        file.script_annotations()
            .map(|a| a.name())
            .collect::<Vec<_>>(),
        ["tool"]
    );

    let members: Vec<_> = file.members().collect();
    let summary: Vec<_> = members
        .iter()
        .map(|m| {
            let kind = match m {
                Member::Signal(_) => "signal",
                Member::Const(_) => "const",
                Member::Var(_) => "var",
                Member::Func(_) => "func",
                Member::Enum(_) => "enum",
                Member::Class(_) => "class",
            };
            (kind, m.name().unwrap_or("-"), m.node().line())
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("signal", "died", 4),
            ("signal", "hit", 5),
            ("const", "SPEED", 6),
            ("var", "health", 7),
            ("var", "count", 8),
            ("enum", "State", 9),
            ("enum", "-", 10),
            ("func", "move", 12),
            ("func", "make", 15),
            ("class", "Inner", 18),
        ]
    );

    let Member::Signal(hit) = members[1] else {
        panic!()
    };
    let params: Vec<_> = hit.parameters().map(|p| (p.name, p.type_text)).collect();
    assert_eq!(params, [("amount", Some("int")), ("source", None)]);

    let Member::Var(health) = members[3] else {
        panic!()
    };
    assert_eq!(health.type_text(), Some("int"));
    assert_eq!(health.initializer().unwrap().trimmed_text(), "3");
    assert!(!health.is_static());
    let Member::Var(count) = members[4] else {
        panic!()
    };
    assert!(count.is_static());

    let Member::Func(moved) = members[7] else {
        panic!()
    };
    let params: Vec<_> = moved
        .parameters()
        .map(|p| (p.name, p.type_text, p.default.map(|d| d.trimmed_text())))
        .collect();
    assert_eq!(
        params,
        [("delta", Some("float"), None), ("scale", None, Some("1.0"))]
    );
    assert_eq!(moved.return_type_text(), Some("void"));
    assert!(moved.body().is_some());
    let Member::Func(make) = members[8] else {
        panic!()
    };
    assert!(make.is_static());

    let Member::Class(inner) = members[9] else {
        panic!()
    };
    assert_eq!(inner.extends().unwrap().base_text(), "Node2D");
    let inner_members: Vec<_> = inner.members().filter_map(|m| m.name()).collect();
    assert_eq!(inner_members, ["inside", "Deeper"]);
    let Some(Member::Class(deeper)) = inner.members().nth(1) else {
        panic!()
    };
    assert!(deeper.extends().is_none());
    assert_eq!(
        deeper
            .members()
            .filter_map(|m| m.name())
            .collect::<Vec<_>>(),
        ["deep"]
    );
}

/// `(member, [(annotation, [argument])])`
type Annotated = Vec<(String, Vec<(String, Vec<String>)>)>;

#[test]
fn annotations_attach_to_the_following_declaration() {
    let source = "@tool\nextends Node\n\n@export var a := 1\n@export_range(0, 10) @onready\nvar b = $B\nvar c\n@rpc(\"any_peer\", \"call_local\")\nfunc d(): pass\n";
    let parsed = parse(source);
    assert!(parsed.is_valid(), "{:?}", parsed.diagnostics());
    let file = SourceFile::cast(parsed.root()).unwrap();
    let annotated: Annotated = file
        .members()
        .map(|m| {
            let annotations = m
                .annotations()
                .map(|a| {
                    (
                        a.name().to_owned(),
                        a.arguments().into_iter().map(str::to_owned).collect(),
                    )
                })
                .collect();
            (m.name().unwrap().to_owned(), annotations)
        })
        .collect();
    let expected: Annotated = vec![
        ("a".into(), vec![("export".into(), vec![])]),
        (
            "b".into(),
            vec![
                ("export_range".into(), vec!["0".into(), "10".into()]),
                ("onready".into(), vec![]),
            ],
        ),
        ("c".into(), vec![]),
        (
            "d".into(),
            vec![(
                "rpc".into(),
                vec!["\"any_peer\"".into(), "\"call_local\"".into()],
            )],
        ),
    ];
    assert_eq!(annotated, expected);
}

#[test]
fn node_paths_and_string_literals_are_exposed_unquoted() {
    let source = "var a = $Body/Arm\nvar b = $\"Quoted Name/X\"\nvar c = %Unique/Child\nvar d = preload(\"res://x.tscn\")\nvar e = load('res://y\\\\z.tres')\n";
    let parsed = parse(source);
    assert!(parsed.is_valid(), "{:?}", parsed.diagnostics());
    let paths: Vec<_> = parsed
        .root()
        .descendants()
        .filter_map(ast::GetNode::cast)
        .map(|g| g.path())
        .collect();
    assert_eq!(paths, ["Body/Arm", "Quoted Name/X", "%Unique/Child"]);
    let preload = parsed
        .root()
        .descendants()
        .find_map(ast::Preload::cast)
        .unwrap();
    assert_eq!(
        ast::string_literal(preload.argument().unwrap()).as_deref(),
        Some("res://x.tscn")
    );
    let call = parsed
        .root()
        .descendants()
        .find_map(ast::CallExpr::cast)
        .unwrap();
    assert_eq!(call.callee_text(), "load");
    assert_eq!(
        ast::string_literal(call.arguments()[0]).as_deref(),
        Some("res://y\\z.tres")
    );
    assert!(
        parsed
            .root()
            .descendants()
            .any(|n| n.kind() == SyntaxKind::GetNodeExpr)
    );
}

#[test]
fn line_col_lookup_is_1_based_and_handles_multibyte() {
    let source = "a\r\néé = 1\n\n  x";
    let parsed = parse(source);
    assert_eq!(parsed.line_col(0), (1, 1));
    assert_eq!(parsed.line_col(3), (2, 1));
    let equals = source.find('=').unwrap();
    assert_eq!(parsed.line_col(equals), (2, 4));
    assert_eq!(parsed.line_col(source.len() - 1), (4, 3));
    assert_eq!(parsed.line_col(source.len() + 10), (4, 4));
}
