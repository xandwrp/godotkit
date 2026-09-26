//! `doctor`: workspace → config (errors reported, not fatal) → selection → probe cache health → attach → warning policy from Settings → api cache presence → last check artifact. Renders a DoctorReport (Serialize + Human).

use crate::cli::*;
use crate::context::Context;
use crate::render::Exit;

pub fn run(ctx: &Context, args: ProjectArgs) -> gdproject::Result<Exit> {
    todo!()
}
