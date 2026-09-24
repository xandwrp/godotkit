//! `project.godot` as data. Sections, keys, and raw values, with typed getters
//! for the keys an agent guesses wrong without an editor: input actions, layer
//! names, main scene, window, autoloads, warnings.
//!
//! # Tests (tests/settings.rs)
//! - `parses_sections_keys_and_quoted_values`
//! - `get_is_section_scoped_not_prefix_matched` (regression: `other/gdscript/warnings/enable`)
//! - `main_scene_reads_application_run_main_scene_as_res_or_uid`
//! - `autoloads_preserve_declaration_order_and_singleton_marker`
//! - `warnings_reports_defaults_when_keys_absent`
//! - `input_actions_parse_deadzone_and_key_joypad_mouse_events`
//! - `layer_names_cover_2d_3d_render_physics_navigation_and_avoidance`
//! - `window_reports_size_mode_and_stretch_with_godot_defaults`
//! - `config_version_and_features_are_typed`
//! - `malformed_lines_produce_line_numbered_parse_errors`

use std::collections::BTreeMap;

use serde::Serialize;

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

    /// `[input]` actions with their events. Godot's built-in `ui_*` actions are
    /// included with `builtin: true` so an agent sees the full set it may reference.
    pub fn input_actions(&self) -> crate::Result<Vec<InputAction>> {
        todo!()
    }

    /// `[layer_names]` for every layer kind, index -> name.
    pub fn layer_names(&self) -> LayerNames {
        todo!()
    }

    /// `[display]` window settings with Godot defaults filled in.
    pub fn window(&self) -> WindowSettings {
        todo!()
    }

    /// `application/config/features` entries.
    pub fn features(&self) -> Vec<String> {
        todo!()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum MainScene {
    Path(ResPath),
    Uid(crate::respath::Uid),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WarningPolicy {
    pub enabled: bool,
    pub exclude_addons: bool,
    /// Every `gdscript/warnings/<name>` override beyond the two above.
    pub overrides: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct InputAction {
    pub name: String,
    pub builtin: bool,
    pub deadzone: f64,
    pub events: Vec<InputEvent>,
}

/// Events as written in `project.godot`, decoded enough to be readable.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum InputEvent {
    Key { keycode: Option<String>, physical_keycode: Option<String>, modifiers: Vec<String> },
    MouseButton { button: String },
    JoypadButton { device: i64, button: String },
    JoypadMotion { device: i64, axis: String, axis_value: f64 },
    Other { type_name: String },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct LayerNames {
    pub render_2d: BTreeMap<u32, String>,
    pub physics_2d: BTreeMap<u32, String>,
    pub navigation_2d: BTreeMap<u32, String>,
    pub render_3d: BTreeMap<u32, String>,
    pub physics_3d: BTreeMap<u32, String>,
    pub navigation_3d: BTreeMap<u32, String>,
    pub avoidance: BTreeMap<u32, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WindowSettings {
    pub width: u32,
    pub height: u32,
    pub mode: String,
    pub stretch_mode: String,
    pub stretch_aspect: String,
    pub resizable: bool,
}
