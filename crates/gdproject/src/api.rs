//! Engine API index from the configured editor, cached per engine in the
//! project's state directory, merged with the project's own scripts.
//!
//! Native flow ([`load_native`]): a cache hit on (engine fingerprint, index
//! schema) parses `.godot/gdkit/api-index.json`. A miss runs two projectless
//! engine commands in a bare scratch directory, both through
//! [`runner::run_projectless`]:
//! 1. `--dump-extension-api-with-docs` → [`ApiIndex::from_extension_api_json`].
//!    Never under `--path`: Godot 4.7.2 then writes the dump into the project
//!    and aborts. The dump does not depend on the project, so none is needed.
//! 2. `--doctool <dir>` → [`doc_xml::parse_class`] per file →
//!    [`ApiIndex::merge_doctool`] (`@GDScript`, property defaults).
//!
//! Both must exit 0 and produce parseable output, or the load fails and
//! nothing is cached. The cache is replaced atomically; a corrupt or stale
//! cache is a miss, never an error.
//!
//! Project GDExtension classes are deferred: `extension_classes` stays empty.
//!
//! # Tests (tests/api.rs, offline with `fake-godot`)
//! - `load_populates_cache_then_reuses_it`
//! - `cache_key_changes_with_engine_fingerprint`
//! - `failed_or_empty_engine_runs_are_errors_and_are_not_cached`
//! - `corrupt_or_stale_cache_is_replaced_not_fatal`
//! - `dump_runs_without_path_in_a_bare_directory`
//! - `imported_projects_are_documented_in_place_and_merge_over_native_classes`
//! - `undocumented_scripts_fall_back_to_source_with_the_engine_reason`
//! - `not_imported_or_locked_projects_are_documented_from_a_copy`
//! - `a_failed_docs_run_falls_back_everything_and_is_not_cached`
//! - `script_docs_are_cached_until_a_script_changes`
//! - `autoload_scripts_answer_to_their_autoload_name`
//! - `a_uid_main_scene_does_not_abort_the_docs_run_and_scriptless_projects_skip_it`
//!
//! Engine (`#[ignore]`, `GDKIT_TEST_GODOT`): `real_engine_native_index_has_builtins_utilities_gdscript_and_docs`,
//! `real_engine_script_docs_leave_the_project_untouched`

use std::path::Path;
use std::time::{Duration, Instant};

use gdview::api::{API_INDEX_SCHEMA_VERSION, ApiIndex, doc_xml};
use serde::{Deserialize, Serialize};

use crate::engine::Engine;
use crate::process::Captured;
use crate::runner::{self, Invocation};
use crate::workspace::{API_CACHE_FILE, IsolatedCopy, Workspace};

mod project;

pub use project::{ScriptDocs, ScriptDocsSource, ScriptFallback, load_scripts};

/// For both engine runs together.
pub const DEFAULT_DUMP_DEADLINE: Duration = Duration::from_secs(120);

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct CacheKey {
    engine_fingerprint: String,
    schema_version: u32,
}

#[derive(Serialize)]
struct CacheRecord<'a> {
    key: &'a CacheKey,
    index: &'a ApiIndex,
}

#[derive(Deserialize)]
struct CachedRecord {
    key: CacheKey,
    index: ApiIndex,
}

/// Whether [`load_native`] answered from the cache.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheUse {
    Hit,
    Miss,
}

/// The native index for this engine, from the project's cache or a fresh dump.
pub fn load_native(
    workspace: &Workspace,
    engine: &Engine,
    deadline: Duration,
) -> crate::Result<(ApiIndex, CacheUse)> {
    let key = CacheKey {
        engine_fingerprint: engine.fingerprint.clone(),
        schema_version: API_INDEX_SCHEMA_VERSION,
    };
    if let Some(index) = read_cache(&workspace.api_cache_path(), &key) {
        return Ok((index, CacheUse::Hit));
    }
    let index = dump_native(engine, deadline)?;
    let bytes = serde_json::to_vec(&CacheRecord {
        key: &key,
        index: &index,
    })?;
    workspace.replace_state_file(API_CACHE_FILE, &bytes)?;
    Ok((index, CacheUse::Miss))
}

/// A fresh dump with no cache, for `api --dump` outside a project.
pub fn load_native_standalone(engine: &Engine, deadline: Duration) -> crate::Result<ApiIndex> {
    dump_native(engine, deadline)
}

/// Unreadable, corrupt, or stale caches are misses.
fn read_cache(path: &Path, key: &CacheKey) -> Option<ApiIndex> {
    let bytes = std::fs::read(path).ok()?;
    let record: CachedRecord = serde_json::from_slice(&bytes).ok()?;
    (record.key == *key).then_some(record.index)
}

