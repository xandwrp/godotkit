//! `scene-tree` (offline): read file → gdview::scene::parse → optionally expand with Project::discover(path) as SceneSource → compact_tree or JSON of ExpandedScene.

use crate::cli::*;
use crate::context::Context;
use crate::render::Exit;

pub fn run(ctx: &Context, args: SceneTreeArgs) -> gdproject::Result<Exit> {
    todo!()
}
