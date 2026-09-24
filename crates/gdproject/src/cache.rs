//! Refreshing Godot's derived caches under `.godot/` with a headless editor import.
//! Needed once on a fresh clone before `resource create` or `api` can see the
//! project's class cache and imported assets. This is the only operation that
//! writes the real project's `.godot`, and it runs under the workspace lock.
//!
//! # Tests (tests/cache.rs)
//! Offline: `refresh_takes_the_lock_and_records_diagnostics` (fake-godot).
//! Engine (`#[ignore]`): `real_engine_refresh_persists_uid_mapping_after_move`.

use std::time::Duration;

use serde::Serialize;

use crate::engine::Engine;
use crate::workspace::Workspace;

pub const DEFAULT_DEADLINE: Duration = Duration::from_secs(600);

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RefreshReport {
    pub diagnostics: Vec<crate::diagnostics::Diagnostic>,
    pub elapsed_ms: u64,
}

/// `--editor --import` on the real project.
pub fn refresh(workspace: &Workspace, engine: &Engine, deadline: Duration) -> crate::Result<RefreshReport> {
    todo!()
}