fn dump_native(engine: &Engine, deadline: Duration) -> crate::Result<ApiIndex> {
    let started = Instant::now();
    let scratch = IsolatedCopy::bare()?;
    let mut invocation = Invocation::new(engine, scratch.path(), deadline);

    invocation.engine_args = vec!["--dump-extension-api-with-docs".into()];
    let what = "engine API dump (--dump-extension-api-with-docs)";
    finished(&runner::run_projectless(&invocation)?.0, what, deadline)?;
    let dump = scratch.path().join("extension_api.json");
    let text = std::fs::read_to_string(&dump).map_err(|error| crate::Error::EngineRun {
        what: what.into(),
        message: format!(
            "the engine exited cleanly but {} is unreadable: {error}",
            dump.display()
        ),
    })?;
    let mut index = ApiIndex::from_extension_api_json(&text)?;

    // Godot refuses a --doctool directory that does not exist yet.
    let docs = scratch.path().join("doctool");
    std::fs::create_dir(&docs).map_err(|source| crate::Error::Io {
        path: docs.clone(),
        source,
    })?;
    invocation.deadline = deadline
        .checked_sub(started.elapsed())
        .filter(|left| !left.is_zero())
        .ok_or_else(|| crate::Error::Timeout {
            what: "engine API dump".into(),
            deadline,
        })?;
    invocation.engine_args = vec!["--doctool".into(), docs.clone().into_os_string()];
    let what = "engine class reference (--doctool)";
    finished(&runner::run_projectless(&invocation)?.0, what, deadline)?;
    let mut files = Vec::new();
    collect_xml(&docs, &mut files)?;
    files.sort();
    let mut classes = Vec::with_capacity(files.len());
    for file in files {
        let text = std::fs::read_to_string(&file).map_err(|source| crate::Error::Io {
            path: file.clone(),
            source,
        })?;
        classes.push(doc_xml::parse_class(&text).map_err(|error| error.with_path(file))?);
    }
    if !classes.iter().any(|doc| doc.class.name == "@GDScript") {
        return Err(crate::Error::EngineRun {
            what: what.into(),
            message: "the engine wrote no @GDScript reference".into(),
        });
    }
    index.merge_doctool(classes);
    Ok(index)
}

/// Maps an unsuccessful run to an error that carries the end of its output.
fn finished(captured: &Captured, what: &str, deadline: Duration) -> crate::Result<()> {
    if captured.timed_out {
        return Err(crate::Error::Timeout {
            what: what.into(),
            deadline,
        });
    }
    if captured.success() {
        return Ok(());
    }
    let output = String::from_utf8_lossy(&captured.stderr()).into_owned()
        + &String::from_utf8_lossy(&captured.stdout());
    let tail: Vec<&str> = output.lines().rev().take(5).collect();
    let tail: Vec<&str> = tail.into_iter().rev().collect();
    let status = match captured.status {
        _ if captured.output_limit_exceeded => "output limit exceeded".to_owned(),
        Some(status) => status.to_string(),
        None => "no exit status".to_owned(),
    };
    Err(crate::Error::EngineRun {
        what: what.into(),
        message: if tail.is_empty() {
            status
        } else {
            format!("{status}\n{}", tail.join("\n"))
        },
    })
}

fn collect_xml(directory: &Path, files: &mut Vec<std::path::PathBuf>) -> crate::Result<()> {
    let io = |source| crate::Error::Io {
        path: directory.to_owned(),
        source,
    };
    for entry in std::fs::read_dir(directory).map_err(io)? {
        let entry = entry.map_err(io)?;
        let kind = entry.file_type().map_err(io)?;
        let path = entry.path();
        if kind.is_dir() {
            collect_xml(&path, files)?;
        } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "xml") {
            files.push(path);
        }
    }
    Ok(())
}

/// The native index with the project's script classes added, for `gdkit api`.
pub struct ProjectApi {
    pub index: ApiIndex,
    pub scripts: ScriptDocs,
    /// Script classes not added because an engine class has the same name.
    pub shadowed: Vec<String>,
}

impl ProjectApi {
    /// Adds the project's scripts to an already loaded native index.
    pub fn with_scripts(
        mut index: ApiIndex,
        workspace: &Workspace,
        engine: &Engine,
        deadline: Duration,
    ) -> crate::Result<Self> {
        let (classes, scripts) = load_scripts(workspace, engine, deadline)?;
        let shadowed = index.add_scripts(classes);
        Ok(Self {
            index,
            scripts,
            shadowed,
        })
    }
}
