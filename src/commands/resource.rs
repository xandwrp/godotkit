//! `resource schema`: workspace → engine → gdproject::resource::schema → emit.
//! `resource create`: read spec JSON → CreateSpec::from_json → workspace → engine
//! → gdproject::resource::create → emit CreateReport. Verify/publish failures are
//! `Err` (exit 2): nothing was written, and that is a tool outcome, not a project verdict.

use crate::cli::*;
use crate::context::Context;
use crate::render::Exit;

pub fn run(ctx: &Context, command: ResourceCommand) -> gdproject::Result<Exit> {
    todo!()
}
