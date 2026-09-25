//! Offline acceptance tests: construct captured output and API data without scaffold calls.
use std::collections::BTreeMap;
use std::time::Duration;

use OutputStream::{Stderr, Stdout};
use gdproject::config::IgnoreRule;
use gdproject::diagnostics::{self, Severity, Stream};
use gdproject::process::{Captured, OutputLine, OutputStream};
use gdview::api::{ApiClass, ApiIndex, ApiMethod, ApiProperty, ApiType};

fn capture(lines: &[(OutputStream, &str)]) -> Captured {
    Captured {
        pid: 42,
        status: Some(Default::default()),
        timed_out: false,
        output_limit_exceeded: false,
        duration: Duration::ZERO,
        lines: lines
            .iter()
            .enumerate()
            .map(|(sequence, (stream, text))| OutputLine {
                sequence: sequence * 2,
                stream: *stream,
                bytes: text.as_bytes().to_vec(),
                observed_at_unix_ms: 1000 + sequence as u64,
            })
            .collect(),
    }
}

#[test]
fn large_distinct_error_flood_preserves_order_and_counts() {
    const DISTINCT: usize = 50_000;
    let mut captured = capture(&[]);
    // Revisit all keys in reverse order, including full source frames, so the
    // result must retain the original order and metadata rather than hash order.
    for (sequence, index) in (0..DISTINCT).chain((0..DISTINCT).rev()).enumerate() {
        captured.lines.push(OutputLine {
            sequence,
            stream: if index % 2 == 0 { Stdout } else { Stderr },
            bytes: format!(
                "SCRIPT ERROR: distinct failure {index}\nat: run (res://flood.gd:{}:2)\n",
                index + 1
            )
            .into_bytes(),
            observed_at_unix_ms: 1000 + sequence as u64,
        });
    }
    let parsed = diagnostics::parse(&captured, 17);
    assert_eq!(parsed.len(), DISTINCT);
    for (index, diagnostic) in parsed.iter().enumerate() {
        assert_eq!(diagnostic.message, format!("distinct failure {index}"));
        assert_eq!(diagnostic.sequence, 17 + index as u64);
        assert_eq!(diagnostic.timestamp_unix_ms, Some(1000 + index as u64));
        assert_eq!(diagnostic.occurrences, 2);
        assert_eq!(diagnostic.line, Some(index as u32 + 1));
        assert_eq!(diagnostic.column, Some(2));
        assert_eq!(diagnostic.frames.len(), 1);
        assert_eq!(diagnostic.identity, diagnostic.compute_identity());
    }
}

#[test]
fn parses_error_script_error_and_warning_headers_on_either_stream() {
    for stream in [Stdout, Stderr] {
        let captured = capture(&[
            (stream, "Godot Engine banner"),
            (stream, "  ERROR: engine failure\r\n"),
            (stream, "\tSCRIPT ERROR: Parse Error: bad type\n"),
            (stream, " WARNING: unused variable "),
            (stream, "ordinary output containing ERROR: not a header"),
        ]);
        let parsed = diagnostics::parse(&captured, 20);
        assert_eq!(parsed.len(), 3);
        for (i, diagnostic) in parsed.iter().enumerate() {
            assert_eq!(diagnostic.sequence, 22 + i as u64 * 2);
            assert_eq!(diagnostic.stream, Stream::from(stream));
            assert_eq!(diagnostic.timestamp_unix_ms, Some(1001 + i as u64));
            assert_eq!(diagnostic.occurrences, 1);
            assert_eq!(diagnostic.identity, diagnostic.compute_identity());
        }
        assert_eq!(parsed[0].severity, Severity::Error);
        assert_eq!(parsed[0].message, "engine failure");
        assert_eq!(parsed[0].code, None);
        assert_eq!(parsed[1].code.as_deref(), Some("SCRIPT_ERROR"));
        assert_eq!(parsed[2].severity, Severity::Warning);
        assert_eq!(parsed[2].message, "unused variable");
    }
    assert!(diagnostics::parse(&capture(&[]), 0).is_empty());
}

#[test]
fn attaches_following_stack_frames_until_next_header_same_stream_only() {
    let parsed = diagnostics::parse(
        &capture(&[
            (Stdout, "at: orphan (res://orphan.gd:1)"),
            (Stderr, "ERROR: first"),
            (Stdout, "WARNING: other stream"),
            (Stderr, "GDScript backtrace (most recent call first):"),
            (Stdout, "at: other (res://other.gd:2)"),
            (Stderr, "[0] own (res://own.gd:3)"),
            (Stderr, "\n"),
            (Stderr, "[1] caller (res://caller.gd:4)"),
            (Stderr, "SCRIPT ERROR: second"),
            (Stderr, "at: second (res://second.gd:5)"),
        ]),
        0,
    );
    assert_eq!(parsed.len(), 3);
    assert_eq!(parsed[0].frames.len(), 2);
    assert_eq!(parsed[0].resource.as_deref(), Some("res://own.gd"));
    assert_eq!(parsed[1].frames.len(), 1);
    assert_eq!(parsed[1].resource.as_deref(), Some("res://other.gd"));
    assert_eq!(parsed[2].frames.len(), 1);
    assert_eq!(parsed[2].resource.as_deref(), Some("res://second.gd"));
}

