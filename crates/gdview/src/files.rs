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
    todo!()
}

#[derive(Debug, Default)]
pub struct WalkResult {
    pub files: Vec<PathBuf>,
    pub symlinks: Vec<PathBuf>,
}
