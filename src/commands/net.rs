//! Offline multiplayer observations. No workspace, engine selection, or cache.

use crate::cli::NetArgs;
use crate::context::Context;
use crate::render::{self, Exit};

mod human;

pub fn run(ctx: &Context, args: NetArgs) -> gdproject::Result<Exit> {
    let project = gdview::Project::discover(&args.project.project)?;
    let report = gdview::net::analyze_project(&project)?;
    if let Some(query) = args.explain {
        if query.trim().is_empty() {
            return Err(gdproject::Error::Invalid(
                "net --explain requires a non-empty query".into(),
            ));
        }
        let explanation = gdview::net::explain(&report, &query);
        let exit = if explanation.matched {
            Exit::Ok
        } else {
            Exit::Failed
        };
        render::write(ctx.output, &human::Explanation(explanation))?;
        Ok(exit)
    } else {
        render::write(ctx.output, &human::Report(report))?;
        Ok(Exit::Ok)
    }
}