#[test]
fn stack_frame_parses_at_lines_and_indexed_backtrace_lines_with_line_and_column() {
    for (text, function, resource, line, column) in [
        (
            " at: _ready (res://main.gd:12:8)\r\n",
            Some("_ready"),
            Some("res://main.gd"),
            Some(12),
            Some(8),
        ),
        (
            " [12] reload (core/io/file.cpp:34)",
            Some("reload"),
            Some("core/io/file.cpp"),
            Some(34),
            None,
        ),
        (
            "at: (res://main.gd)",
            None,
            Some("res://main.gd"),
            None,
            None,
        ),
        (
            "[0] f (C:\\project\\file.cpp:9:2)",
            Some("f"),
            Some("C:\\project\\file.cpp"),
            Some(9),
            Some(2),
        ),
        (
            "at: f (res://a (copy)/b.gd:7)",
            Some("f"),
            Some("res://a (copy)/b.gd"),
            Some(7),
            None,
        ),
    ] {
        let frame = diagnostics::stack_frame(text).unwrap();
        assert_eq!(frame.function.as_deref(), function, "{text}");
        assert_eq!(frame.resource.as_deref(), resource, "{text}");
        assert_eq!((frame.line, frame.column), (line, column), "{text}");
    }
    for invalid in [
        "",
        "ordinary (res://a.gd:1)",
        "[x] f (a:1)",
        "[] f (a:1)",
        "at: f (a:1",
        "at: f",
        "[+1] f (a:1)",
    ] {
        assert!(diagnostics::stack_frame(invalid).is_none(), "{invalid}");
    }
}

#[test]
fn source_is_first_res_frame() {
    let parsed = diagnostics::parse(
        &capture(&[
            (Stderr, "ERROR: failure"),
            (Stderr, "at: native (core/file.cpp:20)"),
            (Stderr, "[0] source (res://a.gd:3:4)"),
            (Stderr, "[1] caller (res://b.gd:9:10)"),
            (Stderr, "ERROR: native only"),
            (Stderr, "at: native (core/file.cpp:20)"),
        ]),
        0,
    );
    assert_eq!(parsed[0].frames.len(), 3);
    assert_eq!(parsed[0].resource.as_deref(), Some("res://a.gd"));
    assert_eq!((parsed[0].line, parsed[0].column), (Some(3), Some(4)));
    assert_eq!(parsed[1].resource, None);
    assert_eq!(parsed[1].line, None);
}

#[test]
fn identical_diagnostics_collapse_with_occurrence_count_preserving_first_order() {
    let parsed = diagnostics::parse(
        &capture(&[
            (Stderr, "ERROR: repeated\nat: f (res://a.gd:1)"),
            (Stderr, "WARNING: intervening"),
            (Stderr, "ERROR: repeated\nat: f (res://a.gd:1)"),
            (Stderr, "ERROR: repeated\nat: f (res://a.gd:2)"),
            (Stdout, "ERROR: repeated\nat: f (res://a.gd:1)"),
            (Stderr, "ERROR: repeated\nat: g (res://a.gd:1)"),
            (Stderr, "SCRIPT ERROR: repeated\nat: f (res://a.gd:1)"),
        ]),
        10,
    );
    assert_eq!(parsed.len(), 6);
    assert_eq!(parsed[0].occurrences, 2);
    assert_eq!(parsed[0].sequence, 10);
    assert_eq!(parsed[0].timestamp_unix_ms, Some(1000));
    assert_eq!(parsed[1].message, "intervening");
    assert_eq!(parsed[2].line, Some(2));
    assert_eq!(parsed[0].identity, parsed[2].identity);
    assert!(parsed[1..].iter().all(|d| d.occurrences == 1));
}

