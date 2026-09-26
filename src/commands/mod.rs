//! One file per top-level command. Each exposes `run(ctx, args) -> gdproject::Result<Exit>`.

use crate::cli::Command;
use crate::context::Context;
use crate::render::Exit;

pub mod api;
pub mod autoloads;
pub mod check;
pub mod config;
pub mod doctor;
pub mod import;
pub mod init;
pub mod net;
pub mod refs;
pub mod resource;
pub mod run;
pub mod scene_tree;
pub mod settings;

pub fn dispatch(ctx: &Context, command: Command) -> gdproject::Result<Exit> {
    match command {
        Command::Init { project } => init::run(ctx, project),
        Command::Config { command } => config::run(ctx, command),
        Command::Doctor { project } => doctor::run(ctx, project),
        Command::Check(args) => check::run(ctx, args),
        Command::Api(args) => api::run(ctx, args),
        Command::Refs { project, path } => refs::run(ctx, project, path),
        Command::Settings { project, what } => settings::run(ctx, project, what),
        Command::Resource { command } => resource::run(ctx, command),
        Command::SceneTree(args) => scene_tree::run(ctx, args),
        Command::Autoloads { project } => autoloads::run(ctx, project),
        Command::Net(args) => net::run(ctx, args),
        Command::Import { project } => import::run(ctx, project),
        Command::Run(args) => run::run(ctx, args),
    }
}
