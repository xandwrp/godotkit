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

use crate::autoload::{AutoloadTarget, Autoloads};
use crate::respath::ResPath;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    /// `section -> [(key, raw value text)]` in file order. The root section is `""`.
    pub sections: BTreeMap<String, Vec<(String, String)>>,
}

impl Settings {
    /// Godot's `ConfigFile` text: `[section]` headers, `key=value` lines whose
    /// value may span lines while a bracket or string is open, and `;`/`#`
    /// comment lines. Values are kept as written (trimmed).
    pub fn parse(source: &str) -> crate::Result<Self> {
        let mut settings = Settings::default();
        let mut section = String::new();
        let mut lines = source.lines().enumerate();
        while let Some((index, line)) = lines.next() {
            let number = index + 1;
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('#') {
                continue;
            }
            if let Some(header) = trimmed.strip_prefix('[') {
                let name = header
                    .strip_suffix(']')
                    .ok_or_else(|| parse_error(number, "unterminated section header"))?;
                section = name.trim().to_owned();
                settings.sections.entry(section.clone()).or_default();
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                return Err(parse_error(
                    number,
                    format!("expected `key=value`, found `{trimmed}`"),
                ));
            };
            let key = key.trim();
            if key.is_empty() {
                return Err(parse_error(number, "empty key"));
            }
            let mut value = value.to_owned();
            let mut scan = ValueScan::default();
            scan.feed(&value);
            while scan.open() {
                let Some((_, next)) = lines.next() else {
                    return Err(parse_error(
                        number,
                        format!("value of `{key}` is never closed"),
                    ));
                };
                value.push('\n');
                value.push_str(next);
                scan.feed("\n");
                scan.feed(next);
            }
            settings
                .sections
                .entry(section.clone())
                .or_default()
                .push((key.to_owned(), value.trim().to_owned()));
        }
        Ok(settings)
    }

    /// Raw value text for `section/key`, e.g. `("debug", "gdscript/warnings/enable")`.
    /// A key repeated in its section answers with its last value, as Godot reads it.
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.sections
            .get(section)?
            .iter()
            .rev()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    pub fn config_version(&self) -> Option<u32> {
        todo!()
    }

    /// `application/run/main_scene`, either a `res://` path or a `uid://`.
    pub fn main_scene(&self) -> Option<MainScene> {
        todo!()
    }

    /// `[autoload]` in declaration order. `*` marks a global singleton; targets
    /// are `res://` paths or `uid://`s (see [`crate::autoload::Autoload::path`]).
    pub fn autoloads(&self) -> crate::Result<Autoloads> {
        let entries = self
            .sections
            .get("autoload")
            .map(Vec::as_slice)
            .unwrap_or_default();
        let mut autoloads = Vec::with_capacity(entries.len());
        for (name, value) in entries {
            let text = unquote(value).ok_or_else(|| {
                crate::Error::InvalidResPath(format!("autoload {name} = {value}"))
            })?;
            let (singleton, target) = match text.strip_prefix('*') {
                Some(target) => (true, target),
                None => (false, text.as_str()),
            };
            let target = if target.starts_with("uid://") {
                AutoloadTarget::Uid(crate::respath::Uid(target.to_owned()))
            } else {
                AutoloadTarget::Path(ResPath::parse(target)?)
            };
            autoloads.push(crate::autoload::Autoload {
                name: name.clone(),
                target,
                singleton,
            });
        }
        Ok(Autoloads(autoloads))
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
    Key {
        keycode: Option<String>,
        physical_keycode: Option<String>,
        modifiers: Vec<String>,
    },
    MouseButton {
        button: String,
    },
    JoypadButton {
        device: i64,
        button: String,
    },
    JoypadMotion {
        device: i64,
        axis: String,
        axis_value: f64,
    },
    Other {
        type_name: String,
    },
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

fn parse_error(line: usize, message: impl Into<String>) -> crate::Error {
    crate::Error::Parse {
        path: None,
        line,
        message: message.into(),
    }
}

/// A quoted string value's contents, with `\"` and `\\` unescaped.
fn unquote(value: &str) -> Option<String> {
    let inner = value.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            out.push(chars.next()?);
        } else {
            out.push(ch);
        }
    }
    Some(out)
}

/// Tracks whether a value is still open: an unclosed string, or unbalanced
/// `(`, `[`, `{` outside strings.
#[derive(Default)]
struct ValueScan {
    depth: usize,
    in_string: bool,
    escaped: bool,
}

impl ValueScan {
    fn feed(&mut self, text: &str) {
        for ch in text.chars() {
            if self.in_string {
                match (self.escaped, ch) {
                    (true, _) => self.escaped = false,
                    (false, '\\') => self.escaped = true,
                    (false, '"') => self.in_string = false,
                    _ => {}
                }
                continue;
            }
            match ch {
                '"' => self.in_string = true,
                '(' | '[' | '{' => self.depth += 1,
                ')' | ']' | '}' => self.depth = self.depth.saturating_sub(1),
                _ => {}
            }
        }
    }

    fn open(&self) -> bool {
        self.in_string || self.depth > 0
    }
}
