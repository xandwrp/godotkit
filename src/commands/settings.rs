//! `settings` (offline): Project::discover → settings() → the typed getter for the subcommand → emit. `main-scene` resolves a uid through UidMap.
//! `get` and `main-scene` exit 1 when the value is unset.

use std::collections::BTreeMap;
use std::io::Write;

use gdview::ResPath;
use gdview::settings::{InputAction, InputEvent, LayerNames, MainScene, WindowSettings};
use gdview::similar::similar;
use gdview::uid::UidMap;
use serde::Serialize;

use crate::cli::*;
use crate::context::Context;
use crate::render::{self, Exit, Human};

pub fn run(ctx: &Context, project: ProjectArgs, what: SettingsCommand) -> gdproject::Result<Exit> {
    let project = gdview::Project::discover(&project.project)?;
    let settings = project.settings()?;
    let output = ctx.output;
    match what {
        SettingsCommand::Input => {
            let actions = settings.input_actions()?;
            render::write(output, &Input { actions })?;
        }
        SettingsCommand::Layers => render::write(output, &Layers(settings.layer_names()?))?,
        SettingsCommand::Window => render::write(output, &Window(settings.window()?))?,
        SettingsCommand::MainScene => {
            let written = settings.main_scene()?;
            let path = match &written {
                Some(MainScene::Path(path)) => Some(path.clone()),
                Some(MainScene::Uid(uid)) => UidMap::build(&project)?.resolve(uid).cloned(),
                None => None,
            };
            let report = Main {
                exists: path.as_ref().is_some_and(|path| project.exists(path)),
                written,
                path,
            };
            render::write(output, &report)?;
            return Ok(if report.exists {
                Exit::Ok
            } else {
                Exit::Failed
            });
        }
        SettingsCommand::Get { section, key } => {
            let value = settings.get(&section, &key).map(str::to_owned);
            let found = value.is_some();
            let suggestions = match found {
                true => Vec::new(),
                false => {
                    let keys = settings.sections.get(&section).into_iter().flatten();
                    similar(&key, keys.map(|entry| entry.key.as_str()), 3)
                }
            };
            let report = Get {
                section,
                key,
                value,
                suggestions,
            };
            render::write(output, &report)?;
            return Ok(if found { Exit::Ok } else { Exit::Failed });
        }
    }
    Ok(Exit::Ok)
}

#[derive(Serialize)]
struct Input {
    actions: Vec<InputAction>,
}

