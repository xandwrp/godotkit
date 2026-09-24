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
//! - `protocol_gd_encoder_output_matches_gdview_variant_grammar` (golden JSON checked into fixtures, produced once by a real engine)

use serde::{Deserialize, Serialize};
use serde::de::DeserializeOwned;

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
pub fn parse_envelope<T: DeserializeOwned>(
    lines: impl Iterator<Item = String>,
) -> Result<Envelope<T>, ProtocolError> {
    todo!()
}
