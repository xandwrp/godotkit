//! `init`: workspace → refuse an existing gdkit.toml → `ctx.engine` (flag, env, or global default; probed and cached) → `Config::write_initial` when --godot pins one, else `Config::write_initial_unpinned`. Prints the config path and where the engine comes from.

use std::io::Write;
use std::path::PathBuf;

use gdproject::Engine;
use gdproject::config::{CONFIG_FILE_NAME, Config, SelectionSource};
use serde::Serialize;

use crate::cli::*;
use crate::context::Context;
use crate::render::{self, Exit, Human};

pub fn run(ctx: &Context, args: ProjectArgs) -> gdproject::Result<Exit> {
    let workspace = ctx.workspace(&args)?;
    let existing = workspace.root().join(CONFIG_FILE_NAME);
    // symlink_metadata so a dangling symlink also counts as existing.
    if std::fs::symlink_metadata(&existing).is_ok() {
        return Err(gdproject::Error::Invalid(format!(
            "{} already exists; edit its [engine] table to change the engine",
            existing.display()
        )));
    }
    // Probe before writing, so a wrong engine never leaves a config behind.
    let engine = ctx.engine(&workspace, &args)?;
    let config = match &args.godot {
        Some(godot) => Config::write_initial(workspace.root(), godot)?,
        None => Config::write_initial_unpinned(workspace.root())?,
    };
    let report = Init {
        config,
        pinned: args.godot.is_some(),
        engine,
    };
    render::emit(ctx.output, &report).map_err(|source| gdproject::Error::Io {
        path: "<stdout>".into(),
        source,
    })?;
    Ok(Exit::Ok)
}

#[derive(Serialize)]
struct Init {
    config: PathBuf,
    /// Whether gdkit.toml names the engine. When false, the engine is whatever
    /// `GDKIT_GODOT` or the global default resolves to at each run.
    pinned: bool,
    engine: Engine,
}

impl Human for Init {
    fn human(&self, out: &mut dyn Write) -> std::io::Result<()> {
        writeln!(out, "wrote {}", self.config.display())?;
        let engine = self.engine.executable.display();
        match (self.pinned, self.engine.source) {
            (true, _) => writeln!(out, "engine: pinned to {engine}"),
            (false, SelectionSource::Environment) => writeln!(
                out,
                "engine: not pinned; {engine} came from GDKIT_GODOT, and the global default applies without it"
            ),
            (false, _) => writeln!(
                out,
                "engine: follows the global default, currently {engine} (`gdkit config set godot <path>` changes it)"
            ),
        }
    }
}
