//! Native API reflection (cached) merged with project declarations (live).
//!
//! Cache key: engine fingerprint + hash of the project's `.gdextension` files
//! and every library they point at (relative or `res://`). A harness run that
//! prints `ERROR:` never populates the cache.
//!
//! # Tests (tests/api.rs, offline with `fake-godot` returning a canned index)
//! - `load_populates_cache_then_reuses_it`
//! - `cache_key_changes_with_engine_fingerprint_or_extension_library_bytes`
//! - `cache_is_skipped_when_reflection_run_printed_errors`
//! - `cache_write_is_atomic_and_corrupt_cache_is_replaced_not_fatal`
//! - `project_api_prefers_native_then_project_classes_and_reports_shadowing`
//! - `dump_outside_a_project_uses_an_empty_scratch_project`
//!   Engine (`#[ignore]`): `real_engine_reflects_gdextension_classes_and_inheritance`

use std::path::Path;
use std::time::Duration;

use gdview::api::ApiIndex;
use gdview::declarations::ProjectDeclarations;

use crate::engine::Engine;
use crate::workspace::Workspace;

pub const DEFAULT_REFLECT_DEADLINE: Duration = Duration::from_secs(120);

/// Reflects (or loads from cache) the native index for this engine + project.
pub fn load_native(workspace: &Workspace, engine: &Engine, deadline: Duration) -> crate::Result<ApiIndex> {
    todo!()
}

/// Reflects with an empty scratch project; no cache. For `--dump-json` outside a project.
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

pub struct SearchResult<'a> {
    pub class: ClassView<'a>,
    pub member: Option<String>,
}
