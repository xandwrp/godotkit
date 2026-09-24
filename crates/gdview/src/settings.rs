//! `project.godot` as data. Sections, keys, and raw values, with typed getters
//! for the handful of keys gdkit cares about.
//!
//! # Tests (tests/settings.rs)
//! - `parses_sections_keys_and_quoted_values`
//! - `get_is_section_scoped_not_prefix_matched` (regression: `other/gdscript/warnings/enable`)
//! - `main_scene_reads_application_run_main_scene_as_res_or_uid`
//! - `autoloads_preserve_declaration_order_and_singleton_marker`
//! - `warnings_reports_defaults_when_keys_absent`
//! - `config_version_and_features_are_typed`
//! - `malformed_lines_produce_line_numbered_parse_errors`

use std::collections::BTreeMap;

use crate::autoload::Autoloads;
use crate::respath::ResPath;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    /// `section -> key -> raw value text`. The root section is `""`.
    pub sections: BTreeMap<String, BTreeMap<String, String>>,
}

impl Settings {
    pub fn parse(source: &str) -> crate::Result<Self> {
        todo!()
    }

    /// Raw value text for `section/key`, e.g. `("debug", "gdscript/warnings/enable")`.
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        todo!()
    }

    pub fn config_version(&self) -> Option<u32> {
        todo!()
    }

    /// `application/run/main_scene`, either a `res://` path or a `uid://`.
    pub fn main_scene(&self) -> Option<MainScene> {
        todo!()
    }

    pub fn autoloads(&self) -> crate::Result<Autoloads> {
        todo!()
    }

    /// The GDScript warning policy as the engine will apply it.
    pub fn warnings(&self) -> WarningPolicy {
        todo!()
    }

    /// `application/config/features` entries.
    pub fn features(&self) -> Vec<String> {
        todo!()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MainScene {
    Path(ResPath),
    Uid(crate::respath::Uid),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WarningPolicy {
    pub enabled: bool,
    pub exclude_addons: bool,
    /// Every `gdscript/warnings/<name>` override beyond the two above.
    pub overrides: BTreeMap<String, String>,
}
