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
//! - `warnings_read_directory_rules_and_migrate_exclude_addons_like_the_engine`
//! - `malformed_warning_settings_are_errors_naming_the_key`
//! - `input_actions_parse_deadzone_and_key_joypad_mouse_events`
//! - `layer_names_cover_2d_3d_render_physics_navigation_and_avoidance`
//! - `window_reports_size_mode_and_stretch_with_godot_defaults`
//! - `config_version_and_features_are_typed`
//! - `real_engine_builtin_input_actions_match_the_table` (opt-in, GDKIT_TEST_GODOT)
//! - `malformed_lines_produce_line_numbered_parse_errors`

use std::collections::BTreeMap;

use serde::Serialize;

use crate::autoload::{AutoloadTarget, Autoloads};
use crate::respath::{ResPath, Uid};
use crate::scene::Value;

mod input_names;

/// Godot's built-in `ui_*` actions, as the engine writes them in `[input]`.
const BUILTIN_INPUT: &str = include_str!("settings/builtin_input.godot");

/// `InputMap`'s deadzone for a project action that does not set one.
const DEFAULT_DEADZONE: f64 = 0.2;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    /// `section -> entries` in file order. The root section is `""`.
    pub sections: BTreeMap<String, Vec<Entry>>,
}

/// One `key=value` line (or lines, for a value left open across them).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub key: String,
    /// Raw value text, trimmed.
    pub value: String,
    /// Where the key is written, 1-based.
    pub line: usize,
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
                .push(Entry {
                    key: key.to_owned(),
                    value: value.trim().to_owned(),
                    line: number,
                });
        }
        Ok(settings)
    }

    /// Raw value text for `section/key`, e.g. `("debug", "gdscript/warnings/enable")`.
    /// A key repeated in its section answers with its last value, as Godot reads it.
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.entry(section, key).map(|entry| entry.value.as_str())
    }

    /// The entry [`Settings::get`] answers from, with its line.
    pub fn entry(&self, section: &str, key: &str) -> Option<&Entry> {
        self.sections
            .get(section)?
            .iter()
            .rev()
            .find(|entry| entry.key == key)
    }

    /// Entries of `section` with the last value of each key, in first-appearance order.
    fn effective(&self, section: &str) -> Vec<&Entry> {
        let mut entries: Vec<&Entry> = Vec::new();
        for entry in self.sections.get(section).into_iter().flatten() {
            match entries.iter_mut().find(|seen| seen.key == entry.key) {
                Some(seen) => *seen = entry,
                None => entries.push(entry),
            }
        }
        entries
    }

    /// The root `config_version`: 5 for Godot 4 projects.
    pub fn config_version(&self) -> crate::Result<Option<u32>> {
        self.get("", "config_version")
            .map(|value| {
                value
                    .parse()
                    .map_err(|_| setting_error("config_version", value))
            })
            .transpose()
    }

    /// `application/run/main_scene`, either a `res://` path or a `uid://`.
    /// `None` when unset or empty, as Godot treats both.
    pub fn main_scene(&self) -> crate::Result<Option<MainScene>> {
        let Some(value) = self.get("application", "run/main_scene") else {
            return Ok(None);
        };
        let text =
            unquote(value).ok_or_else(|| setting_error("application/run/main_scene", value))?;
        if text.is_empty() {
            return Ok(None);
        }
        if text.starts_with("uid://") {
            return Ok(Some(MainScene::Uid(Uid(text))));
        }
        ResPath::parse(&text)
            .map(|path| Some(MainScene::Path(path)))
            .map_err(|_| setting_error("application/run/main_scene", value))
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
        for Entry {
            key: name, value, ..
        } in entries
        {
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

    /// The GDScript warning policy as Godot 4.7 loads it from `[debug]`.
    ///
    /// Without `directory_rules`, the default excludes `res://addons`. A legacy
    /// `exclude_addons` still wins for `res://addons`: the engine moves that
    /// rule to the front, whatever the line order. An explicit `{}` stays empty.
    pub fn warnings(&self) -> crate::Result<WarningPolicy> {
        const PREFIX: &str = "gdscript/warnings/";
        let key = |name: &str| format!("{PREFIX}{name}");
        let enabled = match self.get("debug", &key("enable")) {
            Some(value) => parse_bool(value).ok_or_else(|| debug_error(&key("enable"), value))?,
            None => true,
        };
        let mut directory_rules = match self.get("debug", &key("directory_rules")) {
            Some(value) => parse_directory_rules(value)
                .ok_or_else(|| debug_error(&key("directory_rules"), value))?,
            None => vec![DirectoryRule {
                path: "res://addons".into(),
                mode: DirectoryRuleMode::Exclude,
            }],
        };
        if let Some(value) = self.get("debug", &key("exclude_addons")) {
            let exclude =
                parse_bool(value).ok_or_else(|| debug_error(&key("exclude_addons"), value))?;
            directory_rules.retain(|rule| rule.path != "res://addons");
            directory_rules.insert(
                0,
                DirectoryRule {
                    path: "res://addons".into(),
                    mode: if exclude {
                        DirectoryRuleMode::Exclude
                    } else {
                        DirectoryRuleMode::Include
                    },
                },
            );
        }
        let overrides = self
            .sections
            .get("debug")
            .into_iter()
            .flatten()
            .filter_map(|entry| Some((entry.key.strip_prefix(PREFIX)?, &entry.value)))
            .filter(|(name, _)| !matches!(*name, "enable" | "directory_rules" | "exclude_addons"))
            .map(|(name, value)| (name.to_owned(), value.clone()))
            .collect();
        Ok(WarningPolicy {
            enabled,
            directory_rules,
            overrides,
        })
    }

    /// `[input]` actions with their events. Godot's built-in `ui_*` actions are
    /// included with `builtin: true` so an agent sees the full set it may reference.
    ///
    /// The project's actions come first, in file order; a project entry named like
    /// a built-in replaces it whole, as the engine does. The untouched built-ins
    /// follow, sorted by name.
    pub fn input_actions(&self) -> crate::Result<Vec<InputAction>> {
        let builtins = Settings::parse(BUILTIN_INPUT).expect("builtin_input.godot parses");
        let builtin_names: Vec<&str> = builtins
            .effective("input")
            .into_iter()
            .map(|entry| entry.key.as_str())
            .collect();
        let mut actions = Vec::new();
        for entry in self.effective("input") {
            let builtin = builtin_names.contains(&entry.key.as_str());
            actions.push(input_action(entry, builtin, true)?);
        }
        for entry in builtins.effective("input") {
            if actions.iter().all(|action| action.name != entry.key) {
                actions.push(input_action(entry, true, false)?);
            }
        }
        Ok(actions)
    }

    /// `[layer_names]` for every layer kind, index -> name. Keys that name no
    /// known layer kind are ignored.
    pub fn layer_names(&self) -> crate::Result<LayerNames> {
        let mut names = LayerNames::default();
        for entry in self.effective("layer_names") {
            let Some((kind, layer)) = entry.key.split_once('/') else {
                continue;
            };
            let map = match kind {
                "2d_render" => &mut names.render_2d,
                "2d_physics" => &mut names.physics_2d,
                "2d_navigation" => &mut names.navigation_2d,
                "3d_render" => &mut names.render_3d,
                "3d_physics" => &mut names.physics_3d,
                "3d_navigation" => &mut names.navigation_3d,
                "avoidance" => &mut names.avoidance,
                _ => continue,
            };
            let Some(index) = layer
                .strip_prefix("layer_")
                .and_then(|n| n.parse().ok())
                .filter(|index| (1..=32).contains(index))
            else {
                continue;
            };
            let key = format!("layer_names/{}", entry.key);
            let name = unquote(&entry.value).ok_or_else(|| setting_error(&key, &entry.value))?;
            if !name.is_empty() {
                map.insert(index, name);
            }
        }
        Ok(names)
    }

    /// `[display]` window settings with Godot 4.7's defaults filled in.
    pub fn window(&self) -> crate::Result<WindowSettings> {
        let read = |key: &str| {
            self.get("display", &format!("window/{key}"))
                .map(|value| (format!("display/window/{key}"), value))
        };
        let int = |key: &str, default: u32| -> crate::Result<u32> {
            read(key).map_or(Ok(default), |(key, value)| {
                value.parse().map_err(|_| setting_error(&key, value))
            })
        };
        let text = |key: &str, default: &str| -> crate::Result<String> {
            read(key).map_or(Ok(default.to_owned()), |(key, value)| {
                unquote(value).ok_or_else(|| setting_error(&key, value))
            })
        };
        let mode = match int("size/mode", 0)? {
            0 => "windowed",
            1 => "minimized",
            2 => "maximized",
            3 => "fullscreen",
            4 => "exclusive_fullscreen",
            _ => {
                let (key, value) = read("size/mode").expect("a non-default mode is written");
                return Err(setting_error(&key, value));
            }
        };
        let resizable = match read("size/resizable") {
            None => true,
            Some((key, value)) => parse_bool(value).ok_or_else(|| setting_error(&key, value))?,
        };
        let stretch_scale = match read("stretch/scale") {
            None => 1.0,
            Some((key, value)) => value.parse().map_err(|_| setting_error(&key, value))?,
        };
        Ok(WindowSettings {
            width: int("size/viewport_width", 1152)?,
            height: int("size/viewport_height", 648)?,
            width_override: int("size/window_width_override", 0)?,
            height_override: int("size/window_height_override", 0)?,
            mode: mode.to_owned(),
            resizable,
            stretch_mode: text("stretch/mode", "disabled")?,
            stretch_aspect: text("stretch/aspect", "keep")?,
            stretch_scale,
            stretch_scale_mode: text("stretch/scale_mode", "fractional")?,
        })
    }

    /// `application/config/features` entries.
    pub fn features(&self) -> crate::Result<Vec<String>> {
        const KEY: &str = "application/config/features";
        let Some(value) = self.get("application", "config/features") else {
            return Ok(Vec::new());
        };
        let invalid = || setting_error(KEY, value);
        let Ok(Value::Call { name, args }) = crate::scene::parse_value(value) else {
            return Err(invalid());
        };
        if name != "PackedStringArray" {
            return Err(invalid());
        }
        args.iter()
            .map(|arg| arg.as_str().map(str::to_owned).ok_or_else(invalid))
            .collect()
    }
}

/// One `[input]` entry: `{"deadzone": 0.2, "events": [Object(InputEventKey, …), …]}`.
fn input_action(entry: &Entry, builtin: bool, in_project: bool) -> crate::Result<InputAction> {
    let key = format!("input/{}", entry.key);
    let invalid = |message: &str| crate::Error::Setting {
        key: key.clone(),
        message: message.to_owned(),
    };
    let Ok(Value::Dict(fields)) = crate::scene::parse_value(&entry.value) else {
        return Err(invalid(
            "expected a dictionary with `deadzone` and `events`",
        ));
    };
    let field = |name: &str| {
        fields
            .iter()
            .rev()
            .find(|(key, _)| key.as_str() == Some(name))
            .map(|(_, value)| value)
    };
    let deadzone = match field("deadzone") {
        None => DEFAULT_DEADZONE,
        Some(value) => number(value).ok_or_else(|| invalid("`deadzone` is not a number"))?,
    };
    let events = match field("events") {
        None => Vec::new(),
        Some(Value::Array(events)) => events
            .iter()
            .map(|event| {
                input_event(event).ok_or_else(|| invalid("an event is not an `Object(…)`"))
            })
            .collect::<crate::Result<_>>()?,
        Some(_) => return Err(invalid("`events` is not an array")),
    };
    Ok(InputAction {
        name: entry.key.clone(),
        builtin,
        in_project,
        deadzone,
        events,
    })
}

fn input_event(value: &Value) -> Option<InputEvent> {
    let Value::Call { name, args } = value else {
        return None;
    };
    let (Some(Value::Str(class)), true) = (args.first(), name == "Object") else {
        return None;
    };
    let props: Vec<(&str, &Value)> = args[1..]
        .chunks(2)
        .filter_map(|pair| Some((pair.first()?.as_str()?, pair.get(1)?)))
        .collect();
    let get = |name: &str| {
        props
            .iter()
            .rev()
            .find(|(key, _)| *key == name)
            .map(|(_, v)| *v)
    };
    let int = |name: &str| match get(name) {
        Some(Value::Int(value)) => Some(*value),
        _ => None,
    };
    let flag = |name: &str| matches!(get(name), Some(Value::Bool(true)));
    let device = int("device").unwrap_or(-1);
    let modifiers = || {
        let mut modifiers = Vec::new();
        if flag("command_or_control_autoremap") {
            modifiers.push("command_or_control".to_owned());
        }
        for (key, name) in [
            ("ctrl_pressed", "ctrl"),
            ("shift_pressed", "shift"),
            ("alt_pressed", "alt"),
            ("meta_pressed", "meta"),
        ] {
            if flag(key) {
                modifiers.push(name.to_owned());
            }
        }
        modifiers
    };
    let named =
        |table: &[(i64, &str)], value: i64| match table.binary_search_by_key(&value, |(v, _)| *v) {
            Ok(index) => table[index].1.to_owned(),
            Err(_) => value.to_string(),
        };
    let key = |name: &str| {
        int(name)
            .filter(|code| *code != 0)
            .map(|code| named(input_names::KEYS, code))
    };
    Some(match class.as_str() {
        "InputEventKey" => InputEvent::Key {
            keycode: key("keycode"),
            physical_keycode: key("physical_keycode"),
            key_label: key("key_label"),
            modifiers: modifiers(),
        },
        "InputEventMouseButton" => InputEvent::MouseButton {
            button: named(input_names::MOUSE_BUTTONS, int("button_index").unwrap_or(0)),
            modifiers: modifiers(),
            double_click: flag("double_click"),
        },
        "InputEventJoypadButton" => InputEvent::JoypadButton {
            device,
            button: named(input_names::JOY_BUTTONS, int("button_index").unwrap_or(0)),
        },
        "InputEventJoypadMotion" => InputEvent::JoypadMotion {
            device,
            axis: named(input_names::JOY_AXES, int("axis").unwrap_or(0)),
            axis_value: get("axis_value").and_then(number).unwrap_or(0.0),
        },
        other => InputEvent::Other {
            type_name: other.to_owned(),
        },
    })
}

fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Int(value) => Some(*value as f64),
        Value::Float(value) => Some(*value),
        _ => None,
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
    /// In the engine's order.
    pub directory_rules: Vec<DirectoryRule>,
    /// Every other `gdscript/warnings/<name>`, raw. Levels are `0` ignore,
    /// `1` warn, `2` error; the last value of a repeated key wins.
    pub overrides: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DirectoryRule {
    pub path: String,
    pub mode: DirectoryRuleMode,
}

/// `0` and `1` in `directory_rules`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectoryRuleMode {
    Exclude,
    Include,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct InputAction {
    pub name: String,
    /// One of Godot's `ui_*` actions.
    pub builtin: bool,
    /// Declared in project.godot. With `builtin`, it replaces the engine's events.
    pub in_project: bool,
    pub deadzone: f64,
    pub events: Vec<InputEvent>,
}

/// Events as written in `project.godot`, decoded enough to be readable.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum InputEvent {
    /// Keys are `Key` constants (`KEY_SPACE`); a field is absent when the
    /// event does not match on it. `physical_keycode` is the key's position on
    /// a US QWERTY layout, whatever the user's layout.
    Key {
        keycode: Option<String>,
        physical_keycode: Option<String>,
        key_label: Option<String>,
        /// `command_or_control` (Ctrl, or Cmd on macOS), `ctrl`, `shift`, `alt`, `meta`.
        modifiers: Vec<String>,
    },
    MouseButton {
        button: String,
        modifiers: Vec<String>,
        double_click: bool,
    },
    /// `device` -1 matches every joypad.
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
    /// The viewport size the game is designed for.
    pub width: u32,
    pub height: u32,
    /// The window's initial size when set; `0` means the viewport size.
    pub width_override: u32,
    pub height_override: u32,
    /// `windowed`, `minimized`, `maximized`, `fullscreen`, `exclusive_fullscreen`.
    pub mode: String,
    pub resizable: bool,
    /// `disabled`, `canvas_items`, `viewport`.
    pub stretch_mode: String,
    /// `ignore`, `keep`, `keep_width`, `keep_height`, `expand`.
    pub stretch_aspect: String,
    pub stretch_scale: f64,
    /// `fractional` or `integer`.
    pub stretch_scale_mode: String,
}

