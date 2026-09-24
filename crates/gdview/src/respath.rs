//! `res://` paths as a validated type.
//!
//! Every place the old code did `strip_prefix("res://")` and `replace('\\', "/")`
//! by hand goes through here. There is exactly one definition of "valid".
//!
//! # Tests (tests/respath.rs)
//! - `parse_accepts_forward_slash_paths_and_rejects_backslashes`
//! - `parse_rejects_dot_dot_and_empty_segments`
//! - `parse_rejects_paths_into_dot_godot`
//! - `extension_and_file_name_match_godot_semantics` (`.tscn`, `.gd`, `.import`, no ext)
//! - `join_and_parent_never_escape_the_root`
//! - `display_round_trips_exact_input`

use std::fmt;

use serde::{Deserialize, Serialize};

/// A normalized `res://` path. Always forward slashes, never `.` or `..`, never
/// empty segments, never under `.godot/`.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ResPath(String);

impl ResPath {
    /// Parses and validates. `res://` prefix is required.
    pub fn parse(text: &str) -> crate::Result<Self> {
        todo!()
    }

    /// Builds from a project-relative path such as `scenes/main.tscn`.
    pub fn from_relative(relative: &str) -> crate::Result<Self> {
        todo!()
    }

    /// The path without the `res://` prefix. Never starts with `/`.
    pub fn relative(&self) -> &str {
        todo!()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn extension(&self) -> Option<&str> {
        todo!()
    }

    pub fn file_name(&self) -> &str {
        todo!()
    }

    pub fn parent(&self) -> Option<ResPath> {
        todo!()
    }

    pub fn join(&self, segment: &str) -> crate::Result<ResPath> {
        todo!()
    }
}

impl fmt::Debug for ResPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Display for ResPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for ResPath {
    type Error = crate::Error;
    fn try_from(value: String) -> crate::Result<Self> {
        ResPath::parse(&value)
    }
}

impl From<ResPath> for String {
    fn from(value: ResPath) -> String {
        value.0
    }
}

/// A `uid://` identifier as it appears in scenes and `.uid` sidecars.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Uid(pub String);

/// A Godot node path such as `Player/Camera3D` or `/root/Match`.
/// Kept as text; only normalization is offered, no resolution.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodePath(pub String);

impl NodePath {
    pub fn is_absolute(&self) -> bool {
        todo!()
    }
    pub fn segments(&self) -> impl Iterator<Item = &str> {
        self.0.split('/').filter(|segment| !segment.is_empty())
    }
    pub fn join(&self, child: &str) -> NodePath {
        todo!()
    }
}
