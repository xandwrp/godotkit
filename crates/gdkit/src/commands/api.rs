//! `api`: `--dump` → load_native (or standalone) and print the ApiIndex JSON. `search <term>` → search. One arg → lookup_class, falling back to lookup_global (utility function / global enum). Two args → lookup_member. A miss is Exit::Failed with suggestions in the JSON.

use crate::cli::*;
use crate::context::Context;
use crate::render::Exit;

pub fn run(ctx: &Context, args: ApiArgs) -> gdproject::Result<Exit> {
    todo!()
}