impl Human for Input {
    fn human(&self, out: &mut dyn Write) -> std::io::Result<()> {
        let (listed, builtin): (Vec<_>, Vec<_>) = self.actions.iter().partition(|a| a.in_project);
        let width = listed.iter().map(|a| a.name.len()).max().unwrap_or(0);
        for action in &listed {
            let events: Vec<String> = action.events.iter().map(event).collect();
            let mut line = format!("{:width$}  {}", action.name, events.join(", "));
            if action.events.is_empty() {
                line.push_str("(no events)");
            }
            if action.deadzone != 0.2 {
                line.push_str(&format!("  [deadzone {}]", action.deadzone));
            }
            if action.builtin {
                line.push_str("  [overrides the built-in]");
            }
            writeln!(out, "{}", line.trim_end())?;
        }
        let names: Vec<&str> = builtin.iter().map(|a| a.name.as_str()).collect();
        if !listed.is_empty() {
            writeln!(out)?;
        }
        let heading = match listed.iter().any(|a| a.builtin) {
            true => "other built-in actions",
            false => "built-in actions",
        };
        writeln!(
            out,
            "{heading} ({}; --output json lists their events):",
            names.len()
        )?;
        let mut line = String::new();
        for name in names {
            if !line.is_empty() && line.len() + 1 + name.len() > 96 {
                writeln!(out, "  {line}")?;
                line.clear();
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(name);
        }
        writeln!(out, "  {line}")
    }
}

fn event(event: &InputEvent) -> String {
    match event {
        InputEvent::Key {
            keycode,
            physical_keycode,
            key_label,
            modifiers,
        } => {
            let key = match (keycode, physical_keycode, key_label) {
                (Some(key), _, _) => key.clone(),
                (None, Some(key), _) => format!("{key} (physical)"),
                (None, None, Some(key)) => format!("{key} (label)"),
                (None, None, None) => "any key".to_owned(),
            };
            chord(modifiers, key)
        }
        InputEvent::MouseButton {
            button,
            modifiers,
            double_click,
        } => {
            let button = match double_click {
                true => format!("{button} (double-click)"),
                false => button.clone(),
            };
            chord(modifiers, button)
        }
        InputEvent::JoypadButton { device, button } => with_device(button.clone(), *device),
        InputEvent::JoypadMotion {
            device,
            axis,
            axis_value,
        } => with_device(format!("{axis} {axis_value:+}"), *device),
        InputEvent::Other { type_name } => type_name.clone(),
    }
}

fn chord(modifiers: &[String], key: String) -> String {
    modifiers
        .iter()
        .cloned()
        .chain([key])
        .collect::<Vec<_>>()
        .join("+")
}

fn with_device(text: String, device: i64) -> String {
    match device {
        -1 => text,
        device => format!("{text} (device {device})"),
    }
}

#[derive(Serialize)]
#[serde(transparent)]
struct Layers(LayerNames);

impl Human for Layers {
    fn human(&self, out: &mut dyn Write) -> std::io::Result<()> {
        let names = &self.0;
        let kinds: [(&str, &BTreeMap<u32, String>); 7] = [
            ("2d_render", &names.render_2d),
            ("2d_physics", &names.physics_2d),
            ("2d_navigation", &names.navigation_2d),
            ("3d_render", &names.render_3d),
            ("3d_physics", &names.physics_3d),
            ("3d_navigation", &names.navigation_3d),
            ("avoidance", &names.avoidance),
        ];
        let mut any = false;
        for (kind, layers) in kinds.into_iter().filter(|(_, layers)| !layers.is_empty()) {
            any = true;
            writeln!(out, "{kind}")?;
            let width = layers.values().map(String::len).max().unwrap_or(0);
            for (index, name) in layers {
                // The bit a collision_layer/collision_mask value sets for this layer.
                let value = 1u64 << (index - 1);
                writeln!(out, "  {index:>2}  {name:width$}  mask value {value}")?;
            }
        }
        if !any {
            writeln!(out, "no named layers")?;
        }
        Ok(())
    }
}

#[derive(Serialize)]
#[serde(transparent)]
struct Window(WindowSettings);

impl Human for Window {
    fn human(&self, out: &mut dyn Write) -> std::io::Result<()> {
        let w = &self.0;
        writeln!(out, "viewport        {}x{}", w.width, w.height)?;
        if w.width_override != 0 || w.height_override != 0 {
            writeln!(
                out,
                "window          {}x{}",
                w.width_override, w.height_override
            )?;
        }
        writeln!(out, "mode            {}", w.mode)?;
        writeln!(out, "resizable       {}", w.resizable)?;
        writeln!(out, "stretch mode    {}", w.stretch_mode)?;
        writeln!(out, "stretch aspect  {}", w.stretch_aspect)?;
        writeln!(
            out,
            "stretch scale   {} ({})",
            w.stretch_scale, w.stretch_scale_mode
        )
    }
}

#[derive(Serialize)]
struct Main {
    /// As written in project.godot.
    written: Option<MainScene>,
    /// The scene file, with a uid resolved.
    path: Option<ResPath>,
    exists: bool,
}

impl Human for Main {
    fn human(&self, out: &mut dyn Write) -> std::io::Result<()> {
        match (&self.written, &self.path) {
            (None, _) => writeln!(out, "no main scene set (application/run/main_scene)"),
            (Some(MainScene::Uid(uid)), None) => {
                writeln!(out, "{}: no project file claims this uid", uid.0)
            }
            (Some(written), Some(path)) => {
                write!(out, "{path}")?;
                if let MainScene::Uid(uid) = written {
                    write!(out, "  (from {})", uid.0)?;
                }
                if !self.exists {
                    write!(out, "  (missing)")?;
                }
                writeln!(out)
            }
            (Some(MainScene::Path(_)), None) => unreachable!("a path is its own resolution"),
        }
    }
}

#[derive(Serialize)]
struct Get {
    section: String,
    key: String,
    /// Raw text as written, quotes included.
    value: Option<String>,
    /// Close keys in the same section, when it is not set.
    suggestions: Vec<String>,
}

impl Human for Get {
    fn human(&self, out: &mut dyn Write) -> std::io::Result<()> {
        let Some(value) = &self.value else {
            // stdout stays empty so `$(gdkit settings get …)` is the value or nothing.
            eprint!("{}/{} is not set in project.godot", self.section, self.key);
            match self.suggestions.as_slice() {
                [] => eprintln!(),
                close => eprintln!("; did you mean {}?", close.join(", ")),
            }
            return Ok(());
        };
        writeln!(out, "{value}")
    }
}
