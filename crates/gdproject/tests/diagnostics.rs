// Acceptance tests for gdproject::diagnostics. Offline unless prefixed real_engine_.
#![allow(unused)]

#[test]
#[ignore = "scaffold"]
fn parses_error_script_error_and_warning_headers_on_either_stream() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn attaches_following_stack_frames_until_next_header_same_stream_only() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn stack_frame_parses_at_lines_and_indexed_backtrace_lines_with_line_and_column() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn source_is_first_res_frame() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn identical_diagnostics_collapse_with_occurrence_count_preserving_first_order() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn shutdown_leak_messages_are_classified_not_dropped() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn ignore_rules_match_exact_message_and_own_source_frame_only() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn has_errors_is_true_for_zero_exit_script_errors() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn unresolved_uid_is_extracted_from_message() {
    todo!()
}

#[test]
fn identity_ignores_line_and_occurrences_but_keeps_message_and_resource() {
    use gdproject::diagnostics::{Diagnostic, Severity, Stream};
    let base = Diagnostic {
        sequence: 0,
        severity: Severity::Error,
        stream: Stream::Tool,
        code: Some("GDKIT_MISSING_NODE".into()),
        message: "node path \"A\" is missing".into(),
        resource: Some("res://a.gd".into()),
        line: Some(3),
        column: Some(1),
        frames: Vec::new(),
        timestamp_unix_ms: None,
        occurrences: 1,
        is_shutdown_noise: false,
        identity: String::new(),
        suggestions: Vec::new(),
    };
    let identity = base.compute_identity();
    let moved = Diagnostic { sequence: 9, line: Some(30), column: None, occurrences: 4, stream: Stream::Stderr, ..base.clone() };
    assert_eq!(moved.compute_identity(), identity);
    for changed in [
        Diagnostic { message: "node path \"B\" is missing".into(), ..base.clone() },
        Diagnostic { resource: Some("res://b.gd".into()), ..base.clone() },
        Diagnostic { resource: None, ..base.clone() },
        Diagnostic { severity: Severity::Warning, ..base.clone() },
        Diagnostic { code: None, ..base.clone() },
    ] {
        assert_ne!(changed.compute_identity(), identity, "{changed:?}");
    }
}

#[test]
#[ignore = "scaffold"]
fn suggestions_for_nonexistent_function_come_from_the_api_index() {
    todo!()
}
