//! Turning engine output into structured diagnostics. Pure text processing.
//!
//! # Tests (tests/diagnostics.rs)
//! - `parses_error_script_error_and_warning_headers_on_either_stream`
//! - `attaches_following_stack_frames_until_next_header_same_stream_only`
//! - `stack_frame_parses_at_lines_and_indexed_backtrace_lines_with_line_and_column`
//! - `source_is_first_res_frame`
//! - `identical_diagnostics_collapse_with_occurrence_count_preserving_first_order`
//! - `large_distinct_error_flood_preserves_order_and_counts`
//! - `shutdown_leak_messages_are_classified_not_dropped`
//! - `ignore_rules_match_exact_message_and_own_source_frame_only`
//! - `has_errors_is_true_for_zero_exit_script_errors`
//! - `unresolved_uid_is_extracted_from_message`
//! - `identity_ignores_line_and_occurrences_but_keeps_message_and_resource`
//! - `suggestions_for_nonexistent_function_come_from_the_api_index`

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::config::IgnoreRule;
use crate::process::{Captured, OutputStream};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Warning,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    /// blake3 over severity, code, message, and resource; hex, 16 chars.
    /// Line, column, occurrences, and sequence are excluded so an edit that
    /// moves a diagnostic does not make it "new".
    pub fn compute_identity(&self) -> String {
        let mut hasher = blake3::Hasher::new();
        let severity = match self.severity {
            Severity::Warning => "warning",
            Severity::Error => "error",
        };
        for part in [
            severity,
            self.code.as_deref().unwrap_or(""),
            &self.message,
            self.resource.as_deref().unwrap_or(""),
        ] {
            hasher.update(&(part.len() as u64).to_le_bytes());
            hasher.update(part.as_bytes());
        }
        hasher.finalize().to_hex()[..16].to_owned()
    }
}

/// Fills `suggestions` for messages that name a missing method, property, class,
/// or node (`Nonexistent function 'x' in base 'Y'`, `Node not found: "A/B"`).
pub fn suggest(diagnostics: &mut [Diagnostic], api: Option<&gdview::api::ApiIndex>) {
    let Some(api) = api else { return };
    for diagnostic in diagnostics {
        let message = &diagnostic.message;
        let method = quoted_after(message, "Nonexistent function ")
            .or_else(|| quoted_after(message, "Nonexistent method "));
        let property = quoted_after(message, "Invalid access to property or key ")
            .or_else(|| quoted_after(message, "Nonexistent property "));
        let class = quoted_after(message, "Could not find type ")
            .or_else(|| quoted_after(message, "Could not find class "));
        let Some(name) = method.or(property).or(class) else {
            continue;
        };
        let mut candidates = std::collections::BTreeSet::new();
        if class.is_some() {
            candidates.extend(api.classes.keys().map(String::as_str));
            candidates.extend(api.builtin_classes.keys().map(String::as_str));
            candidates.extend(api.extension_classes.iter().map(String::as_str));
        } else if let Some(base) = quoted_after(message, "in base ")
            .or_else(|| quoted_after(message, "on a base object of type "))
        {
            let base = base.split('(').next().unwrap_or(base).trim();
            let mut next = Some(base);
            let mut visited = std::collections::BTreeSet::new();
            while let Some(name) = next {
                if !visited.insert(name) {
                    break;
                }
                let Some(class) = api.class(name) else { break };
                if method.is_some() {
                    candidates.extend(class.methods.iter().map(|m| m.name.as_str()));
                } else {
                    candidates.extend(class.properties.iter().map(|p| p.name.as_str()));
                }
                next = class.parent.as_deref();
            }
        }
        let query = name.to_lowercase();
        let threshold = (query.chars().count() / 3).clamp(1, 3);
        let mut ranked: Vec<_> = candidates
            .into_iter()
            .filter(|candidate| *candidate != name)
            .map(|candidate| (edit_distance(&query, &candidate.to_lowercase()), candidate))
            .filter(|(distance, _)| *distance <= threshold)
            .collect();
        ranked.sort_unstable();
        for (_, candidate) in ranked.into_iter().take(5) {
            if !diagnostic.suggestions.iter().any(|s| s == candidate) {
                diagnostic.suggestions.push(candidate.to_owned());
            }
        }
        // Node paths require a scene tree, which an engine API index does not contain.
    }
}

fn quoted_after<'a>(message: &'a str, prefix: &str) -> Option<&'a str> {
    let rest = message.split_once(prefix)?.1;
    let quote = rest.chars().next()?;
    if !matches!(quote, '\'' | '"') {
        return None;
    }
    rest[1..].split_once(quote).map(|(value, _)| value)
}

fn edit_distance(left: &str, right: &str) -> usize {
    let right: Vec<_> = right.chars().collect();
    let mut row: Vec<_> = (0..=right.len()).collect();
    for (i, a) in left.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, b) in right.iter().enumerate() {
            let previous = row[j + 1];
            row[j + 1] = (previous + 1)
                .min(row[j] + 1)
                .min(diagonal + usize::from(a != *b));
            diagonal = previous;
        }
    }
    row[right.len()]
}

