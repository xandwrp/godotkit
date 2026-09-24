//! Binary glTF (`.glb`) reading, enough to list packed animations offline.
//!
//! # Tests (tests/gltf.rs)
//! - `reads_json_and_bin_chunks_with_padding`
//! - `rejects_bad_magic_version_truncated_header_and_chunk_length_overflow` (no panics, ever)
//! - `lists_animations_with_names_channels_samplers_and_length`
//! - `length_is_max_input_time_matching_godot_import_not_key_span`
//! - `unnamed_animations_get_godot_style_index_names`
//! - `filter_is_substring_case_sensitive`

use serde::Serialize;

pub struct Glb<'a> {
    pub json: serde_json::Value,
    pub bin: Option<&'a [u8]>,
}

/// Parses the container. Never panics on malformed input.
pub fn read_glb(bytes: &[u8]) -> crate::Result<Glb<'_>> {
    todo!()
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PackedAnimation {
    pub index: usize,
    pub name: String,
    pub channels: usize,
    pub samplers: usize,
    /// Max sampler input time in seconds, which is what Godot imports as `Animation.length`.
    pub length_seconds: Option<f64>,
    pub first_key_seconds: Option<f64>,
}

pub fn animations(glb: &Glb<'_>) -> crate::Result<Vec<PackedAnimation>> {
    todo!()
}
