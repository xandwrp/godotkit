//! A Godot project on disk. Discovery, file enumeration, and path mapping.
//!
//! # Tests (tests/project.rs)
//! - `open_requires_project_godot_as_regular_file`
//! - `open_and_discover_canonicalize_dot_dot_and_symlinked_roots`
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
    /// The root is canonicalized (see [`Project::root`]).
    pub fn open(root: impl AsRef<Path>) -> crate::Result<Self> {
        let absolute = absolute(root.as_ref())?;
        let root = match canonical(&absolute) {
            Ok(root) => root,
            Err(Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                return Err(Error::NotAProject { path: absolute });
            }
            Err(error) => return Err(error),
        };
        if !has_config(&root)? {
            return Err(Error::NotAProject { path: root });
        }
        Ok(Self { root })
    }

    /// Finds the nearest ancestor containing `project.godot`, walking the
    /// canonical (physical) path. A file starts the search at its parent.
    /// `from` must exist.
    pub fn discover(from: impl AsRef<Path>) -> crate::Result<Self> {
        let from = canonical(&absolute(from.as_ref())?)?;
        let metadata = std::fs::metadata(&from).map_err(|source| Error::Io {
            path: from.clone(),
            source,
        })?;
        let mut ancestors = from.ancestors();
        if !metadata.is_dir() {
            ancestors.next();
        }
        for root in ancestors {
            if has_config(root)? {
                return Ok(Self {
                    root: root.to_path_buf(),
                });
            }
        }
        Err(Error::NotAProject { path: from })
    }

    /// Canonical root: absolute, no `.`/`..`, and no symlinks in it or its
    /// ancestors, resolved once at open/discovery. The user-chosen root and its
    /// ancestors are trusted (so `../proj`, a symlinked checkout, or a symlinked
    /// `/home` all work); writers refuse symlinks only *inside* the root.
    /// A symlinked checkout therefore reports its target's spelling.
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn config_path(&self) -> PathBuf {
        self.root.join("project.godot")
    }

    /// Parses `project.godot` now.
    pub fn settings(&self) -> crate::Result<Settings> {
        let path = self.config_path();
        let source = std::fs::read_to_string(&path).map_err(|source| crate::Error::Io {
            path: path.clone(),
            source,
        })?;
        Settings::parse(&source).map_err(|error| error.with_path(path))
    }

    /// Enumerates project files. See [`FileQuery`] for ignore semantics.
    pub fn files(&self, query: &FileQuery) -> crate::Result<Vec<PathBuf>> {
        Ok(crate::files::walk(&self.root, query)?.files)
    }

    /// OS path inside the root -> `res://` path. Errors if outside the root or under `.godot`.
    /// Relative paths are taken relative to the root. The path is not required to exist.
    /// An absolute path spelled through a symlinked root or ancestor is mapped by
    /// resolving its deepest existing ancestor.
    pub fn localize(&self, path: &Path) -> crate::Result<ResPath> {
        let invalid = || Error::InvalidResPath(path.display().to_string());
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        };
        let resolved;
        let relative = match absolute.strip_prefix(&self.root) {
            Ok(relative) => relative,
            Err(_) => {
                resolved = resolve_existing_prefix(&absolute).ok_or_else(invalid)?;
                resolved.strip_prefix(&self.root).map_err(|_| invalid())?
            }
        };
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
        path.relative()
            .split('/')
            .filter(|segment| !segment.is_empty())
            .fold(self.root.clone(), |acc, segment| acc.join(segment))
    }

    /// Reads a project file by `res://` path.
    pub fn read_to_string(&self, path: &ResPath) -> crate::Result<String> {
        let os_path = self.globalize(path);
        std::fs::read_to_string(&os_path).map_err(|source| Error::Io {
            path: os_path,
            source,
        })
    }

    /// True if the `res://` path names an existing file or directory.
    pub fn exists(&self, path: &ResPath) -> bool {
        self.globalize(path).exists()
    }
}

fn absolute(path: &Path) -> crate::Result<PathBuf> {
    std::path::absolute(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// `fs::canonicalize`, keeping plain drive paths free of Windows' verbatim prefix.
fn canonical(path: &Path) -> crate::Result<PathBuf> {
    let canonical = std::fs::canonicalize(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(simplify_verbatim(canonical))
}

#[cfg(windows)]
fn simplify_verbatim(path: PathBuf) -> PathBuf {
    use std::path::Prefix;
    let Some(Component::Prefix(prefix)) = path.components().next() else {
        return path;
    };
    if !matches!(prefix.kind(), Prefix::VerbatimDisk(_)) {
        return path;
    }
    // `\\?\C:\dir` -> `C:\dir`, only while the plain form stays under MAX_PATH.
    match path.to_str().and_then(|text| text.strip_prefix(r"\\?\")) {
        Some(plain) if plain.len() < 260 => PathBuf::from(plain),
        _ => path,
    }
}

#[cfg(not(windows))]
fn simplify_verbatim(path: PathBuf) -> PathBuf {
    path
}

/// Canonicalizes the deepest existing ancestor of an absolute path and re-appends
/// the missing tail. `None` when nothing resolves or the tail contains `..`.
fn resolve_existing_prefix(path: &Path) -> Option<PathBuf> {
    let mut tail = Vec::new();
    for ancestor in path.ancestors() {
        if let Ok(mut resolved) = canonical(ancestor) {
            resolved.extend(tail.iter().rev());
            return Some(resolved);
        }
        match ancestor.components().next_back()? {
            Component::Normal(name) => tail.push(name),
            _ => return None,
        }
    }
    None
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
