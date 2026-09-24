//! Godot's derived caches under `.godot/`. `status` is offline; the rest run the engine.
//!
//! # Tests (tests/cache.rs)
//! Offline: `status_reports_presence_and_sizes_without_creating_anything`,
//! `clean_targets_are_a_closed_list_and_never_follow_symlinks_or_reparse_points`,
//! `clean_dry_run_lists_without_removing`, `clean_refuses_while_a_session_is_running`.
//! Engine (`#[ignore]`): `real_engine_refresh_persists_uid_mapping_after_move`.

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;

use crate::engine::Engine;
use crate::workspace::Workspace;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CacheStatus {
    pub entries: Vec<CacheEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CacheEntry {
    pub name: String,
    pub path: PathBuf,
    pub present: bool,
    pub bytes: u64,
}

pub fn status(workspace: &Workspace) -> crate::Result<CacheStatus> {
    todo!()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RefreshReport {
    pub diagnostics: Vec<crate::diagnostics::Diagnostic>,
    pub elapsed_ms: u64,
}

/// `--editor --import` on the real project, under the workspace lock.
pub fn refresh(workspace: &Workspace, engine: &Engine, deadline: Duration) -> crate::Result<RefreshReport> {
    todo!()
}

/// Removes the UID and script-class indexes, then `refresh`.
pub fn rebuild(workspace: &Workspace, engine: &Engine, deadline: Duration) -> crate::Result<RefreshReport> {
    todo!()
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CleanOptions {
    pub dry_run: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CleanReport {
    pub removed: Vec<PathBuf>,
    pub dry_run: bool,
}

pub fn clean(workspace: &Workspace, options: CleanOptions) -> crate::Result<CleanReport> {
    todo!()
}