fn parse_error(line: usize, message: impl Into<String>) -> crate::Error {
    crate::Error::Parse {
        path: None,
        line,
        message: message.into(),
    }
}

/// `key` is the full `section/key` path.
fn setting_error(key: &str, value: &str) -> crate::Error {
    crate::Error::Setting {
        key: key.to_owned(),
        message: format!("unexpected value `{value}`"),
    }
}

fn debug_error(key: &str, value: &str) -> crate::Error {
    setting_error(&format!("debug/{key}"), value)
}

fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// `{ "res://x": 0, … }`: quoted keys, `0` or `1` values, as the editor writes it.
fn parse_directory_rules(value: &str) -> Option<Vec<DirectoryRule>> {
    let mut rest = value.strip_prefix('{')?.strip_suffix('}')?.trim();
    let mut rules = Vec::new();
    while !rest.is_empty() {
        let (path, after) = split_quoted(rest)?;
        let after = after.trim_start().strip_prefix(':')?.trim_start();
        let end = after.find(',').unwrap_or(after.len());
        let mode = match after[..end].trim() {
            "0" => DirectoryRuleMode::Exclude,
            "1" => DirectoryRuleMode::Include,
            _ => return None,
        };
        rules.retain(|rule: &DirectoryRule| rule.path != path);
        rules.push(DirectoryRule { path, mode });
        rest = after[end..].strip_prefix(',').unwrap_or("").trim_start();
    }
    Some(rules)
}

/// A leading quoted string, unescaped, and the text after its closing quote.
fn split_quoted(text: &str) -> Option<(String, &str)> {
    let inner = text.strip_prefix('"')?;
    let mut escaped = false;
    for (index, ch) in inner.char_indices() {
        match (escaped, ch) {
            (true, _) => escaped = false,
            (false, '\\') => escaped = true,
            (false, '"') => {
                let quoted = &text[..index + 2];
                return Some((unquote(quoted)?, &inner[index + 1..]));
            }
            _ => {}
        }
    }
    None
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
