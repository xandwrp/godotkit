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

use std::path::{Component, Path, PathBuf};

use crate::Error;
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
        let root = absolute(root.as_ref())?;
        if !has_config(&root)? {
            return Err(Error::NotAProject { path: root });
        }
        Ok(Self { root })
    }

    /// Finds the nearest ancestor containing `project.godot`.
    /// A file starts the search at its parent. `from` must exist.
    pub fn discover(from: impl AsRef<Path>) -> crate::Result<Self> {
        let from = absolute(from.as_ref())?;
        let metadata = std::fs::metadata(&from).map_err(|source| Error::Io { path: from.clone(), source })?;
        let mut ancestors = from.ancestors();
        if !metadata.is_dir() {
            ancestors.next();
        }
        for root in ancestors {
            if has_config(root)? {
                return Ok(Self { root: root.to_path_buf() });
            }
        }
        Err(Error::NotAProject { path: from })
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
        Ok(crate::files::walk(&self.root, query)?.files)
    }

    /// OS path inside the root -> `res://` path. Errors if outside the root or under `.godot`.
    /// Relative paths are taken relative to the root. The path is not required to exist.
    pub fn localize(&self, path: &Path) -> crate::Result<ResPath> {
        let invalid = || Error::InvalidResPath(path.display().to_string());
        let absolute = if path.is_absolute() { path.to_path_buf() } else { self.root.join(path) };
        let relative = absolute.strip_prefix(&self.root).map_err(|_| invalid())?;
        let mut segments = Vec::new();
        for component in relative.components() {
            match component {
                Component::Normal(segment) => segments.push(segment.to_str().ok_or_else(invalid)?),
                Component::CurDir => {}
                _ => return Err(invalid()),
            }
        }
        ResPath::from_relative(&segments.join("/")).map_err(|_| invalid())
    }

    /// `res://` path -> OS path under the root. Never fails; existence is not checked.
    pub fn globalize(&self, path: &ResPath) -> PathBuf {
        path.relative().split('/').filter(|segment| !segment.is_empty()).fold(self.root.clone(), |acc, segment| acc.join(segment))
    }

    /// Reads a project file by `res://` path.
    pub fn read_to_string(&self, path: &ResPath) -> crate::Result<String> {
        let os_path = self.globalize(path);
        std::fs::read_to_string(&os_path).map_err(|source| Error::Io { path: os_path, source })
    }

    /// True if the `res://` path names an existing file or directory.
    pub fn exists(&self, path: &ResPath) -> bool {
        self.globalize(path).exists()
    }
}

fn absolute(path: &Path) -> crate::Result<PathBuf> {
    std::path::absolute(path).map_err(|source| Error::Io { path: path.to_path_buf(), source })
}

/// `Ok(false)` only when `project.godot` is absent; anything else there is an error.
fn has_config(root: &Path) -> crate::Result<bool> {
    let path = root.join("project.godot");
    match std::fs::metadata(&path) {
        Ok(metadata) if metadata.is_file() => Ok(true),
        Ok(_) => Err(Error::ConfigNotAFile { path }),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            // A dangling symlink is an obstruction, not an absent marker.
            match std::fs::symlink_metadata(&path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
                _ => Err(Error::Io { path, source }),
            }
        }
        Err(source) => Err(Error::Io { path, source }),
    }
}
