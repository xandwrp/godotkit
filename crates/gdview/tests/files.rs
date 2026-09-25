// Acceptance tests for gdview::files.

use std::fs;
use std::path::{Path, PathBuf};

use gdview::files::{FileQuery, walk};

fn tree(files: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for file in files {
        let path = dir.path().join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "").unwrap();
    }
    dir
}

fn relative(root: &Path, paths: &[PathBuf]) -> Vec<String> {
    paths.iter().map(|p| p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/")).collect()
}

fn listed(root: &Path, query: &FileQuery) -> Vec<String> {
    relative(root, &walk(root, query).unwrap().files)
}

#[test]
fn respects_nested_gitignore_and_negations() {
    let dir = tree(&["a.gd", "build/out.gd", "sub/keep.gd", "sub/drop.gd", "sub/drop_but_keep.gd"]);
    fs::write(dir.path().join(".gitignore"), "build/\n").unwrap();
    fs::write(dir.path().join("sub/.gitignore"), "drop*.gd\n!drop_but_keep.gd\n").unwrap();
    assert_eq!(
        listed(dir.path(), &FileQuery::with_extensions(["gd"])),
        ["a.gd", "sub/drop_but_keep.gd", "sub/keep.gd"]
    );
    let all = FileQuery { respect_gitignore: false, ..FileQuery::with_extensions(["gd"]) };
    assert_eq!(listed(dir.path(), &all).len(), 5);
}

#[test]
fn respects_git_info_exclude_and_global_excludes_without_git_binary() {
    let dir = tree(&["a.gd", "local_only.gd", "global_only.gd"]);
    fs::create_dir_all(dir.path().join(".git/info")).unwrap();
    fs::write(dir.path().join(".git/info/exclude"), "local_only.gd\n").unwrap();
    let config = tempfile::tempdir().unwrap();
    fs::create_dir_all(config.path().join("git")).unwrap();
    fs::write(config.path().join("git/ignore"), "global_only.gd\n").unwrap();
    // SAFETY: no other test in this binary reads or writes XDG_CONFIG_HOME.
    unsafe { std::env::set_var("XDG_CONFIG_HOME", config.path()) };
    let files = listed(dir.path(), &FileQuery::with_extensions(["gd"]));
    unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
    assert_eq!(files, ["a.gd"]);
}

#[test]
fn gdignore_prunes_directory_subtree() {
    let dir = tree(&["a.gd", "vendor/.gdignore", "vendor/lib.gd", "vendor/deep/x.gd"]);
    assert_eq!(listed(dir.path(), &FileQuery::with_extensions(["gd"])), ["a.gd"]);
    let query = FileQuery { respect_gdignore: false, ..FileQuery::with_extensions(["gd"]) };
    assert_eq!(listed(dir.path(), &query), ["a.gd", "vendor/deep/x.gd", "vendor/lib.gd"]);
}

#[test]
fn hidden_paths_and_dot_godot_are_excluded_by_default() {
    let dir = tree(&["a.gd", ".hidden/b.gd", ".c.gd", ".godot/editor/d.gd", ".git/e.gd", "addons/x/.godot/f.gd"]);
    assert_eq!(listed(dir.path(), &FileQuery::with_extensions(["gd"])), ["a.gd"]);
    let query = FileQuery { include_hidden: true, ..FileQuery::with_extensions(["gd"]) };
    assert_eq!(listed(dir.path(), &query), [".c.gd", ".hidden/b.gd", "a.gd"]);
}

#[test]
fn extension_match_is_case_insensitive_when_requested() {
    let dir = tree(&["a.gd", "B.GD", "c.Gd", "d.gdshader", "e"]);
    assert_eq!(listed(dir.path(), &FileQuery::with_extensions(["gd"])), ["B.GD", "a.gd", "c.Gd"]);
    let exact = FileQuery { case_insensitive_extensions: false, ..FileQuery::with_extensions(["gd"]) };
    assert_eq!(listed(dir.path(), &exact), ["a.gd"]);
    assert_eq!(listed(dir.path(), &FileQuery::default()).len(), 5);
}

#[test]
fn results_are_sorted_and_deterministic() {
    let dir = tree(&["z.gd", "a/z.gd", "a/b.gd", "m.gd", "a.gd"]);
    let first = listed(dir.path(), &FileQuery::default());
    // Component-wise path order: the directory `a` sorts before the file `a.gd`.
    assert_eq!(first, ["a/b.gd", "a/z.gd", "a.gd", "m.gd", "z.gd"]);
    assert_eq!(first, listed(dir.path(), &FileQuery::default()));
}

#[cfg(unix)]
#[test]
fn symlinks_are_reported_not_followed() {
    let dir = tree(&["real/a.gd", "b.gd"]);
    std::os::unix::fs::symlink(dir.path().join("real"), dir.path().join("linked_dir")).unwrap();
    std::os::unix::fs::symlink(dir.path().join("b.gd"), dir.path().join("linked.gd")).unwrap();
    let result = walk(dir.path(), &FileQuery::default()).unwrap();
    assert_eq!(relative(dir.path(), &result.files), ["b.gd", "real/a.gd"]);
    assert_eq!(relative(dir.path(), &result.symlinks), ["linked.gd", "linked_dir"]);
}
