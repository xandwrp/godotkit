//! Autoload declarations in initialization order.
//!
//! # Tests (tests/settings.rs covers parsing; tests/autoload.rs)
//! - `display_lists_zero_based_order_kind_and_path`
//! - `scripts_vs_scenes_are_distinguished_by_extension`

use std::fmt;

use serde::Serialize;

use crate::respath::{ResPath, Uid};
use crate::uid::UidMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Autoload {
    pub name: String,
    pub target: AutoloadTarget,
    /// `*` prefix in project.godot: registered as a global singleton.
    pub singleton: bool,
}

/// What an autoload loads: Godot writes a `res://` path, or since 4.4 often a `uid://`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum AutoloadTarget {
    Path(ResPath),
    Uid(Uid),
}

impl Autoload {
    /// The loaded file, resolving a `uid://` through `uids`.
    pub fn path<'a>(&'a self, uids: &'a UidMap) -> Option<&'a ResPath> {
        match &self.target {
            AutoloadTarget::Path(path) => Some(path),
            AutoloadTarget::Uid(uid) => uids.resolve(uid),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Autoloads(pub Vec<Autoload>);

impl Autoloads {
    pub fn iter(&self) -> impl Iterator<Item = &Autoload> {
        self.0.iter()
    }
    pub fn by_name(&self, name: &str) -> Option<&Autoload> {
        self.0.iter().find(|autoload| autoload.name == name)
    }
}

impl fmt::Display for Autoloads {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}
