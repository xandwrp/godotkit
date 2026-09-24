//! One file per top-level command. Each exposes `run(ctx, args) -> gdproject::Result<Exit>`.

use crate::cli::Command;
use crate::context::Context;
use crate::render::Exit;

pub mod animation;
pub mod api;
pub mod autoloads;
pub mod cache;
pub mod check;
pub mod doctor;
pub mod init;
pub mod inspect;
pub mod net;
pub mod resource;
pub mod scenario;
pub mod scene_tree;
pub mod sessions;

pub fn dispatch(ctx: &Context, command: Command) -> gdproject::Result<Exit> {
    match command {
        Command::Init { project } => init::run(ctx, project),
        Command::Doctor { project } => doctor::run(ctx, project),
        Command::Check(args) => check::run(ctx, args),
        Command::Api(args) => api::run(ctx, args),
        Command::Resource { command } => resource::run(ctx, command),
        Command::Animation { command } => animation::run(ctx, command),
        Command::SceneTree(args) => scene_tree::run(ctx, args),
        Command::Autoloads { project } => autoloads::run(ctx, project),
        Command::Net(args) => net::run(ctx, args),
        Command::Cache { command } => cache::run(ctx, command),
        Command::Run(args) => sessions::launch(ctx, args),
        Command::Sessions { project, all } => sessions::list(ctx, project, all),
        Command::Logs { project, session } => sessions::logs(ctx, project, session),
        Command::Stop { project, session } => sessions::stop(ctx, project, session),
        Command::Restart { project, session } => sessions::restart(ctx, project, session),
        Command::Inspect(args) => inspect::run(ctx, args),
        Command::Scenario { command } => scenario::run(ctx, command),
    }
}
