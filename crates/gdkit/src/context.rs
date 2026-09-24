//! Everything a command needs that isn't in its args: env, output mode, and
//! the two resolution steps every engine-backed command performs.

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use gdproject::{Engine, Workspace};

use crate::cli::{Output, ProjectArgs};

pub struct Context {
    pub output: Output,
    pub env_godot: Option<OsString>,
    pub probe_deadline: Duration,
}

impl Context {
    pub fn from_env(output: Output) -> Self {
        Self {
            output,
            env_godot: std::env::var_os("GDKIT_GODOT"),
            probe_deadline: gdproject::engine::DEFAULT_PROBE_DEADLINE,
        }
    }

    /// `Project::discover(args.project)` then `Workspace::open`.
    pub fn workspace(&self, args: &ProjectArgs) -> gdproject::Result<Workspace> {
        todo!()
    }

    /// `Config::load` → `select_engine(flag, env, config)` → `Engine::attach` (cached probe).
    /// Prints `engine: <path> (<version>)` to stderr in human mode.
    pub fn engine(&self, workspace: &Workspace, args: &ProjectArgs) -> gdproject::Result<Engine> {
        todo!()
    }

    /// Engine without a project, for `api --dump` outside one.
    pub fn standalone_engine(&self, explicit: Option<&Path>) -> gdproject::Result<Engine> {
        todo!()
    }

    pub fn json(&self) -> bool {
        self.output == Output::Json
    }
}
