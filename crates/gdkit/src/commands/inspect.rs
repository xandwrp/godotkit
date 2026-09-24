//! `inspect <session> --net | --checkpoints [--compare <session>]`.
//! resolve session(s) → require Running with a probe endpoint → gdproject::probe::observe_network
//! or collect_checkpoints (adapter from config; NotConfigured is Exit::Failed) → compare → emit.

use crate::cli::*;
use crate::context::Context;
use crate::render::Exit;

pub fn run(ctx: &Context, args: InspectArgs) -> gdproject::Result<Exit> {
    todo!()
}
