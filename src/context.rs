//! Everything a command needs that isn't in its args: env, output mode, and
//! the two resolution steps every engine-backed command performs.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gdproject::global::GlobalConfig;
use gdproject::{Engine, Workspace};

use crate::cli::{Output, ProjectArgs};

pub struct Context {
    pub output: Output,
    pub env_godot: Option<OsString>,
    /// `None` when neither `GDKIT_CONFIG_DIR` nor a home/config directory is set.
    pub global_config_path: Option<PathBuf>,
    pub probe_deadline: Duration,
}

impl Context {
    pub fn from_env(output: Output) -> Self {
        Self {
            output,
            env_godot: std::env::var_os("GDKIT_GODOT"),
            global_config_path: GlobalConfig::locate(|name| std::env::var_os(name)),
            probe_deadline: gdproject::engine::DEFAULT_PROBE_DEADLINE,
        }
    }

    /// `Project::discover(args.project)` then `Workspace::open`. Both resolve the
    /// canonical root, so `--project ../proj` and symlinked checkouts/ancestors
    /// work and the report's `project.root` is the canonical path.
    pub fn workspace(&self, args: &ProjectArgs) -> gdproject::Result<Workspace> {
        let project = gdview::Project::discover(&args.project)?;
        Workspace::open(project.root())
    }

    /// `Config::load` → `select_engine(flag, env, config, global)` → `Engine::attach` (cached probe).
    /// Prints `engine: <path> (<version>)` to stderr in human mode.
    pub fn engine(&self, workspace: &Workspace, args: &ProjectArgs) -> gdproject::Result<Engine> {
        let config = gdproject::config::Config::load(workspace.root())?;
        let selection = gdproject::config::select_engine(
            workspace.root(),
            args.godot.as_deref(),
            self.env_godot.as_ref(),
            config.as_ref(),
            self.global_config()?.as_ref(),
        )?;
        let (engine, _) = Engine::attach(&selection, workspace, self.probe_deadline)?;
        self.announce_engine(&engine);
        Ok(engine)
    }

    /// Engine without a project, for `api --dump` outside one.
    pub fn standalone_engine(&self, explicit: Option<&Path>) -> gdproject::Result<Engine> {
        let selection = gdproject::config::select_engine(
            Path::new("."),
            explicit,
            self.env_godot.as_ref(),
            None,
            self.global_config()?.as_ref(),
        )?;
        let engine = Engine::attach_standalone(&selection, self.probe_deadline)?;
        self.announce_engine(&engine);
        Ok(engine)
    }

    /// The global config file, whether or not it exists yet.
    pub fn global_config_path(&self) -> gdproject::Result<&Path> {
        self.global_config_path.as_deref().ok_or_else(|| {
            gdproject::Error::Invalid(format!(
                "cannot locate the global config; set {} to a directory",
                gdproject::global::CONFIG_DIR_ENV
            ))
        })
    }

    /// The global config, or `None` when it is absent or cannot be located.
    pub fn global_config(&self) -> gdproject::Result<Option<GlobalConfig>> {
        match &self.global_config_path {
            Some(path) => GlobalConfig::load(path),
            None => Ok(None),
        }
    }

    fn announce_engine(&self, engine: &Engine) {
        if !self.json() {
            eprintln!(
                "engine: {} ({})",
                engine.executable.display(),
                engine.version
            );
        }
    }

    pub fn json(&self) -> bool {
        self.output == Output::Json
    }
}
