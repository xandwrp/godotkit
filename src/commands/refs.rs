//! `refs` (offline): Project::discover → declarations + UidMap + ProjectGraph::load → gdview::xref::refs(path) → emit. Exit::Failed when the path itself does not exist.
//! The path is `res://…`, `uid://…`, or relative to the project root.

use std::io::Write;
use std::path::Path;

use gdview::ResPath;
use gdview::respath::Uid;
use gdview::uid::UidMap;
use gdview::xref::{ProjectGraph, ReferenceKind, Refs};

use crate::cli::*;
use crate::context::Context;
use crate::render::{self, Exit, Human};

pub fn run(ctx: &Context, project: ProjectArgs, path: String) -> gdproject::Result<Exit> {
    let project = gdview::Project::discover(&project.project)?;
    let declarations = gdview::declarations::index_project(&project)?;
    let uids = UidMap::build(&project)?;
    let query = if path.starts_with("uid://") {
        uids.resolve(&Uid(path.clone()))
            .cloned()
            .ok_or_else(|| gdproject::Error::Invalid(format!("no project file claims {path}")))?
    } else if path.starts_with("res://") {
        ResPath::parse(&path)?
    } else {
        project.localize(Path::new(&path))?
    };
    let graph = ProjectGraph::load(&project, &declarations, &uids)?;
    let report = Report(gdview::xref::refs(&graph, &query)?);
    render::write(ctx.output, &report)?;
    Ok(if report.0.exists {
        Exit::Ok
    } else {
        Exit::Failed
    })
}

#[derive(serde::Serialize)]
#[serde(transparent)]
struct Report(Refs);

impl Human for Report {
    fn human(&self, out: &mut dyn Write) -> std::io::Result<()> {
        let refs = &self.0;
        write!(out, "{}", refs.path)?;
        if let Some(uid) = &refs.uid {
            write!(out, "  ({})", uid.0)?;
        }
        if !refs.exists {
            write!(out, "  (does not exist)")?;
        }
        writeln!(out)?;
        if !refs.suggestions.is_empty() {
            writeln!(out, "did you mean {}?", refs.suggestions.join(", "))?;
        }
        if !refs.sidecars.is_empty() {
            let sidecars: Vec<String> = refs.sidecars.iter().map(ToString::to_string).collect();
            writeln!(out, "move with it: {}", sidecars.join(", "))?;
        }
        if let Some(class_name) = &refs.class_name {
            writeln!(
                out,
                "class_name {class_name}: uses by that name are not listed"
            )?;
        }
        match refs.references.len() {
            0 => return writeln!(out, "no references"),
            1 => writeln!(out, "1 reference")?,
            n => writeln!(out, "{n} references")?,
        }
        let rows: Vec<(String, String)> = refs
            .references
            .iter()
            .map(|r| {
                let mut what = kind(r.kind).to_owned();
                if let Some(node) = &r.node {
                    what.push_str(&format!(" on node {}", node.0));
                }
                if let Some(key) = &r.key {
                    what.push_str(&format!(" {key}"));
                }
                if refs.directory {
                    what.push_str(&format!(" -> {}", r.target));
                }
                if r.by_uid {
                    what.push_str(" (by uid)");
                }
                (format!("{}:{}", r.at.path, r.at.line), what)
            })
            .collect();
        let width = rows.iter().map(|(at, _)| at.len()).max().unwrap_or(0);
        for (at, what) in rows {
            writeln!(out, "  {at:width$}  {what}")?;
        }
        Ok(())
    }
}

fn kind(kind: ReferenceKind) -> &'static str {
    match kind {
        ReferenceKind::ExtResource => "ext_resource",
        ReferenceKind::Script => "script",
        ReferenceKind::Instance => "instance",
        ReferenceKind::Inherits => "inherits",
        ReferenceKind::Placeholder => "instance_placeholder",
        ReferenceKind::Property => "property",
        ReferenceKind::Preload => "preload",
        ReferenceKind::Load => "load",
        ReferenceKind::Extends => "extends",
        ReferenceKind::String => "string",
        ReferenceKind::Autoload => "autoload",
        ReferenceKind::MainScene => "main scene",
        ReferenceKind::ProjectSetting => "setting",
    }
}
