//! `check`: workspace → engine → CheckRequest from args (+ config strict_methods) → gdproject::check::run with a stderr observer (silent in JSON mode) → emit report → Exit from report.outcome.

use crate::cli::*;
use crate::context::Context;
use crate::render::Exit;

pub fn run(ctx: &Context, args: CheckArgs) -> gdproject::Result<Exit> {
    todo!()
}
