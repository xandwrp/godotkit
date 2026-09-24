//! An engine executable that has passed the compatibility probe.
//!
//! The probe result is cached per project under the engine's identity
//! (path, size, mtime, companion binary on Windows, and a hash of the harness
//! sources) so a rebuilt engine or a changed harness re-probes automatically.
//!
//! # Tests (tests/engine.rs, offline with `fake-godot`)
//! - `attach_probes_once_then_hits_cache`
//! - `cache_misses_when_engine_size_or_mtime_or_harness_hash_changes`
//! - `cache_is_not_written_when_probe_fails_or_engine_changes_mid_probe`
//! - `probe_rejects_engines_missing_headless_editor_flags`
//! - `probe_rejects_non_editor_or_non_4x_builds`
//! - `probe_respects_deadline` (fake engine that hangs)
//! - `console_launcher_tracks_companion_exe_on_windows`
//! - `fingerprint_is_stable_for_identical_key_and_differs_otherwise`

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::EngineSelection;
use crate::workspace::Workspace;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Engine {
    pub executable: PathBuf,
    pub version: String,
    /// blake3 of [`ProbeKey`]; identifies engine + harness generation.
    pub fingerprint: String,
    pub source: crate::config::SelectionSource,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeKey {
    pub files: Vec<TrackedFile>,
    pub harness_hash: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackedFile {
    pub path: PathBuf,
    pub size: u64,
    pub modified_unix_ns: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeCacheHealth {
    Missing,
    Current,
    Stale,
    Malformed,
    Unreadable,
}

pub const DEFAULT_PROBE_DEADLINE: Duration = Duration::from_secs(60);

impl Engine {
    /// Uses the cached probe when the key matches, otherwise runs [`probe`] and caches.
    /// Returns whether the cache was hit so `doctor` can say so.
    pub fn attach(
        selection: &EngineSelection,
        workspace: &Workspace,
        deadline: Duration,
    ) -> crate::Result<(Engine, bool)> {
        todo!()
    }

    /// Like `attach` but with no project: probes into a temp dir, no cache.
    pub fn attach_standalone(selection: &EngineSelection, deadline: Duration) -> crate::Result<Engine> {
        todo!()
    }
}

pub fn probe_key(executable: &Path) -> std::io::Result<ProbeKey> {
    todo!()
}

pub fn probe_cache_health(executable: &Path, workspace: &Workspace) -> ProbeCacheHealth {
    todo!()
}

#[derive(Clone, Debug, Deserialize)]
pub struct ProbeReport {
    pub version: String,
    pub editor: bool,
    pub major: u32,
}

/// Runs `--help` and the probe harness. Requires headless editor flags,
/// Godot 4, editor feature, and working resource loading.
pub fn probe(executable: &Path, deadline: Duration) -> crate::Result<ProbeReport> {
    todo!()
}