#[test]
fn shutdown_leak_messages_are_classified_not_dropped() {
    let parsed = diagnostics::parse(
        &capture(&[
            (
                Stderr,
                "ERROR: 12 RID allocations of type 'DummyTexture' were leaked at exit.",
            ),
            (
                Stderr,
                "WARNING: 21 RIDs of type \"CanvasItem\" were leaked.",
            ),
            (
                Stderr,
                "WARNING: ObjectDB instances leaked at exit (run with --verbose for details).",
            ),
            (
                Stderr,
                "ERROR: 5 resources still in use at exit (run with --verbose for details).",
            ),
            (Stderr, "ERROR: Unrecognized UID: \"uid://missing\"."),
            (Stderr, "SCRIPT ERROR: Parse Error: Invalid type."),
            (Stderr, "ERROR: Unknown engine failure"),
        ]),
        0,
    );
    assert_eq!(parsed.len(), 7);
    assert!(parsed[..4].iter().all(|d| d.is_shutdown_noise));
    assert!(parsed[4..].iter().all(|d| !d.is_shutdown_noise));
    assert!(diagnostics::has_errors(&parsed[..4]));
}

#[test]
fn ignore_rules_match_exact_message_and_own_source_frame_only() {
    let mut parsed = diagnostics::parse(
        &capture(&[
            (Stderr, "ERROR: known\nat: own (res://addons/plugin.gd:1)"),
            (Stderr, "ERROR: known\nat: own (res://addons/plugin.gd:1)"),
            (
                Stderr,
                "ERROR: known extra\nat: own (res://addons/plugin.gd:1)",
            ),
            (
                Stderr,
                "ERROR: known\nat: own (res://game.gd:1)\n[1] caller (res://addons/plugin.gd:1)",
            ),
            (
                Stderr,
                "ERROR: known\nat: own (res://addons/plugin.gd.extra:1)",
            ),
            (
                Stderr,
                "SCRIPT ERROR: known\nat: own (res://addons/plugin.gd:1)",
            ),
            (Stderr, "ERROR: known"),
            (Stdout, "at: unrelated (res://addons/plugin.gd:1)"),
        ]),
        0,
    );
    let rule = IgnoreRule {
        message: "ERROR: known".into(),
        source: gdview::ResPath::parse("res://addons/plugin.gd").unwrap(),
    };
    assert_eq!(diagnostics::apply_ignore_rules(&mut parsed, &[]), 0);
    assert_eq!(
        diagnostics::apply_ignore_rules(&mut parsed, &[rule.clone(), rule]),
        1
    );
    assert_eq!(parsed.len(), 5);
    assert_eq!(parsed[0].message, "known extra");
    assert_eq!(parsed[1].resource.as_deref(), Some("res://game.gd"));
    assert_eq!(parsed[4].resource, None);
}

#[test]
fn has_errors_is_true_for_zero_exit_script_errors() {
    for stream in [Stdout, Stderr] {
        let captured = capture(&[
            (Stdout, "GDKIT_SCRIPT_STARTED"),
            (Stdout, "{\"complete\":true,\"null_loads\":[]}"),
            (
                stream,
                "SCRIPT ERROR: Parse Error: Expected expression.\nat: GDScript::reload (res://broken.gd:3)",
            ),
        ]);
        assert!(captured.success());
        assert!(diagnostics::has_errors(&diagnostics::parse(&captured, 0)));
    }
    assert!(!diagnostics::has_errors(&[]));
    assert!(!diagnostics::has_errors(&diagnostics::parse(
        &capture(&[(Stderr, "WARNING: warning")]),
        0
    )));
}

#[test]
fn unresolved_uid_is_extracted_from_message() {
    for message in [
        "Unrecognized UID: \"uid://abc123\".",
        "ERROR: Unrecognized UID: \"uid://abc123\".",
    ] {
        assert_eq!(diagnostics::unresolved_uid(message), Some("uid://abc123"));
    }
    for message in [
        "uid://abc123",
        "Unrecognized UID: \"res://a.gd\"",
        "Unrecognized UID: \"uid://\"",
        "Unrecognized UID: \"uid://abc",
        "Unrecognized UID: \"uid://a b\"",
    ] {
        assert_eq!(diagnostics::unresolved_uid(message), None, "{message}");
    }
}

#[test]
fn identity_ignores_line_and_occurrences_but_keeps_message_and_resource() {
    let base = diagnostics::parse(
        &capture(&[(Stderr, "SCRIPT ERROR: broken\nat: f (res://a.gd:1:2)")]),
        0,
    )
    .remove(0);
    let identity = base.compute_identity();
    assert_eq!(identity.len(), 16);
    assert!(identity.bytes().all(|b| b.is_ascii_hexdigit()));
    let mut moved = base.clone();
    moved.sequence = 99;
    moved.line = Some(100);
    moved.column = None;
    moved.occurrences = 20;
    moved.stream = Stream::Tool;
    moved.timestamp_unix_ms = None;
    moved.frames.clear();
    moved.suggestions.push("suggestion".into());
    moved.is_shutdown_noise = true;
    assert_eq!(moved.compute_identity(), identity);
    for field in 0..5 {
        let mut changed = base.clone();
        match field {
            0 => changed.message.push('!'),
            1 => changed.resource = Some("res://b.gd".into()),
            2 => changed.resource = None,
            3 => changed.severity = Severity::Warning,
            _ => changed.code = None,
        }
        assert_ne!(changed.compute_identity(), identity);
    }
}

