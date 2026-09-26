//! `init`: discover project → select engine (flag/env required, config must not exist) → Engine::attach (probe) → Config::write_initial. Prints the config path.

use crate::cli::*;
use crate::context::Context;
use crate::render::Exit;

pub fn run(ctx: &Context, args: ProjectArgs) -> gdproject::Result<Exit> {
    todo!()
}
