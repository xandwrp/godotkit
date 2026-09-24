//! `run` / `sessions` / `logs` / `stop` / `restart`.
//! `run`: workspace → engine → LaunchSpec → gdproject::session::launch → print record.
//! `logs`: resolve → stream the log file to stdout (no JSON form; it is raw text).
//! `stop`: resolve → session::stop(grace 5s) → Exit::Failed when NotFound.

use crate::cli::*;
use crate::context::Context;
use crate::render::Exit;

pub fn launch(ctx: &Context, args: RunArgs) -> gdproject::Result<Exit> {
    todo!()
}

pub fn list(ctx: &Context, project: ProjectArgs, all: bool) -> gdproject::Result<Exit> {
    todo!()
}

pub fn logs(ctx: &Context, project: ProjectArgs, session: String) -> gdproject::Result<Exit> {
    todo!()
}

pub fn stop(ctx: &Context, project: ProjectArgs, session: String) -> gdproject::Result<Exit> {
    todo!()
}

pub fn restart(ctx: &Context, project: ProjectArgs, session: String) -> gdproject::Result<Exit> {
    todo!()
}
