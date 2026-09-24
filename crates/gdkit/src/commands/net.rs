//! `net`: workspace → gdview declarations + parsed scenes + autoloads → gdview::net::analyze → unless --offline, engine + run_harness NetFacts → merge_engine_facts → optional explain → emit.

use crate::cli::*;
use crate::context::Context;
use crate::render::Exit;

pub fn run(ctx: &Context, args: NetArgs) -> gdproject::Result<Exit> {
    todo!()
}
