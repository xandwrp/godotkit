//! Turning engine output into structured diagnostics. Pure text processing.
//!
//! # Tests (tests/diagnostics.rs)
//! - `parses_error_script_error_and_warning_headers_on_either_stream`
//! - `attaches_following_stack_frames_until_next_header_same_stream_only`
//! - `stack_frame_parses_at_lines_and_indexed_backtrace_lines_with_line_and_column`
//! - `source_is_first_res_frame`
//! - `identical_diagnostics_collapse_with_occurrence_count_preserving_first_order`
//! - `shutdown_leak_messages_are_classified_not_dropped`
//! - `ignore_rules_match_exact_message_and_own_source_frame_only`
//! - `has_errors_is_true_for_zero_exit_script_errors`
//! - `unresolved_uid_is_extracted_from_message`
//! - `identity_ignores_line_and_occurrences_but_keeps_message_and_resource`
//! - `suggestions_for_nonexistent_function_come_from_the_api_index`

use serde::{Deserialize, Serialize};

use crate::config::IgnoreRule;
use crate::process::{Captured, OutputStream};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Warning,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stream {
    Stdout,
    Stderr,
    /// Produced by gdkit itself, not the engine.
    Tool,
}

impl From<OutputStream> for Stream {
    fn from(stream: OutputStream) -> Self {
        match stream {
            OutputStream::Stdout => Stream::Stdout,
            OutputStream::Stderr => Stream::Stderr,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StackFrame {
    pub function: Option<String>,
    pub resource: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Position in the combined output of the producing phase.
    pub sequence: u64,
    pub severity: Severity,
    pub stream: Stream,
    /// `SCRIPT_ERROR`, `GDKIT_SCRIPT_CLASS_CACHE`, … when known.
    pub code: Option<String>,
    pub message: String,
    pub resource: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub frames: Vec<StackFrame>,
    pub timestamp_unix_ms: Option<u64>,
    pub occurrences: u32,
    /// Shutdown leak reports and similar: real, but not about the project's sources.
    pub is_shutdown_noise: bool,
    /// Stable key for baseline comparison: severity + code + message + resource. Not line.
    pub identity: String,
    /// "Did you mean": filled by [`suggest`] when an api index is available.
    pub suggestions: Vec<String>,
}

impl Diagnostic {
    pub fn compute_identity(&self) -> String {
        todo!()
    }
}

/// Fills `suggestions` for messages that name a missing method, property, class,
/// or node (`Nonexistent function 'x' in base 'Y'`, `Node not found: "A/B"`).
pub fn suggest(diagnostics: &mut [Diagnostic], api: Option<&gdview::api::ApiIndex>) {
    todo!()
}

/// Parses every header line and its frames. Duplicates collapse into `occurrences`.
pub fn parse(captured: &Captured, sequence_base: u64) -> Vec<Diagnostic> {
    todo!()
}

/// Removes diagnostics matching a rule; returns how many were suppressed.
#[allow(clippy::ptr_arg)] // removal needs the Vec
pub fn apply_ignore_rules(diagnostics: &mut Vec<Diagnostic>, rules: &[IgnoreRule]) -> usize {
    todo!()
}

pub fn has_errors(diagnostics: &[Diagnostic]) -> bool {
    diagnostics.iter().any(|d| d.severity == Severity::Error)
}

/// `uid://…` from an "Unrecognized UID" message.
pub fn unresolved_uid(message: &str) -> Option<&str> {
    todo!()
}

pub fn stack_frame(line: &str) -> Option<StackFrame> {
    todo!()
}
