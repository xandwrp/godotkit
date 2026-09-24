//! Engine API index from `--dump-extension-api-with-docs`, cached per engine +
//! project extension set, merged with live project declarations.
//!
//! Flow: cache hit on (engine fingerprint, extension fingerprint) → parse cached
//! index. Miss → `run_engine` with `--dump-extension-api-with-docs` in a temp dir
//! (with `--path <project>` so registered GDExtension classes can appear) →
//! `ApiIndex::from_extension_api_json` → atomic cache write unless the run
//! printed `ERROR:`.
//!
//! To verify on the target engine: whether project GDExtension classes appear in
//! the dump under `--path`. If they do not, `extension_classes` stays empty and a
//! small ClassDB harness is the documented follow-up, scoped to that delta only.
//!
//! # Tests (tests/api.rs, offline with `fake-godot` writing a canned dump)
//! - `load_populates_cache_then_reuses_it`
//! - `cache_key_changes_with_engine_fingerprint_or_extension_library_bytes`
//! - `cache_is_skipped_when_dump_run_printed_errors`
//! - `cache_write_is_atomic_and_corrupt_cache_is_replaced_not_fatal`
//! - `project_api_prefers_native_then_project_classes_and_reports_shadowing`
//! - `dump_outside_a_project_uses_an_empty_scratch_project`
//!
//! Engine (`#[ignore]`): `real_engine_dump_parses_and_includes_builtins_utilities_and_docs`,
//! `real_engine_dump_includes_project_gdextension_classes` (the open question above)

use std::path::Path;
use std::time::Duration;

use gdview::api::ApiIndex;
use gdview::declarations::ProjectDeclarations;

use crate::engine::Engine;
use crate::workspace::Workspace;

pub const DEFAULT_DUMP_DEADLINE: Duration = Duration::from_secs(120);

/// Dumps (or loads from cache) the native index for this engine + project.
pub fn load_native(workspace: &Workspace, engine: &Engine, deadline: Duration) -> crate::Result<ApiIndex> {
    todo!()
}

/// Dumps with an empty scratch project; no cache. For `api --dump` outside a project.
pub fn load_native_standalone(engine: &Engine, deadline: Duration) -> crate::Result<ApiIndex> {
    todo!()
}

/// Hash of `.gdextension` descriptors and the library files they reference.
pub fn extension_fingerprint(project_root: &Path) -> crate::Result<String> {
    todo!()
}

/// Native + project view used by `gdkit api`.
pub struct ProjectApi {
    pub native: ApiIndex,
    pub project: ProjectDeclarations,
}

impl ProjectApi {
    pub fn load(workspace: &Workspace, engine: &Engine, deadline: Duration) -> crate::Result<Self> {
        todo!()
    }
    pub fn lookup_class(&self, name: &str) -> Option<ClassView<'_>> {
        todo!()
    }
    pub fn lookup_member(&self, class: &str, member: &str) -> MemberLookup<'_> {
        todo!()
    }
    /// Utility functions and global enums when `class` is not given.
    pub fn lookup_global(&self, name: &str) -> GlobalLookup<'_> {
        todo!()
    }
    pub fn search(&self, term: &str, limit: usize) -> Vec<SearchResult<'_>> {
        todo!()
    }
}

pub enum ClassView<'a> {
    Native(&'a gdview::api::ApiClass),
    Project(&'a gdview::declarations::ScriptDeclaration),
}

pub enum MemberLookup<'a> {
    Found { declaring: ClassView<'a>, member: MemberView<'a> },
    ClassMissing { suggestions: Vec<String> },
    MemberMissing { class: ClassView<'a>, suggestions: Vec<String> },
}

pub enum MemberView<'a> {
    Native(gdview::api::MemberHit<'a>),
    Project(&'a gdview::declarations::MemberDeclaration),
}

pub enum GlobalLookup<'a> {
    Utility(&'a gdview::api::ApiMethod),
    Enum(&'a gdview::api::ApiEnum),
    Missing { suggestions: Vec<String> },
}

pub struct SearchResult<'a> {
    pub hit: gdview::api::SearchHit<'a>,
    pub project_class: Option<&'a gdview::declarations::ScriptDeclaration>,
}
