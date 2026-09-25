//! Where gdkit is allowed to write: `.godot/gdkit/**` inside the project, scratch
//! copies in the temp dir, and brand-new files published with create-new semantics.
//!
//! # Tests (tests/workspace.rs)
//! - `open_requires_a_project_and_creates_state_dir_lazily`
//! - `lock_is_exclusive_across_processes_and_released_on_drop`
//! - `isolated_copy_full_excludes_dot_godot_dot_git_and_refuses_symlinks`
//! - `isolated_copy_slice_rejects_dot_dot_absolute_and_dot_godot_and_keeps_relative_layout`
//! - `isolated_copy_is_removed_on_drop`
//! - `artifact_dir_names_are_unique_and_sortable`
//! - `publish_new_file_is_atomic_and_never_overwrites`
//! - `publish_new_file_falls_back_from_hard_link_to_rename_on_filesystems_without_links`

use std::path::{Path, PathBuf};

use gdview::Project;

pub struct Workspace {
    project: Project,
    state_dir: PathBuf,
}

impl Workspace {
    /// Opens the project at `root` (no discovery; the CLI decides that).
    pub fn open(root: &Path) -> crate::Result<Self> {
        let project = Project::open(root)?;
        let state_dir = project.root().join(".godot").join("gdkit");
        Ok(Self { project, state_dir })
    }
    pub fn project(&self) -> &Project {
        &self.project
    }
    pub fn root(&self) -> &Path {
        self.project.root()
    }
    /// `<root>/.godot/gdkit`
    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }
    pub fn probe_cache_path(&self) -> PathBuf {
        self.state_dir.join("engine-probe.json")
    }
    pub fn api_cache_path(&self) -> PathBuf {
        self.state_dir.join("api-index.json")
    }
    /// Exclusive lock for operations that write the real `.godot` (cache refresh).
    pub fn lock(&self) -> crate::Result<Lock> {
        todo!()
    }
    /// New `<state>/artifacts/<kind>/<unix_ms>-<pid>-<n>/`.
    pub fn new_artifact_dir(&self, kind: &str) -> crate::Result<ArtifactDir> {
        todo!()
    }
}

pub struct Lock {
    path: PathBuf,
    // platform file lock
}

impl Drop for Lock {
    fn drop(&mut self) {
        todo!()
    }
}

pub struct ArtifactDir {
    pub path: PathBuf,
}

impl ArtifactDir {
    pub fn write(&self, name: &str, bytes: &[u8]) -> crate::Result<PathBuf> {
        todo!()
    }
}

/// A disposable copy of the project in the temp dir, removed on drop.
pub struct IsolatedCopy {
    pub path: PathBuf,
}

impl IsolatedCopy {
    /// A bare `project.godot` with `config_version=5`.
    pub fn empty() -> crate::Result<Self> {
        todo!()
    }
    /// Everything except `.godot` and `.git`. Symlinks are an error.
    pub fn full(project: &Project) -> crate::Result<Self> {
        todo!()
    }
    /// Only the selected relative paths, plus a minimal `project.godot`
    /// unless `project.godot` is itself selected.
    pub fn slice(project: &Project, selections: &[PathBuf]) -> crate::Result<Self> {
        todo!()
    }
}

impl Drop for IsolatedCopy {
    fn drop(&mut self) {
        todo!()
    }
}

/// Atomically publishes `staged` at `destination`. Fails if `destination` exists.
/// Prefers a hard link (true create-new), falls back to rename where links are unsupported.
pub fn publish_new_file(staged: &Path, destination: &Path) -> crate::Result<()> {
    todo!()
}
