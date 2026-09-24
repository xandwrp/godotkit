//! A Godot project on disk. Discovery, file enumeration, and path mapping.
//!
//! # Tests (tests/project.rs)
//! - `open_requires_project_godot_as_regular_file`
//! - `discover_walks_ancestors_and_stops_at_first_marker`
//! - `discover_from_file_starts_at_its_parent`
//! - `localize_maps_os_paths_inside_root_and_rejects_outside`
//! - `localize_rejects_paths_under_dot_godot`
//! - `globalize_is_inverse_of_localize`
//! - `settings_reads_project_godot_lazily_each_call`

use std::path::{Path, PathBuf};

use crate::files::FileQuery;
use crate::respath::ResPath;
use crate::settings::Settings;

/// Filesystem handle to a project root. Cheap to clone. Holds no parsed state:
/// every accessor re-reads, so results reflect disk at call time.
#[derive(Clone, Debug)]
pub struct Project {
    root: PathBuf,
}

impl Project {
    /// Opens a directory that directly contains `project.godot`. Does not search.
    pub fn open(root: impl AsRef<Path>) -> crate::Result<Self> {
        todo!()
    }

    /// Finds the nearest ancestor containing `project.godot`.
    pub fn discover(from: impl AsRef<Path>) -> crate::Result<Self> {
        todo!()
    }

    /// Absolute, normalized root. Not canonicalized (symlinked checkouts keep their spelling).
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn config_path(&self) -> PathBuf {
        self.root.join("project.godot")
    }

    /// Parses `project.godot` now.
    pub fn settings(&self) -> crate::Result<Settings> {
        todo!()
    }

    /// Enumerates project files. See [`FileQuery`] for ignore semantics.
    pub fn files(&self, query: &FileQuery) -> crate::Result<Vec<PathBuf>> {
        todo!()
    }

    /// OS path inside the root -> `res://` path. Errors if outside the root or under `.godot`.
    pub fn localize(&self, path: &Path) -> crate::Result<ResPath> {
        todo!()
    }

    /// `res://` path -> OS path under the root. Never fails; existence is not checked.
    pub fn globalize(&self, path: &ResPath) -> PathBuf {
        todo!()
    }

    /// Reads a project file by `res://` path.
    pub fn read_to_string(&self, path: &ResPath) -> crate::Result<String> {
        todo!()
    }
}
