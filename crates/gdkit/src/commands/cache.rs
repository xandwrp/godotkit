//! `cache status` (offline) / `refresh` / `rebuild` / `clean [--dry-run]`.
//! Engine-backed variants take the workspace lock inside gdproject.

use crate::cli::*;
use crate::context::Context;
use crate::render::Exit;

pub fn run(ctx: &Context, command: CacheCommand) -> gdproject::Result<Exit> {
    todo!()
}
