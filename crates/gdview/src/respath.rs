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
    /// Parses and validates. `res://` prefix is required. `res://` alone is the root.
    pub fn parse(text: &str) -> crate::Result<Self> {
        let invalid = || crate::Error::InvalidResPath(text.to_owned());
        let relative = text.strip_prefix(PREFIX).ok_or_else(invalid)?;
        if relative.is_empty() {
            return Ok(Self(PREFIX.to_owned()));
        }
        if relative.contains('\\') || relative.contains('\0') {
            return Err(invalid());
        }
        let mut segments = relative.split('/');
        if segments.clone().any(|segment| matches!(segment, "" | "." | "..")) {
            return Err(invalid());
        }
        if segments.next() == Some(".godot") {
            return Err(invalid());
        }
        Ok(Self(text.to_owned()))
    }

    /// Builds from a project-relative path such as `scenes/main.tscn`.
    pub fn from_relative(relative: &str) -> crate::Result<Self> {
        Self::parse(&format!("{PREFIX}{relative}"))
    }

    /// The root, `res://`.
    pub fn root() -> Self {
        Self(PREFIX.to_owned())
    }

    /// The path without the `res://` prefix. Never starts with `/`. Empty for the root.
    pub fn relative(&self) -> &str {
        &self.0[PREFIX.len()..]
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Text after the last `.` of the file name, as Godot's `get_extension`:
    /// `a.tscn` -> `tscn`, `a.png.import` -> `import`, `.hidden` -> `hidden`, `a` -> `None`.
    pub fn extension(&self) -> Option<&str> {
        let name = self.file_name();
        name.rfind('.').map(|dot| &name[dot + 1..])
    }

    /// Last segment. Empty for the root.
    pub fn file_name(&self) -> &str {
        let relative = self.relative();
        relative.rsplit('/').next().unwrap_or(relative)
    }

    /// `None` for the root.
    pub fn parent(&self) -> Option<ResPath> {
        let relative = self.relative();
        if relative.is_empty() {
            return None;
        }
        Some(match relative.rfind('/') {
            Some(slash) => Self(format!("{PREFIX}{}", &relative[..slash])),
            None => Self::root(),
        })
    }

    /// Appends one or more `/`-separated segments. `..`, `.`, and empty segments are rejected.
    pub fn join(&self, segment: &str) -> crate::Result<ResPath> {
        match self.relative() {
            "" => Self::from_relative(segment),
            relative => Self::from_relative(&format!("{relative}/{segment}")),
        }
    }

    /// True if `self` is `ancestor` or lies under it.
    pub fn starts_with(&self, ancestor: &ResPath) -> bool {
        let (path, prefix) = (self.relative(), ancestor.relative());
        prefix.is_empty() || path == prefix || path.strip_prefix(prefix).is_some_and(|rest| rest.starts_with('/'))
    }
}

const PREFIX: &str = "res://";

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
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NodePath(pub String);

impl NodePath {
    pub fn is_absolute(&self) -> bool {
        self.0.starts_with('/')
    }
    pub fn segments(&self) -> impl Iterator<Item = &str> {
        self.0.split('/').filter(|segment| !segment.is_empty())
    }
    /// `.` joined with `A` is `A`; `A` joined with `B` is `A/B`.
    pub fn join(&self, child: &str) -> NodePath {
        match self.0.as_str() {
            "" | "." => NodePath(child.to_owned()),
            parent => NodePath(format!("{}/{child}", parent.trim_end_matches('/'))),
        }
    }
}
