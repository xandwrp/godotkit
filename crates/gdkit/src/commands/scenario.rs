//! `scenario start|status|disconnect|crash|stop`. Each maps 1:1 to gdproject::scenario.
//! `start` exits Failed when the run ends in `Failed`; the report carries participant logs.

use crate::cli::*;
use crate::context::Context;
use crate::render::Exit;

pub fn run(ctx: &Context, command: ScenarioCommand) -> gdproject::Result<Exit> {
    todo!()
}
