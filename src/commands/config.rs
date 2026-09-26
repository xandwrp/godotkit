//! `config` (machine-wide): locate the global config → get | set (probe the engine through `standalone_engine`, then `GlobalConfig::set_engine`) | unset | list → emit. `get` exits 1 when the key is unset.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use gdproject::Engine;
use gdproject::global::GlobalConfig;
use serde::Serialize;

use crate::cli::{ConfigCommand, ConfigKey};
use crate::context::Context;
use crate::render::{self, Exit, Human};

pub fn run(ctx: &Context, command: ConfigCommand) -> gdproject::Result<Exit> {
    let path = ctx.global_config_path()?;
    match command {
        ConfigCommand::Get { key } => {
            let value = value(ctx.global_config()?.as_ref(), key);
            let exit = if value.is_some() {
                Exit::Ok
            } else {
                Exit::Failed
            };
            emit(
                ctx,
                &Get {
                    path,
                    key: name(key),
                    value,
                },
            )?;
            Ok(exit)
        }
        ConfigCommand::Set { key, value } => {
            let engine = match key {
                ConfigKey::Godot => ctx.standalone_engine(Some(&value))?,
            };
            let value = GlobalConfig::set_engine(path, &value)?;
            emit(
                ctx,
                &Set {
                    path,
                    key: name(key),
                    value,
                    engine,
                },
            )?;
            Ok(Exit::Ok)
        }
        ConfigCommand::Unset { key } => {
            let removed = match key {
                ConfigKey::Godot => GlobalConfig::unset_engine(path)?,
            };
            emit(
                ctx,
                &Unset {
                    path,
                    key: name(key),
                    removed,
                },
            )?;
            Ok(Exit::Ok)
        }
        ConfigCommand::List => {
            let config = ctx.global_config()?;
            let values = [ConfigKey::Godot]
                .into_iter()
                .map(|key| (name(key), value(config.as_ref(), key)))
                .collect();
            emit(ctx, &List { path, values })?;
            Ok(Exit::Ok)
        }
    }
}

fn name(key: ConfigKey) -> &'static str {
    match key {
        ConfigKey::Godot => "godot",
    }
}

/// The value as stored in the file, not resolved.
fn value(config: Option<&GlobalConfig>, key: ConfigKey) -> Option<String> {
    match key {
        ConfigKey::Godot => config
            .and_then(|config| config.engine.as_ref())
            .map(|engine| engine.executable.display().to_string()),
    }
}

fn emit<T: Serialize + Human>(ctx: &Context, report: &T) -> gdproject::Result<()> {
    render::emit(ctx.output, report).map_err(|source| gdproject::Error::Io {
        path: "<stdout>".into(),
        source,
    })
}

#[derive(Serialize)]
struct Get<'a> {
    path: &'a Path,
    key: &'static str,
    value: Option<String>,
}

#[derive(Serialize)]
struct Set<'a> {
    path: &'a Path,
    key: &'static str,
    value: String,
    engine: Engine,
}

#[derive(Serialize)]
struct Unset<'a> {
    path: &'a Path,
    key: &'static str,
    removed: bool,
}

#[derive(Serialize)]
struct List<'a> {
    path: &'a Path,
    values: BTreeMap<&'static str, Option<String>>,
}

impl Human for Get<'_> {
    fn human(&self, out: &mut dyn Write) -> std::io::Result<()> {
        match &self.value {
            Some(value) => writeln!(out, "{value}"),
            None => Ok(()),
        }
    }
}

impl Human for Set<'_> {
    fn human(&self, out: &mut dyn Write) -> std::io::Result<()> {
        writeln!(out, "{} = {}", self.key, self.value)?;
        writeln!(out, "saved to {}", self.path.display())
    }
}

impl Human for Unset<'_> {
    fn human(&self, out: &mut dyn Write) -> std::io::Result<()> {
        if self.removed {
            writeln!(out, "removed {} from {}", self.key, self.path.display())
        } else {
            writeln!(out, "{} was not set in {}", self.key, self.path.display())
        }
    }
}

impl Human for List<'_> {
    fn human(&self, out: &mut dyn Write) -> std::io::Result<()> {
        writeln!(out, "file: {}", self.path.display())?;
        for (key, value) in &self.values {
            writeln!(out, "{key} = {}", value.as_deref().unwrap_or("(not set)"))?;
        }
        Ok(())
    }
}
