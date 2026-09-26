//! `resource schema`: workspace → engine → gdproject::resource::schema → emit.
//! `resource create`: read spec JSON → CreateSpec::from_json → workspace → engine
//! → gdproject::resource::create → emit CreateReport. Verify/publish failures are
//! `Err` (exit 2): nothing was written, and that is a tool outcome, not a project verdict.

use std::path::Path;

use gdproject::resource::{self, CreateSpec, DEFAULT_DEADLINE};
use gdview::ResPath;
use gdview::variant::ResourceTarget;

use crate::cli::*;
use crate::context::Context;
use crate::render::{self, Exit};

mod human;

pub fn run(ctx: &Context, command: ResourceCommand) -> gdproject::Result<Exit> {
    match command {
        ResourceCommand::Schema {
            project,
            class,
            script,
        } => schema(ctx, &project, class, script),
        ResourceCommand::Create { project, spec, out } => create(ctx, &project, &spec, &out),
    }
}

/// Everything checkable offline (spec, destination, references) fails before
/// an engine is selected.
fn create(
    ctx: &Context,
    project: &ProjectArgs,
    spec_path: &Path,
    out: &str,
) -> gdproject::Result<Exit> {
    let text = std::fs::read_to_string(spec_path).map_err(|source| gdproject::Error::Io {
        path: spec_path.to_owned(),
        source,
    })?;
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|error| {
        gdproject::Error::Invalid(format!("{}: not JSON: {error}", spec_path.display()))
    })?;
    let spec = CreateSpec::from_json(&json)
        .map_err(|error| gdproject::Error::Invalid(format!("{}: {error}", spec_path.display())))?;
    let workspace = ctx.workspace(project)?;
    let destination = project_path(&workspace, out)?;
    resource::check_destination(&workspace, &destination)?;
    resource::check_references(&workspace, &spec)?;
    let engine = ctx.engine(&workspace, project)?;
    let report = resource::create(&workspace, &engine, &spec, &destination, DEFAULT_DEADLINE)?;
    render::write(ctx.output, &human::CreateOutput(report))?;
    Ok(Exit::Ok)
}

fn schema(
    ctx: &Context,
    project: &ProjectArgs,
    class: Option<String>,
    script: Option<String>,
) -> gdproject::Result<Exit> {
    let workspace = ctx.workspace(project)?;
    let target = match (class, script) {
        (Some(class), _) => ResourceTarget::Class(class),
        (None, Some(script)) => ResourceTarget::Script(project_path(&workspace, &script)?),
        (None, None) => unreachable!("clap requires --class or --script"),
    };
    let engine = ctx.engine(&workspace, project)?;
    let schema = resource::schema(&workspace, &engine, &target, DEFAULT_DEADLINE)?;
    render::write(ctx.output, &human::SchemaReport(schema))?;
    Ok(Exit::Ok)
}

/// `res://…` as given; anything else is a path inside the project.
fn project_path(workspace: &gdproject::Workspace, path: &str) -> gdproject::Result<ResPath> {
    Ok(if path.starts_with("res://") {
        ResPath::parse(path)?
    } else {
        workspace.project().localize(Path::new(path))?
    })
}
