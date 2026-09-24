//! `run`: workspace → engine → RunRequest from args (`--until /ptr=value` split on the first `=`, value parsed as JSON then as string) → gdproject::run::run → emit RunReport → Exit from report.verdict.

use crate::cli::*;
use crate::context::Context;
use crate::render::Exit;

pub fn run(ctx: &Context, args: RunArgs) -> gdproject::Result<Exit> {
    todo!()
}
