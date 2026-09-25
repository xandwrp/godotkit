// Acceptance tests for gdproject::workspace. Offline unless prefixed real_engine_.
#![allow(unused)]

#[test]
fn open_requires_a_project_and_creates_state_dir_lazily() {
    let dir = tempfile::tempdir().unwrap();
    assert!(gdproject::Workspace::open(dir.path()).is_err());
    std::fs::write(dir.path().join("project.godot"), "config_version=5\n").unwrap();
    let workspace = gdproject::Workspace::open(dir.path()).unwrap();
    assert_eq!(workspace.root(), dir.path());
    assert_eq!(workspace.state_dir(), dir.path().join(".godot/gdkit"));
    assert!(!dir.path().join(".godot").exists(), "opening must not write");
}

#[test]
#[ignore = "scaffold"]
fn lock_is_exclusive_across_processes_and_released_on_drop() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn isolated_copy_full_excludes_dot_godot_dot_git_and_refuses_symlinks() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn isolated_copy_slice_rejects_dot_dot_absolute_and_dot_godot_and_keeps_relative_layout() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn isolated_copy_is_removed_on_drop() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn artifact_dir_names_are_unique_and_sortable() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn publish_new_file_is_atomic_and_never_overwrites() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn publish_new_file_falls_back_from_hard_link_to_rename_on_filesystems_without_links() {
    todo!()
}
