//! `import`: workspace → engine → gdproject::cache::refresh (takes the workspace lock) → emit RefreshReport; Exit::Failed if it reported errors.

use crate::cli::*;
use crate::context::Context;
use crate::render::Exit;

pub fn run(ctx: &Context, project: ProjectArgs) -> gdproject::Result<Exit> {
    todo!()
}
