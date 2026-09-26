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
//! - `unrecognized_lines_end_a_block_so_shader_frames_never_attach_elsewhere`
//! - `copy_root_paths_are_rewritten_to_res_in_messages_and_frames`
//! - `embedded_resource_locations_fill_fields_and_leave_identity_stable`
//! - `mentioned_resources_let_ignore_rules_match_engine_messages`

use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;

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
    /// `SCRIPT_ERROR`, `SHADER_ERROR`, `GDKIT_SCRIPT_CLASS_CACHE`, … when known.
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
    /// moves a diagnostic does not make it "new". That includes a line number
    /// the engine embeds in the message (`res://a.tres:4 - Parse Error: …`).
    pub fn compute_identity(&self) -> String {
        let mut hasher = blake3::Hasher::new();
        let severity = match self.severity {
            Severity::Warning => "warning",
            Severity::Error => "error",
        };
        for part in [
            severity,
            self.code.as_deref().unwrap_or(""),
            &line_free_message(&self.message),
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
        for candidate in gdview::similar::similar(name, candidates, 5) {
            if !diagnostic.suggestions.contains(&candidate) {
                diagnostic.suggestions.push(candidate);
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

/// Parses every header line and its frames. Duplicates collapse into `occurrences`.
/// Same as [`parse_rooted`] without a project copy to map back to `res://`.
pub fn parse(captured: &Captured, sequence_base: u64) -> Vec<Diagnostic> {
    parse_rooted(captured, sequence_base, None)
}

/// [`parse`] for output of an engine run with `--path copy_root`. Absolute
/// paths inside that per-run copy (e.g. GDExtension `Can't open dynamic
/// library: /tmp/gdkit-…/bin/x.so`) are rewritten to `res://…` in messages and
/// frame resources, so identities and ignore rules survive across runs.
///
/// A diagnostic's `resource`/`line` come from, in order: a location the engine
/// embeds in a text-resource or ConfigFile parse error, the first `res://`
/// frame, then the first `res://` path the message mentions (no line).
///
/// A block is a header followed by frames on the same stream; blank lines and
/// `… backtrace (most recent call first):` continue it, any other line ends it.
pub fn parse_rooted(
    captured: &Captured,
    sequence_base: u64,
    copy_root: Option<&Path>,
) -> Vec<Diagnostic> {
    let roots = copy_root.map(root_spellings).unwrap_or_default();
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
                let message = rewrite_roots(message, &roots).into_owned();
                let (resource, line) = match embedded_location(&message) {
                    Some(location) => (
                        Some(message[location.resource].to_owned()),
                        Some(location.line),
                    ),
                    None => (None, None),
                };
                active[stream_index] = Some(diagnostics.len());
                diagnostics.push(Diagnostic {
                    sequence: sequence_base.saturating_add(event.sequence as u64),
                    severity,
                    stream: event.stream.into(),
                    code: code.map(str::to_owned),
                    is_shutdown_noise: shutdown_noise(&message),
                    message,
                    resource,
                    line,
                    column: None,
                    frames: Vec::new(),
                    timestamp_unix_ms: Some(event.observed_at_unix_ms),
                    occurrences: 1,
                    identity: String::new(),
                    suggestions: Vec::new(),
                });
            } else if let Some(mut frame) = stack_frame(line) {
                let Some(index) = active[stream_index] else {
                    continue;
                };
                if let Some(resource) = &mut frame.resource
                    && let Cow::Owned(rewritten) = rewrite_roots(resource, &roots)
                {
                    *resource = rewritten;
                }
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
            } else if !continues_block(line) {
                // Unrecognized output (a shader listing, a print) ends the block,
                // so later frames can never attach to an unrelated diagnostic.
                active[stream_index] = None;
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
            if diagnostic.resource.is_none() {
                diagnostic.resource = mentioned_resource(&diagnostic.message).map(str::to_owned);
            }
            diagnostic.identity = diagnostic.compute_identity();
            Some(diagnostic)
        })
        .collect()
}

fn diagnostic_header(line: &str) -> Option<(Severity, Option<&'static str>, &str)> {
    let line = line.trim();
    for (prefix, severity, code) in [
        ("SCRIPT ERROR:", Severity::Error, Some("SCRIPT_ERROR")),
        ("SHADER ERROR:", Severity::Error, Some("SHADER_ERROR")),
        ("ERROR:", Severity::Error, None),
        ("WARNING:", Severity::Warning, None),
    ] {
        if let Some(message) = line.strip_prefix(prefix) {
            return Some((severity, code, message.trim()));
        }
    }
    None
}

/// Non-frame lines Godot prints inside an error block.
fn continues_block(line: &str) -> bool {
    let line = line.trim();
    line.is_empty() || line.ends_with("backtrace (most recent call first):")
}

/// Godot 4 exit-time leak reports (`core/object/object.cpp`, `core/io/resource.cpp`,
/// `rid_owner.h`, `renderer_canvas_cull.cpp`, `paged_allocator.h`), plus the
/// uncounted Godot 3 ObjectDB wording.
fn shutdown_noise(message: &str) -> bool {
    if message.starts_with("ObjectDB instances leaked at exit")
        || message.starts_with("ObjectDB instances were leaked at exit")
        || message.starts_with("Pages in use exist at exit in Paged")
        || (message.starts_with("StringName: ") && message.ends_with("string names at exit."))
    {
        return true;
    }
    let Some((count, rest)) = message.split_once(' ') else {
        return false;
    };
    count.bytes().all(|b| b.is_ascii_digit())
        && !count.is_empty()
        && ((rest.starts_with("RID allocations of type ") && rest.ends_with(" leaked at exit."))
            || (rest.starts_with("RID of type ") && rest.ends_with(" was leaked."))
            || (rest.starts_with("RIDs of type ") && rest.ends_with(" were leaked."))
            || rest.starts_with("ObjectDB instance was leaked at exit")
            || rest.starts_with("ObjectDB instances were leaked at exit")
            || rest.starts_with("resources still in use at exit"))
}

/// A location the engine formats into the message itself.
struct EmbeddedLocation {
    resource: Range<usize>,
    /// The `:N` suffix after `resource`.
    line_suffix: Range<usize>,
    line: u32,
}

/// `res://a.tres:4 - Parse Error: …` and `Parse Error: …. [Resource file res://a.tscn:5]`
/// (`resource_format_text.cpp`), `ConfigFile parse error at res://a.cfg:2: …`.
fn embedded_location(message: &str) -> Option<EmbeddedLocation> {
    let (start, located) = if let Some((located, _)) = message.split_once(" - Parse Error: ") {
        (0, located)
    } else if let Some(rest) = message.strip_suffix(']')
        && let Some((_, located)) = rest.rsplit_once("[Resource file ")
    {
        (rest.len() - located.len(), located)
    } else {
        let rest = message.strip_prefix("ConfigFile parse error at ")?;
        let start = message.len() - rest.len();
        // The path itself may contain ':'; the location ends at the first `:N: `.
        let end = rest.match_indices(':').find_map(|(colon, _)| {
            let digits = rest[colon + 1..]
                .bytes()
                .take_while(u8::is_ascii_digit)
                .count();
            (digits > 0 && rest[colon + 1 + digits..].starts_with(": "))
                .then_some(colon + 1 + digits)
        })?;
        (start, &rest[..end])
    };
    let (resource, line) = located.rsplit_once(':')?;
    if !resource.starts_with("res://")
        || line.is_empty()
        || !line.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let colon = start + resource.len();
    Some(EmbeddedLocation {
        resource: start..colon,
        line_suffix: colon..start + located.len(),
        line: line.parse().ok()?,
    })
}

/// The message with any [`embedded_location`] line removed.
fn line_free_message(message: &str) -> Cow<'_, str> {
    match embedded_location(message) {
        Some(location) => Cow::Owned(
            [
                &message[..location.line_suffix.start],
                &message[location.line_suffix.end..],
            ]
            .concat(),
        ),
        None => Cow::Borrowed(message),
    }
}

/// First `res://` path in the message, without quotes, trailing punctuation, or `:N`.
fn mentioned_resource(message: &str) -> Option<&str> {
    let start = message.find("res://")?;
    let rest = &message[start..];
    let quote = message[..start]
        .chars()
        .next_back()
        .filter(|c| matches!(c, '\'' | '"' | '`'));
    let path = match quote {
        Some(quote) => rest.split(quote).next().unwrap_or(rest),
        None => {
            let path = rest.split(char::is_whitespace).next().unwrap_or(rest);
            let mut path = path.trim_end_matches(['.', ',', ';', ':', ')', ']', '\'', '"']);
            while let Some((before, line)) = path.rsplit_once(':')
                && !line.is_empty()
                && line.bytes().all(|b| b.is_ascii_digit())
            {
                path = before;
            }
            path
        }
    };
    (path.len() > "res://".len()).then_some(path)
}

/// Spellings the engine may print for the copy root, longest first: as given,
/// canonical, without a Windows `\\?\` prefix, and with `/` separators.
fn root_spellings(root: &Path) -> Vec<String> {
    let mut spellings = Vec::new();
    let canonical = std::fs::canonicalize(root).ok();
    for path in std::iter::once(root).chain(canonical.as_deref()) {
        let text = path.to_string_lossy();
        let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
        let text = text.trim_end_matches(['/', '\\']);
        // Never rewrite a filesystem root (or nothing) into every path.
        if text.trim_end_matches(':').len() <= 1 {
            continue;
        }
        spellings.push(text.to_owned());
        spellings.push(text.replace('\\', "/"));
    }
    spellings.sort_unstable_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    spellings.dedup();
    spellings
}

/// Replaces whole-path occurrences of any root with `res://`. Linear per root.
fn rewrite_roots<'a>(text: &'a str, roots: &[String]) -> Cow<'a, str> {
    let is_name = |c: char| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '~' | '+');
    let mut text = Cow::Borrowed(text);
    for root in roots {
        if !text.contains(root.as_str()) {
            continue;
        }
        let mut rewritten = String::with_capacity(text.len());
        let mut copied = 0;
        for (start, _) in text.match_indices(root.as_str()) {
            let before = text[..start].chars().next_back();
            let mut after = text[start + root.len()..].chars();
            let next = after.next();
            let whole = !before.is_some_and(|c| is_name(c) || matches!(c, '/' | '\\'))
                && match next {
                    None | Some('/' | '\\') => true,
                    // A sentence-ending period, not a sibling like `gdkit-1.old`.
                    Some('.') => after.next().is_none_or(char::is_whitespace),
                    Some(c) => !is_name(c),
                };
            if !whole || start < copied {
                continue;
            }
            rewritten.push_str(&text[copied..start]);
            rewritten.push_str("res://");
            copied = start + root.len();
            if matches!(next, Some('/' | '\\')) {
                copied += 1;
            }
        }
        if copied > 0 {
            rewritten.push_str(&text[copied..]);
            text = Cow::Owned(rewritten);
        }
    }
    text
}

/// Removes diagnostics matching a rule; returns how many were suppressed.
/// Messages compare exactly, except that a line number embedded in a resource
/// parse error is ignored on both sides, as in [`Diagnostic::compute_identity`].
#[allow(clippy::ptr_arg)] // removal needs the Vec
pub fn apply_ignore_rules(diagnostics: &mut Vec<Diagnostic>, rules: &[IgnoreRule]) -> usize {
    let before = diagnostics.len();
    let rules: Vec<_> = rules
        .iter()
        .filter_map(|rule| {
            let (severity, code, message) = diagnostic_header(&rule.message)?;
            Some((
                severity,
                code,
                line_free_message(message),
                rule.source.as_str(),
            ))
        })
        .collect();
    diagnostics.retain(|diagnostic| {
        let message = line_free_message(&diagnostic.message);
        !rules.iter().any(|(severity, code, rule_message, source)| {
            *severity == diagnostic.severity
                && *code == diagnostic.code.as_deref()
                && *rule_message == message
                && diagnostic.resource.as_deref() == Some(*source)
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
