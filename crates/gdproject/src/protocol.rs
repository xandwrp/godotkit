//! The wire contract between Rust and every GDScript harness.
//!
//! Harnesses print exactly one line starting with [`RESULT_PREFIX`] followed by
//! a JSON [`Envelope`]. Everything else on stdout/stderr is engine output and is
//! parsed by [`crate::diagnostics`]. Variant payloads use `gdview::variant`'s
//! grammar, produced by `harness/protocol.gd`.
//!
//! # Tests (tests/protocol.rs)
//! - `parse_envelope_finds_the_single_result_line_among_noise`
//! - `parse_envelope_rejects_missing_duplicate_and_malformed_results`
//! - `parse_envelope_rejects_protocol_version_mismatch_before_payload_decode`
//! - `error_envelopes_surface_stage_and_message`
//! - `documented_variant_json_shapes_survive_envelope_transport`
//! - `engine_golden_variant_payload_survives_envelope_transport`
//! - `engine_golden_cases_decode_and_reencode_identically`
//!
//!   Engine (`#[ignore]`, GDKIT_TEST_GODOT):
//! - `real_engine_protocol_gd_matches_golden_fixture` (set GDKIT_REFRESH_GOLDEN
//!   to rewrite the fixture and its spike copy from engine output)
//! - `real_engine_protocol_gd_rejects_cycles_budgets_and_non_canonical_input`
//!
//! `tests/fixtures/protocol_golden.json` is `protocol.gd`'s encoding of every
//! contract case, produced by the real-engine test. Offline, every case must
//! decode with `gdview::variant` and re-encode to identical JSON, so the two
//! codecs are tested against each other. Harness output escapes control
//! characters, which Godot's `JSON.stringify` writes raw.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;
pub const RESULT_PREFIX: &str = "GDKIT_RESULT:";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Envelope<T> {
    pub protocol: u32,
    pub harness: String,
    pub ok: bool,
    #[serde(default = "Option::default")]
    pub payload: Option<T>,
    #[serde(default)]
    pub error: Option<HarnessError>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HarnessError {
    pub stage: String,
    pub message: String,
    #[serde(default)]
    pub field: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("harness did not print a result line")]
    Missing,
    #[error("harness printed more than one result line")]
    Multiple,
    #[error("result line is not valid JSON: {0}")]
    Malformed(String),
    #[error("protocol version mismatch: gdkit speaks {expected}, harness spoke {found}")]
    VersionMismatch { expected: u32, found: u32 },
    #[error("payload did not match the expected shape: {0}")]
    Payload(String),
}

/// Extracts and decodes the envelope from captured output lines.
///
/// Only a prefix at the start of a line counts. Result cardinality is checked
/// before JSON decoding, and the protocol version before envelope or payload
/// decoding. Invalid JSON or envelope metadata is [`ProtocolError::Malformed`];
/// a non-null payload that cannot deserialize as `T` is [`ProtocolError::Payload`].
/// Missing and null payloads both become `None`, as in [`Envelope`]'s serde contract.
/// Harness failures remain envelopes; the runner interprets `ok` and `error`.
pub fn parse_envelope<T: DeserializeOwned>(
    lines: impl Iterator<Item = String>,
) -> Result<Envelope<T>, ProtocolError> {
    let mut result = None;
    for line in lines {
        if let Some(json) = line.strip_prefix(RESULT_PREFIX) {
            if result.is_some() {
                return Err(ProtocolError::Multiple);
            }
            result = Some(json.to_owned());
        }
    }
    let json = result.ok_or(ProtocolError::Missing)?;

    // Read only the version first: a newer wire format may change the rest of
    // the envelope as well as the payload.
    #[derive(Deserialize)]
    struct Version {
        protocol: u32,
    }
    let version: Version =
        serde_json::from_str(&json).map_err(|error| ProtocolError::Malformed(error.to_string()))?;
    if version.protocol != PROTOCOL_VERSION {
        return Err(ProtocolError::VersionMismatch {
            expected: PROTOCOL_VERSION,
            found: version.protocol,
        });
    }

    let envelope: Envelope<serde_json::Value> =
        serde_json::from_str(&json).map_err(|error| ProtocolError::Malformed(error.to_string()))?;
    let payload = envelope
        .payload
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| ProtocolError::Payload(error.to_string()))?;
    Ok(Envelope {
        protocol: envelope.protocol,
        harness: envelope.harness,
        ok: envelope.ok,
        payload,
        error: envelope.error,
    })
}
