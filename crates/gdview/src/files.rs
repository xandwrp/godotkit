//! Git-aware file enumeration.
//!
//! # Tests (tests/files.rs)
//! - `respects_nested_gitignore_and_negations`
//! - `respects_git_info_exclude_and_global_excludes_without_git_binary`
//! - `gdignore_prunes_directory_subtree`
//! - `hidden_paths_and_dot_godot_are_excluded_by_default`
//! - `extension_match_is_case_insensitive_when_requested`
//! - `results_are_sorted_and_deterministic`
//! - `symlinks_are_reported_not_followed`

use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct FileQuery {
    /// Lowercase extensions without the dot. Empty means all files.
    pub extensions: Vec<String>,
    pub case_insensitive_extensions: bool,
    pub respect_gitignore: bool,
    pub respect_gdignore: bool,
    pub include_hidden: bool,
}

impl Default for FileQuery {
    fn default() -> Self {
        Self {
            extensions: Vec::new(),
            case_insensitive_extensions: true,
            respect_gitignore: true,
            respect_gdignore: true,
            include_hidden: false,
        }
    }
}

impl FileQuery {
    pub fn with_extensions<I: IntoIterator<Item = S>, S: Into<String>>(extensions: I) -> Self {
        Self {
            extensions: extensions.into_iter().map(Into::into).collect(),
            ..Self::default()
        }
    }
}

/// Walks `root` with the query. `.godot` and `.git` are always pruned.
/// Symlinks are never followed; they are returned in `WalkResult::symlinks`
/// so callers can decide whether that is an error.
pub fn walk(root: &Path, query: &FileQuery) -> crate::Result<WalkResult> {
    let io_error = |path: &Path, source: std::io::Error| crate::Error::Io {
        path: path.to_owned(),
        source,
    };
    let metadata = std::fs::metadata(root).map_err(|source| io_error(root, source))?;
    if !metadata.is_dir() {
        return Err(io_error(
            root,
            std::io::Error::new(std::io::ErrorKind::NotADirectory, "not a directory"),
        ));
    }
    let include_hidden = query.include_hidden;
    let respect_gdignore = query.respect_gdignore;
    let walker = ignore::WalkBuilder::new(root)
        .hidden(false)
        .parents(query.respect_gitignore)
        .git_ignore(query.respect_gitignore)
        .git_global(query.respect_gitignore)
        .git_exclude(query.respect_gitignore)
        .require_git(false)
        .ignore(false)
        .follow_links(false)
        .filter_entry(move |entry| {
            if entry.depth() == 0 {
                return true;
            }
            let name = entry.file_name().to_string_lossy();
            let is_dir = entry.file_type().is_some_and(|kind| kind.is_dir());
            if is_dir && (name == ".godot" || name == ".git") {
                return false;
            }
            if !include_hidden && name.starts_with('.') {
                return false;
            }
            !(respect_gdignore && is_dir && entry.path().join(".gdignore").is_file())
        })
        .build();
    let mut result = WalkResult::default();
    for entry in walker {
        let entry = entry.map_err(|error| io_error(root, std::io::Error::other(error)))?;
        let Some(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            result.symlinks.push(entry.into_path());
        } else if kind.is_file() && query.matches_extension(entry.path()) {
            result.files.push(entry.into_path());
        }
    }
    result.files.sort();
    result.symlinks.sort();
    Ok(result)
}

impl FileQuery {
    fn matches_extension(&self, path: &Path) -> bool {
        if self.extensions.is_empty() {
            return true;
        }
        let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
            return false;
        };
        self.extensions.iter().any(|wanted| {
            if self.case_insensitive_extensions {
                extension.eq_ignore_ascii_case(wanted)
            } else {
                extension == wanted
            }
        })
    }
}

#[derive(Debug, Default)]
pub struct WalkResult {
    pub files: Vec<PathBuf>,
    pub symlinks: Vec<PathBuf>,
}
