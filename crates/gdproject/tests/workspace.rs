use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use gdproject::workspace::{IsolatedCopy, publish_new_file};
use gdproject::{Error, Workspace};

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("project.godot"),
        "config_version=5\n; authored\n",
    )
    .unwrap();
    dir
}

fn put(root: &Path, name: &str, bytes: &[u8]) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

#[test]
fn open_requires_a_project_and_creates_state_dir_lazily() {
    let dir = tempfile::tempdir().unwrap();
    assert!(Workspace::open(dir.path()).is_err());
    fs::write(dir.path().join("project.godot"), "config_version=5\n").unwrap();
    let workspace = Workspace::open(dir.path()).unwrap();
    assert_eq!(workspace.root(), dir.path());
    assert_eq!(workspace.state_dir(), dir.path().join(".godot/gdkit"));
    assert_eq!(
        workspace.probe_cache_path(),
        workspace.state_dir().join("engine-probe.json")
    );
    assert_eq!(
        workspace.api_cache_path(),
        workspace.state_dir().join("api-index.json")
    );
    assert!(
        !dir.path().join(".godot").exists(),
        "opening must not write"
    );
}

// Re-exec this test with a private working directory, not global environment state.
#[test]
fn lock_child_process() {
    let root = std::env::current_dir().unwrap();
    let Ok(expected) = fs::read_to_string(root.join("lock-child-expectation")) else {
        return;
    };
    let workspace = Workspace::open(&root).unwrap();
    match expected.as_str() {
        "locked" => assert!(matches!(workspace.lock(), Err(Error::Locked(_)))),
        "free" => drop(workspace.lock().unwrap()),
        other => panic!("unexpected child mode: {other}"),
    }
}

fn run_lock_child(root: &Path, expectation: &str) {
    fs::write(root.join("lock-child-expectation"), expectation).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "lock_child_process", "--nocapture"])
        .current_dir(root)
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("lock child timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn lock_is_exclusive_across_processes_and_released_on_drop() {
    let dir = project();
    let workspace = Workspace::open(dir.path()).unwrap();
    let lock = workspace.lock().unwrap();
    assert!(matches!(workspace.lock(), Err(Error::Locked(_))));
    run_lock_child(dir.path(), "locked");
    drop(lock);
    run_lock_child(dir.path(), "free");
    let lock = workspace.lock().unwrap();
    run_lock_child(dir.path(), "locked");
    drop(lock);
    assert_eq!(
        fs::read(dir.path().join("project.godot")).unwrap(),
        b"config_version=5\n; authored\n"
    );
}

#[test]
fn isolated_copy_full_excludes_dot_godot_dot_git_and_refuses_symlinks() {
    let dir = project();
    put(dir.path(), "scripts/main.gd", b"extends Node\n");
    put(dir.path(), "scripts/main.gd.uid", b"uid://abc\n");
    put(dir.path(), ".godot/imported/a", b"cache");
    put(dir.path(), ".git/config", b"git");
    put(dir.path(), "nested/.godot/a", b"cache");
    put(dir.path(), "nested/.git/a", b"git");
    put(dir.path(), ".hidden", b"authored");
    fs::create_dir(dir.path().join("empty")).unwrap();
    let workspace = Workspace::open(dir.path()).unwrap();
    let copy = IsolatedCopy::full(workspace.project()).unwrap();
    assert!(!copy.path.starts_with(dir.path()));
    assert_eq!(
        fs::read(copy.path.join("project.godot")).unwrap(),
        fs::read(dir.path().join("project.godot")).unwrap()
    );
    for path in ["scripts/main.gd", "scripts/main.gd.uid", ".hidden"] {
        assert_eq!(
            fs::read(copy.path.join(path)).unwrap(),
            fs::read(dir.path().join(path)).unwrap()
        );
    }
    for path in [".godot", ".git", "nested/.godot", "nested/.git"] {
        assert!(!copy.path.join(path).exists());
    }
    assert!(copy.path.join("empty").is_dir());
    fs::write(copy.path.join("scripts/main.gd"), b"changed").unwrap();
    assert_eq!(
        fs::read(dir.path().join("scripts/main.gd")).unwrap(),
        b"extends Node\n"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        // Excluded links are skipped, not traversed or copied.
        fs::remove_dir_all(dir.path().join(".git")).unwrap();
        symlink("missing", dir.path().join(".git")).unwrap();
        IsolatedCopy::full(workspace.project()).unwrap();
        for target in ["scripts/main.gd", "scripts", "missing"] {
            symlink(target, dir.path().join("link")).unwrap();
            assert!(IsolatedCopy::full(workspace.project()).is_err());
            fs::remove_file(dir.path().join("link")).unwrap();
        }
    }
}

