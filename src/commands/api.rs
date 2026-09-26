//! `api`: `--dump` → the native index as JSON (standalone outside a project).
//! Otherwise workspace → engine → `load_native` (cached) → a class, member, or
//! global the engine knows is answered from it alone; anything else (search,
//! project classes, misses) first adds the project's scripts
//! (`ProjectApi::with_scripts`). A miss exits 1 with suggestions.

use std::io::Write;

use gdproject::api::{self, DEFAULT_DUMP_DEADLINE, ProjectApi, ScriptDocs};
use gdview::api::answer::{self, Answer};
use serde::Serialize;

use crate::cli::*;
use crate::context::Context;
use crate::render::{self, Exit};

mod human;

pub fn run(ctx: &Context, args: ApiArgs) -> gdproject::Result<Exit> {
    if args.dump {
        return dump(ctx, &args.project);
    }
    let query = args.query.as_deref().unwrap_or_default();
    let search = query == "search";
    if search && args.member.is_none() {
        return Err(gdproject::Error::Invalid(
            "`gdkit api search` needs a term, e.g. `gdkit api search multiplayer`".into(),
        ));
    }
    let workspace = ctx.workspace(&args.project)?;
    let engine = ctx.engine(&workspace, &args.project)?;
    let (native, _) = api::load_native(&workspace, &engine, DEFAULT_DUMP_DEADLINE)?;
    let native_knows =
        native.class(query).is_some() || (args.member.is_none() && native.global(query).is_some());
    let (index, scripts) = if native_knows && !search {
        (native, None)
    } else {
        let project = ProjectApi::with_scripts(native, &workspace, &engine, DEFAULT_DUMP_DEADLINE)?;
        (project.index, Some(project.scripts))
    };
    let answer = match (search, args.member.as_deref()) {
        (true, Some(term)) => answer::search(&index, term, args.limit),
        (false, Some(member)) => answer::lookup_member(&index, query, member),
        (_, None) => answer::lookup(&index, query),
    };
    let exit = if matches!(answer, Answer::Miss(_)) {
        Exit::Failed
    } else {
        Exit::Ok
    };
    let report = Report {
        engine_version: index.engine_version,
        answer,
        project_scripts: scripts,
    };
    render::emit(ctx.output, &report).map_err(stdout_error)?;
    Ok(exit)
}

/// Inside a project the cached index; outside one, a fresh standalone dump.
fn dump(ctx: &Context, project: &ProjectArgs) -> gdproject::Result<Exit> {
    let index = match ctx.workspace(project) {
        Ok(workspace) => {
            let engine = ctx.engine(&workspace, project)?;
            api::load_native(&workspace, &engine, DEFAULT_DUMP_DEADLINE)?.0
        }
        Err(gdproject::Error::View(gdview::Error::NotAProject { .. })) => {
            let engine = ctx.standalone_engine(project.godot.as_deref())?;
            api::load_native_standalone(&engine, DEFAULT_DUMP_DEADLINE)?
        }
        Err(error) => return Err(error),
    };
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, &index)?;
    writeln!(stdout).map_err(stdout_error)?;
    Ok(Exit::Ok)
}

fn stdout_error(source: std::io::Error) -> gdproject::Error {
    gdproject::Error::Io {
        path: "<stdout>".into(),
        source,
    }
}

#[derive(Serialize)]
struct Report {
    engine_version: String,
    #[serde(flatten)]
    answer: Answer,
    /// How the project's scripts were documented, when the answer needed them.
    #[serde(skip_serializing_if = "Option::is_none")]
    project_scripts: Option<ScriptDocs>,
}
