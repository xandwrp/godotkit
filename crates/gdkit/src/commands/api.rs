//! `api`: `--dump` → load_native (or standalone) and print the ApiIndex JSON. Otherwise ProjectApi::load then lookup_class / lookup_member / search; a missing class or member is Exit::Failed with suggestions.

use crate::cli::*;
use crate::context::Context;
use crate::render::Exit;

pub fn run(ctx: &Context, args: ApiArgs) -> gdproject::Result<Exit> {
    todo!()
}