fn class(name: &str, parent: Option<&str>, methods: &[&str]) -> ApiClass {
    ApiClass {
        name: name.into(),
        parent: parent.map(str::to_owned),
        is_builtin: false,
        is_refcounted: false,
        instantiable: true,
        api_type: "core".into(),
        brief: None,
        methods: methods
            .iter()
            .map(|name| ApiMethod {
                name: (*name).into(),
                is_static: false,
                is_const: false,
                is_virtual: false,
                is_vararg: false,
                return_type: ApiType::Void,
                arguments: vec![],
                brief: None,
            })
            .collect(),
        properties: vec![ApiProperty {
            name: "visible".into(),
            type_: ApiType::Variant,
            getter: None,
            setter: None,
            brief: None,
        }],
        signals: vec![],
        enums: vec![],
        constants: vec![],
    }
}

#[test]
fn suggestions_for_nonexistent_function_come_from_the_api_index() {
    let mut api = ApiIndex {
        schema_version: gdview::api::API_INDEX_SCHEMA_VERSION,
        engine_version: "test".into(),
        has_docs: false,
        classes: BTreeMap::from([
            (
                "Child".into(),
                class("Child", Some("Node"), &["move_and_slide"]),
            ),
            ("Node".into(), class("Node", Some("Child"), &["queue_free"])),
        ]),
        builtin_classes: BTreeMap::from([("String".into(), class("String", None, &["split"]))]),
        utility_functions: vec![],
        global_enums: vec![],
        singletons: vec![],
        extension_classes: vec![],
    };
    let mut parsed = diagnostics::parse(
        &capture(&[
            (
                Stderr,
                "SCRIPT ERROR: Invalid call. Nonexistent function 'move_and_slid' in base 'Child'.",
            ),
            (
                Stderr,
                "SCRIPT ERROR: Nonexistent function 'queue_fre' in base 'Child (script.gd)'.",
            ),
            (
                Stderr,
                "SCRIPT ERROR: Nonexistent function 'spli' in base 'String'.",
            ),
            (
                Stderr,
                "SCRIPT ERROR: Invalid access to property or key 'visibl' on a base object of type 'Child'.",
            ),
            (
                Stderr,
                "SCRIPT ERROR: Parse Error: Could not find type \"Nod\" in the current scope.",
            ),
            (Stderr, "ERROR: Node not found: \"A/B\"."),
            (
                Stderr,
                "SCRIPT ERROR: Nonexistent function 'zzzzzzzzzz' in base 'Child'.",
            ),
            (
                Stderr,
                "SCRIPT ERROR: Nonexistent function 'queue_fre' in base 'Unknown'.",
            ),
        ]),
        0,
    );
    diagnostics::suggest(&mut parsed, None);
    assert!(parsed.iter().all(|d| d.suggestions.is_empty()));
    diagnostics::suggest(&mut parsed, Some(&api));
    for (diagnostic, expected) in parsed.iter().zip([
        vec!["move_and_slide"],
        vec!["queue_free"],
        vec!["split"],
        vec!["visible"],
        vec!["Node"],
        vec![],
        vec![],
        vec![],
    ]) {
        assert_eq!(diagnostic.suggestions, expected);
    }
    let once = parsed.clone();
    diagnostics::suggest(&mut parsed, Some(&api));
    assert_eq!(parsed, once);
    api.classes.get_mut("Child").unwrap().methods[0].name = "move_and_slids".into();
    parsed[0].suggestions.clear();
    diagnostics::suggest(&mut parsed[..1], Some(&api));
    assert_eq!(parsed[0].suggestions, ["move_and_slids"]);
}

#[test]
fn malformed_utf8_and_multiline_events_do_not_lose_later_errors() {
    let mut captured = capture(&[
        (
            Stderr,
            "ERROR: first\r\nat: f (res://a.gd:2)\r\nWARNING: second\n",
        ),
        (Stdout, "SCRIPT ERROR: final"),
    ]);
    captured.lines[0].bytes.splice(7..7, [0xff]);
    let parsed = diagnostics::parse(&captured, 0);
    assert_eq!(parsed.len(), 3);
    assert_eq!(parsed[0].message, "�first");
    assert_eq!(parsed[0].line, Some(2));
    assert_eq!(parsed[2].message, "final");
}
