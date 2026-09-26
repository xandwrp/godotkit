//! `autoloads` (offline): Project::discover → settings().autoloads() → resolve uids and kinds → emit.

use std::io::Write;

use gdview::autoload::{AutoloadKind, ResolvedAutoload};
use gdview::uid::UidMap;

use crate::cli::*;
use crate::context::Context;
use crate::render::{self, Exit, Human};

pub fn run(ctx: &Context, args: ProjectArgs) -> gdproject::Result<Exit> {
    let project = gdview::Project::discover(&args.project)?;
    let uids = UidMap::build(&project)?;
    let autoloads = project.settings()?.autoloads()?.resolve(&project, &uids);
    render::write(ctx.output, &Report { autoloads })?;
    Ok(Exit::Ok)
}

#[derive(serde::Serialize)]
struct Report {
    autoloads: Vec<ResolvedAutoload>,
}

impl Human for Report {
    fn human(&self, out: &mut dyn Write) -> std::io::Result<()> {
        if self.autoloads.is_empty() {
            return writeln!(out, "no autoloads");
        }
        let width = self
            .autoloads
            .iter()
            .map(|a| a.name.len())
            .max()
            .unwrap_or(0);
        for autoload in &self.autoloads {
            let target = match (&autoload.path, &autoload.uid) {
                (Some(path), _) => path.to_string(),
                (None, Some(uid)) => uid.0.clone(),
                (None, None) => String::new(),
            };
            let mut notes = Vec::new();
            if !autoload.singleton {
                notes.push("not a global name".to_owned());
            }
            if let (Some(uid), Some(_)) = (&autoload.uid, &autoload.path) {
                notes.push(format!("from {}", uid.0));
            }
            if autoload.kind == AutoloadKind::Unresolved {
                notes.push("no file claims this uid".to_owned());
            } else if !autoload.exists {
                notes.push("missing".to_owned());
            }
            let notes = match notes.is_empty() {
                true => String::new(),
                false => format!("  ({})", notes.join("; ")),
            };
            writeln!(
                out,
                "{:>2}  {:width$}  {:10}  {target}{notes}",
                autoload.order,
                autoload.name,
                autoload.kind.name(),
            )?;
        }
        Ok(())
    }
}