#[test]
fn isolated_copy_slice_rejects_dot_dot_absolute_and_dot_godot_and_keeps_relative_layout() {
    let dir = project();
    put(dir.path(), "assets/sub/a.bin", &[0, 1, 255]);
    put(dir.path(), "assets/b.txt", b"b");
    put(dir.path(), "assets/.godot/cache", b"cache");
    put(dir.path(), "unselected.txt", b"private");
    let workspace = Workspace::open(dir.path()).unwrap();
    for selection in [
        "",
        ".",
        "..",
        "../escape",
        "assets/../../escape",
        "/absolute",
        ".godot",
        ".godot/cache",
        "assets/.godot",
        ".git/config",
        "C:\\absolute",
        "..\\escape",
        "missing",
    ] {
        assert!(
            IsolatedCopy::slice(workspace.project(), &[selection.into()]).is_err(),
            "accepted {selection:?}"
        );
    }
    let copy = IsolatedCopy::slice(workspace.project(), &["assets/sub/a.bin".into()]).unwrap();
    assert_eq!(
        fs::read(copy.path.join("assets/sub/a.bin")).unwrap(),
        [0, 1, 255]
    );
    assert_eq!(
        fs::read(copy.path.join("project.godot")).unwrap(),
        b"config_version=5\n"
    );
    assert!(!copy.path.join("assets/b.txt").exists());
    assert!(!copy.path.join("unselected.txt").exists());
    let copy = IsolatedCopy::slice(
        workspace.project(),
        &[
            "assets/sub/a.bin".into(),
            "assets".into(),
            "assets".into(),
            "project.godot".into(),
        ],
    )
    .unwrap();
    assert_eq!(fs::read(copy.path.join("assets/b.txt")).unwrap(), b"b");
    assert!(!copy.path.join("assets/.godot").exists());
    assert_eq!(
        fs::read(copy.path.join("project.godot")).unwrap(),
        b"config_version=5\n; authored\n"
    );
    assert_eq!(
        fs::read(
            IsolatedCopy::slice(workspace.project(), &[])
                .unwrap()
                .path
                .join("project.godot")
        )
        .unwrap(),
        b"config_version=5\n"
    );

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("assets", dir.path().join("alias")).unwrap();
        for path in ["alias", "alias/sub/a.bin"] {
            assert!(IsolatedCopy::slice(workspace.project(), &[path.into()]).is_err());
        }
        std::os::unix::fs::symlink("missing", dir.path().join("assets/broken")).unwrap();
        assert!(IsolatedCopy::slice(workspace.project(), &["assets".into()]).is_err());
    }
}

#[test]
fn isolated_copy_is_removed_on_drop() {
    let dir = project();
    put(dir.path(), "a/b", b"original");
    let workspace = Workspace::open(dir.path()).unwrap();
    for copy in [
        IsolatedCopy::empty().unwrap(),
        IsolatedCopy::full(workspace.project()).unwrap(),
        IsolatedCopy::slice(workspace.project(), &["a".into()]).unwrap(),
    ] {
        let path = copy.path.clone();
        assert!(path.join("project.godot").is_file());
        put(&path, ".godot/generated", b"engine output");
        drop(copy);
        assert!(!path.exists());
    }
    assert_eq!(fs::read(dir.path().join("a/b")).unwrap(), b"original");
    assert!(!dir.path().join(".godot").exists());
}

