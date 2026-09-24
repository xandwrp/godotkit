//! `animation list` (offline): read bytes → gdview::gltf::read_glb → animations → filter → emit.
//! `animation inspect`: workspace → engine → resolve scene arg (cwd-relative OS path or res://)
//! → gdproject::animation::inspect → emit; Exit::Failed if any finding is an error.

use crate::cli::*;
use crate::context::Context;
use crate::render::Exit;

pub fn run(ctx: &Context, command: AnimationCommand) -> gdproject::Result<Exit> {
    todo!()
}