/// Parses every header line and its frames. Duplicates collapse into `occurrences`.
pub fn parse(captured: &Captured, sequence_base: u64) -> Vec<Diagnostic> {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut active = [None, None];
    for event in &captured.lines {
        let stream_index = match event.stream {
            OutputStream::Stdout => 0,
            OutputStream::Stderr => 1,
        };
        let text = event.text();
        for line in text.lines() {
            if let Some((severity, code, message)) = diagnostic_header(line) {
                active[stream_index] = Some(diagnostics.len());
                diagnostics.push(Diagnostic {
                    sequence: sequence_base.saturating_add(event.sequence as u64),
                    severity,
                    stream: event.stream.into(),
                    code: code.map(str::to_owned),
                    message: message.to_owned(),
                    resource: None,
                    line: None,
                    column: None,
                    frames: Vec::new(),
                    timestamp_unix_ms: Some(event.observed_at_unix_ms),
                    occurrences: 1,
                    is_shutdown_noise: shutdown_noise(message),
                    identity: String::new(),
                    suggestions: Vec::new(),
                });
            } else if let Some(index) = active[stream_index]
                && let Some(frame) = stack_frame(line)
            {
                let diagnostic = &mut diagnostics[index];
                if diagnostic.resource.is_none()
                    && frame
                        .resource
                        .as_deref()
                        .is_some_and(|path| path.starts_with("res://"))
                {
                    diagnostic.resource = frame.resource.clone();
                    diagnostic.line = frame.line;
                    diagnostic.column = frame.column;
                }
                diagnostic.frames.push(frame);
            }
        }
    }
    let mut occurrences = vec![0_u32; diagnostics.len()];
    {
        let mut first_indices = HashMap::with_capacity(diagnostics.len());
        for (index, diagnostic) in diagnostics.iter().enumerate() {
            // Borrow full keys: hash collisions still compare all fields, and no
            // message/frame copies are needed. Baseline identity omits locations.
            let key = (
                diagnostic.severity,
                diagnostic.stream,
                diagnostic.code.as_deref(),
                diagnostic.message.as_str(),
                diagnostic.frames.as_slice(),
            );
            let first = *first_indices.entry(key).or_insert(index);
            occurrences[first] = occurrences[first].saturating_add(1);
        }
    }
    diagnostics
        .into_iter()
        .zip(occurrences)
        .filter_map(|(mut diagnostic, count)| {
            if count == 0 {
                return None;
            }
            diagnostic.occurrences = count;
            diagnostic.identity = diagnostic.compute_identity();
            Some(diagnostic)
        })
        .collect()
}

fn diagnostic_header(line: &str) -> Option<(Severity, Option<&'static str>, &str)> {
    let line = line.trim();
    for (prefix, severity, code) in [
        ("SCRIPT ERROR:", Severity::Error, Some("SCRIPT_ERROR")),
        ("ERROR:", Severity::Error, None),
        ("WARNING:", Severity::Warning, None),
    ] {
        if let Some(message) = line.strip_prefix(prefix) {
            return Some((severity, code, message.trim()));
        }
    }
    None
}

fn shutdown_noise(message: &str) -> bool {
    (message.contains("RID allocations of type") && message.ends_with("were leaked at exit."))
        || (message.contains("RIDs of type") && message.ends_with("were leaked."))
        || message.starts_with("ObjectDB instances leaked at exit")
        || message.starts_with("ObjectDB instances were leaked at exit")
        || message.split_once(' ').is_some_and(|(count, rest)| {
            count.parse::<u64>().is_ok() && rest.starts_with("resources still in use at exit")
        })
}

/// Removes diagnostics matching a rule; returns how many were suppressed.
#[allow(clippy::ptr_arg)] // removal needs the Vec
pub fn apply_ignore_rules(diagnostics: &mut Vec<Diagnostic>, rules: &[IgnoreRule]) -> usize {
    let before = diagnostics.len();
    diagnostics.retain(|diagnostic| {
        !rules.iter().any(|rule| {
            diagnostic_header(&rule.message).is_some_and(|(severity, code, message)| {
                severity == diagnostic.severity
                    && code == diagnostic.code.as_deref()
                    && message == diagnostic.message
                    && diagnostic.resource.as_deref() == Some(rule.source.as_str())
            })
        })
    });
    before - diagnostics.len()
}

pub fn has_errors(diagnostics: &[Diagnostic]) -> bool {
    diagnostics.iter().any(|d| d.severity == Severity::Error)
}

/// `uid://…` from an "Unrecognized UID" message.
pub fn unresolved_uid(message: &str) -> Option<&str> {
    let uid = quoted_after(message, "Unrecognized UID: ")?;
    let suffix = uid.strip_prefix("uid://")?;
    (!suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_alphanumeric())).then_some(uid)
}

pub fn stack_frame(line: &str) -> Option<StackFrame> {
    let line = line.trim();
    let body = if let Some(body) = line.strip_prefix("at:") {
        body.trim()
    } else {
        let (index, body) = line.strip_prefix('[')?.split_once(']')?;
        if index.is_empty() || !index.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        body.trim()
    };
    let body = body.strip_suffix(')')?;
    // The location may itself contain parentheses (e.g. a resource directory).
    let (function, location) = body
        .split_once(" (")
        .or_else(|| body.strip_prefix('(').map(|s| ("", s)))?;
    let (mut resource, mut line, mut column) = (location, None, None);
    if let Some((before, value)) = resource.rsplit_once(':')
        && let Ok(value) = value.parse::<u32>()
    {
        resource = before;
        line = Some(value);
        if let Some((before, value)) = resource.rsplit_once(':')
            && let Ok(value) = value.parse::<u32>()
        {
            resource = before;
            column = line;
            line = Some(value);
        }
    }
    Some(StackFrame {
        function: (!function.trim().is_empty()).then(|| function.trim().to_owned()),
        resource: (!resource.is_empty()).then(|| resource.to_owned()),
        line,
        column,
    })
}