#[cfg(unix)]
#[test]
fn copies_reject_special_files_and_keep_read_only_sources_writable_in_scratch() {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;
    let dir = project();
    put(dir.path(), "readonly", b"authored");
    fs::set_permissions(
        dir.path().join("readonly"),
        fs::Permissions::from_mode(0o444),
    )
    .unwrap();
    for (name, mode) in [("executable", 0o555), ("privileged", 0o7755)] {
        put(dir.path(), name, b"#!/bin/sh\nexit 0\n");
        fs::set_permissions(dir.path().join(name), fs::Permissions::from_mode(mode)).unwrap();
    }
    let workspace = Workspace::open(dir.path()).unwrap();
    for copy in [
        IsolatedCopy::full(workspace.project()).unwrap(),
        IsolatedCopy::slice(
            workspace.project(),
            &["executable".into(), "privileged".into()],
        )
        .unwrap(),
    ] {
        for (name, source_mode) in [("executable", 0o555), ("privileged", 0o7755)] {
            let copied_mode = fs::metadata(copy.path.join(name))
                .unwrap()
                .permissions()
                .mode()
                & 0o7777;
            assert_eq!(copied_mode, (source_mode & 0o777) | 0o600);
            assert_eq!(
                fs::metadata(dir.path().join(name))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o7777,
                source_mode
            );
        }
    }
    let copy = IsolatedCopy::full(workspace.project()).unwrap();
    assert_ne!(
        fs::metadata(copy.path.join("readonly"))
            .unwrap()
            .permissions()
            .mode()
            & 0o200,
        0
    );
    fs::write(copy.path.join("readonly"), b"changed").unwrap();
    assert_eq!(fs::read(dir.path().join("readonly")).unwrap(), b"authored");
    assert_eq!(
        fs::metadata(dir.path().join("readonly"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o444
    );
    let _socket = UnixListener::bind(dir.path().join("socket")).unwrap();
    assert!(IsolatedCopy::full(workspace.project()).is_err());
    assert!(IsolatedCopy::slice(workspace.project(), &["socket".into()]).is_err());
}

#[test]
fn artifact_dir_names_are_unique_and_sortable() {
    let dir = project();
    let workspace = Workspace::open(dir.path()).unwrap();
    let mut paths = Vec::new();
    for _ in 0..32 {
        let artifact = workspace.new_artifact_dir("check").unwrap();
        assert_eq!(
            artifact.path.parent().unwrap(),
            workspace.state_dir().join("artifacts/check")
        );
        let name = artifact.path.file_name().unwrap().to_str().unwrap();
        assert_eq!(name.split('-').count(), 3);
        for part in name.split('-') {
            part.parse::<u128>().unwrap();
        }
        paths.push(artifact.path.clone());
        let written = artifact.write("nested/result.json", b"{}").unwrap();
        assert_eq!(fs::read(&written).unwrap(), b"{}");
        assert!(artifact.write("nested/result.json", b"overwrite").is_err());
        for name in [
            "",
            ".",
            "../escape",
            "/escape",
            "nested/../../escape",
            "C:\\escape",
        ] {
            assert!(artifact.write(name, b"bad").is_err());
        }
    }
    assert!(paths.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(
        paths.iter().all(|path| path.is_dir()),
        "artifacts persist after drop"
    );
    for kind in [
        "",
        ".",
        "..",
        "../escape",
        "/escape",
        "nested/kind",
        "C:\\escape",
    ] {
        assert!(workspace.new_artifact_dir(kind).is_err());
    }
}

#[cfg(unix)]
#[test]
fn state_artifact_and_publication_paths_refuse_symlinks() {
    use std::os::unix::fs::symlink;
    let dir = project();
    let outside = tempfile::tempdir().unwrap();
    let workspace = Workspace::open(dir.path()).unwrap();
    symlink(outside.path(), dir.path().join(".godot")).unwrap();
    assert!(workspace.lock().is_err());
    assert!(workspace.new_artifact_dir("check").is_err());
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    fs::remove_file(dir.path().join(".godot")).unwrap();
    let artifact = workspace.new_artifact_dir("check").unwrap();
    symlink(outside.path(), artifact.path.join("escape")).unwrap();
    assert!(artifact.write("escape/new", b"bad").is_err());
    symlink(
        outside.path().join("missing"),
        workspace.state_dir().join("lock"),
    )
    .unwrap();
    assert!(workspace.lock().is_err());
    let staged = dir.path().join("staged");
    fs::write(&staged, b"payload").unwrap();
    let link = dir.path().join("link");
    symlink(&staged, &link).unwrap();
    assert!(publish_new_file(&link, &dir.path().join("destination")).is_err());
    assert!(publish_new_file(&staged, &artifact.path.join("escape/destination")).is_err());
    assert!(publish_new_file(&staged, &link).is_err());
    fs::remove_file(&link).unwrap();
    symlink(outside.path().join("missing"), &link).unwrap();
    assert!(publish_new_file(&staged, &link).is_err());
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    assert_eq!(fs::read(staged).unwrap(), b"payload");
}

#[test]
fn publish_new_file_is_atomic_and_never_overwrites() {
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("published");
    let staged = dir.path().join("staged");
    fs::write(&staged, b"complete payload").unwrap();
    publish_new_file(&staged, &destination).unwrap();
    assert!(!staged.exists());
    assert_eq!(fs::read(&destination).unwrap(), b"complete payload");
    fs::write(&staged, b"replacement").unwrap();
    assert!(publish_new_file(&staged, &destination).is_err());
    assert_eq!(fs::read(&destination).unwrap(), b"complete payload");
    assert_eq!(fs::read(&staged).unwrap(), b"replacement");
    assert!(publish_new_file(&staged, &staged).is_err());
    assert!(publish_new_file(dir.path(), &dir.path().join("directory-copy")).is_err());
    assert!(publish_new_file(&staged, &dir.path().join("missing/destination")).is_err());

    let destination = Arc::new(dir.path().join("raced"));
    let barrier = Arc::new(Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let staged = dir.path().join(format!("stage-{i}"));
            let payload = vec![i; 64 * 1024];
            fs::write(&staged, &payload).unwrap();
            let barrier = barrier.clone();
            let destination = destination.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let result = publish_new_file(&staged, &destination);
                let visible = fs::read(destination.as_ref()).unwrap();
                assert_eq!(visible.len(), payload.len());
                assert!(visible.iter().all(|byte| *byte == visible[0]));
                if result.is_ok() {
                    assert!(!staged.exists());
                    assert_eq!(visible, payload);
                    true
                } else {
                    assert_eq!(fs::read(staged).unwrap(), payload);
                    false
                }
            })
        })
        .collect();
    assert_eq!(
        handles
            .into_iter()
            .map(|handle| usize::from(handle.join().unwrap()))
            .sum::<usize>(),
        1
    );
}

// The unsupported-link acceptance gate lives in workspace.rs's private unit
// tests, allowing deterministic injection without global hooks or special mounts.
