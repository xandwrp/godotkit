//! `refs` (offline): Project::discover → declarations + UidMap + ProjectGraph::load → gdview::xref::references_to(path) → emit. Exit::Failed when the path itself does not exist.

use crate::cli::*;
use crate::context::Context;
use crate::render::Exit;

pub fn run(ctx: &Context, project: ProjectArgs, path: String) -> gdproject::Result<Exit> {
    todo!()
}
