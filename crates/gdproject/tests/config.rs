// Acceptance tests for gdproject::config. No engine process or environment mutation:
// PATH lookups receive an explicit search path built from temp directories.
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use gdproject::Error;
use gdproject::config::{
    CONFIG_FILE_NAME, Config, EngineConfig, SelectionSource, select_engine,
    select_engine_with_search_path,
};
use tempfile::tempdir;

fn load_text(root: &Path, text: &str) -> gdproject::Result<Option<Config>> {
    fs::write(root.join(CONFIG_FILE_NAME), text).unwrap();
    Config::load(root)
}

fn config_message(error: Error, root: &Path) -> String {
    match error {
        Error::Config { path, message } => {
            assert_eq!(path, root.join(CONFIG_FILE_NAME));
            message
        }
        other => panic!("expected config error, got {other:?}"),
    }
}

fn engine_config(path: impl Into<PathBuf>) -> Config {
    Config {
        engine: Some(EngineConfig {
            executable: path.into(),
        }),
        ..Config::default()
    }
}

fn file(path: &Path) -> PathBuf {
    fs::write(path, "not an actual engine; selection must not execute it").unwrap();
    fs::canonicalize(path).unwrap()
}

#[test]
fn load_returns_none_when_missing_and_error_when_malformed() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    assert!(Config::load(root).unwrap().is_none());
    let config = load_text(root, "").unwrap().unwrap();
    assert!(config.engine.is_none());
    assert!(!config.check.strict_methods);
    assert!(config.check.ignore_import_errors.is_empty());
    assert!(config.run.checkpoint_adapter.is_none());
    for text in [
        "[engine",
        "engine = 42",
        "[engine]",
        "[engine]\nexecutable = ''",
        "[engine]\nexecutable = ' \t'",
        "[check]\nstrict_methods = 'yes'",
    ] {
        assert!(!config_message(load_text(root, text).unwrap_err(), root).is_empty());
    }
    fs::remove_file(root.join(CONFIG_FILE_NAME)).unwrap();
    fs::create_dir(root.join(CONFIG_FILE_NAME)).unwrap();
    assert!(
        matches!(Config::load(root), Err(Error::Io { path, .. }) if path == root.join(CONFIG_FILE_NAME))
    );
}

#[test]
fn unknown_keys_are_rejected_with_the_offending_path() {
    let dir = tempdir().unwrap();
    for (text, key) in [
        ("typo = true", "typo"),
        (
            "[engine]\nexecutable = 'godot'\nversion = '4'",
            "engine.version",
        ),
        ("[check]\nstrict_method = true", "check.strict_method"),
        ("[run]\nadapter = 'res://a.gd'", "run.adapter"),
        (
            "[check]\nignore_import_errors = [{message = 'ERROR: a', source = 'res://a.gd'}, {message = 'ERROR: b', source = 'res://b.gd', typo = true}]",
            "check.ignore_import_errors[1].typo",
        ),
        ("[check.extra]\nvalue = true", "check.extra"),
    ] {
        let message = config_message(load_text(dir.path(), text).unwrap_err(), dir.path());
        assert!(message.contains(key), "expected {key}: {message}");
    }
}

#[test]
fn ignore_rules_require_error_prefix_and_res_source() {
    let dir = tempdir().unwrap();
    let valid = "[[check.ignore_import_errors]]\nmessage = 'ERROR: known third-party noise'\nsource = 'res://addons/plugin.gd'";
    let config = load_text(dir.path(), valid).unwrap().unwrap();
    assert_eq!(
        config.check.ignore_import_errors[0].message,
        "ERROR: known third-party noise"
    );
    assert_eq!(
        config.check.ignore_import_errors[0].source.as_str(),
        "res://addons/plugin.gd"
    );
    for message in [
        "",
        "WARNING: noise",
        "error: noise",
        " ERROR: noise",
        "SCRIPT ERROR: noise",
    ] {
        let text = valid.replace("ERROR: known third-party noise", message);
        let error = config_message(load_text(dir.path(), &text).unwrap_err(), dir.path());
        assert!(
            error.contains("check.ignore_import_errors[0].message"),
            "{error}"
        );
    }
    for source in [
        "addons/plugin.gd",
        "user://plugin.gd",
        "res://",
        "res://../plugin.gd",
        "res://a\\b.gd",
        "res://a//b.gd",
        "res://.godot/a.gd",
    ] {
        let text = valid.replace("res://addons/plugin.gd", source);
        config_message(load_text(dir.path(), &text).unwrap_err(), dir.path());
    }
    let mut config = config;
    config.check.ignore_import_errors[0].message = "bad".into();
    assert!(config.validate(&dir.path().join(CONFIG_FILE_NAME)).is_err());
}

#[test]
fn checkpoint_adapter_must_be_res_gd_without_dot_dot() {
    let dir = tempdir().unwrap();
    for adapter in ["res://tools/checkpoints.gd", "res://tools/name..gd"] {
        let text = format!("[run]\ncheckpoint_adapter = '{adapter}'");
        assert_eq!(
            load_text(dir.path(), &text)
                .unwrap()
                .unwrap()
                .run
                .checkpoint_adapter
                .unwrap()
                .as_str(),
            adapter
        );
    }
    for adapter in [
        "tools/a.gd",
        "user://a.gd",
        "res://../a.gd",
        "res://tools/../a.gd",
        "res://a.cs",
        "res://a.GD",
        "res://",
        "res://a\\b.gd",
        "res://./a.gd",
    ] {
        let text = format!("[run]\ncheckpoint_adapter = '{adapter}'");
        config_message(load_text(dir.path(), &text).unwrap_err(), dir.path());
    }
    let mut config = Config::default();
    config.run.checkpoint_adapter = Some(gdview::ResPath::parse("res://a.cs").unwrap());
    assert!(config.validate(&dir.path().join(CONFIG_FILE_NAME)).is_err());
}

#[test]
fn select_engine_precedence_is_flag_then_env_then_config() {
    let dir = tempdir().unwrap();
    let flag = file(&dir.path().join("flag"));
    let env_path = file(&dir.path().join("env"));
    let configured = file(&dir.path().join("configured"));
    let env = env_path.clone().into_os_string();
    let config = engine_config(&configured);
    for (explicit, environment, expected, source) in [
        (
            Some(flag.as_path()),
            Some(&env),
            &flag,
            SelectionSource::CommandLine,
        ),
        (None, Some(&env), &env_path, SelectionSource::Environment),
        (None, None, &configured, SelectionSource::ProjectConfig),
    ] {
        let selected = select_engine(dir.path(), explicit, environment, Some(&config)).unwrap();
        assert_eq!(&selected.executable, expected);
        assert_eq!(selected.source, source);
    }
    assert!(matches!(
        select_engine(dir.path(), None, None, None),
        Err(Error::NoEngine)
    ));
    assert!(matches!(
        select_engine(dir.path(), None, None, Some(&Config::default())),
        Err(Error::NoEngine)
    ));
    let missing = dir.path().join("missing");
    assert!(
        matches!(select_engine(dir.path(), Some(&missing), Some(&env), Some(&config)), Err(Error::EngineNotFound(path)) if path == missing)
    );
    for invalid in [missing.into_os_string(), dir.path().as_os_str().to_owned()] {
        assert!(matches!(
            select_engine(dir.path(), None, Some(&invalid), Some(&config)),
            Err(Error::EngineNotFound(_))
        ));
    }
    // An empty or whitespace-only GDKIT_GODOT is unset: it falls back to gdkit.toml.
    for blank in ["", " ", "\t\n"] {
        let blank = OsString::from(blank);
        let selected = select_engine(dir.path(), None, Some(&blank), Some(&config)).unwrap();
        assert_eq!(selected.executable, configured);
        assert_eq!(selected.source, SelectionSource::ProjectConfig);
        assert!(matches!(
            select_engine(dir.path(), None, Some(&blank), None),
            Err(Error::NoEngine)
        ));
    }
    assert!(
        select_engine(
            dir.path(),
            Some(&flag),
            Some(&OsString::new()),
            Some(&engine_config("missing"))
        )
        .is_ok()
    );
}

#[test]
fn relative_engine_paths_resolve_against_the_config_file() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("project");
    fs::create_dir(&root).unwrap();
    let engine = file(&dir.path().join("godot"));
    let config = load_text(&root, "[engine]\nexecutable = '../godot'")
        .unwrap()
        .unwrap();
    assert_eq!(
        config.engine.as_ref().unwrap().executable,
        Path::new("../godot")
    );
    assert_eq!(
        select_engine(&root, None, None, Some(&config))
            .unwrap()
            .executable,
        engine
    );

    // CLI and environment paths are relative to the process, not the project.
    // Reach the temp file from the current directory without writing into it.
    let relative = relative_to(&engine, &fs::canonicalize(".").unwrap());
    assert!(relative.is_relative());
    let env = relative.as_os_str().to_owned();
    for (explicit, environment) in [(Some(relative.as_path()), None), (None, Some(&env))] {
        assert_eq!(
            select_engine(&root, explicit, environment, None)
                .unwrap()
                .executable,
            engine
        );
    }
}

/// `target` relative to `base`; both canonical and on the same volume.
fn relative_to(target: &Path, base: &Path) -> PathBuf {
    let target: Vec<_> = target.components().collect();
    let base: Vec<_> = base.components().collect();
    let common = target.iter().zip(&base).take_while(|(a, b)| a == b).count();
    let mut relative = PathBuf::new();
    for _ in &base[common..] {
        relative.push("..");
    }
    for part in &target[common..] {
        relative.push(part.as_os_str());
    }
    relative
}

fn executable(path: &Path) -> PathBuf {
    let path = file(path);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    path
}

fn search_path(directories: &[&Path]) -> OsString {
    std::env::join_paths(directories).unwrap()
}

#[test]
fn bare_names_are_looked_up_on_path_and_paths_with_separators_are_not() {
    let dir = tempdir().unwrap();
    let (first, second, empty) = (
        dir.path().join("first"),
        dir.path().join("second"),
        dir.path().join("empty"),
    );
    for directory in [&first, &second, &empty] {
        fs::create_dir(directory).unwrap();
    }
    let name = format!("godot{}", std::env::consts::EXE_SUFFIX);
    let shadowed = executable(&second.join(&name));
    let path = search_path(&[&empty, &second]);
    let config = engine_config(&name);
    let root = dir.path().join("project");
    fs::create_dir(&root).unwrap();
    let flag = PathBuf::from(&name);
    let env = OsString::from(&name);
    for (explicit, environment, config, source) in [
        (
            Some(flag.as_path()),
            None,
            None,
            SelectionSource::CommandLine,
        ),
        (None, Some(&env), None, SelectionSource::Environment),
        (None, None, Some(&config), SelectionSource::ProjectConfig),
    ] {
        let selected =
            select_engine_with_search_path(&root, explicit, environment, config, Some(&path))
                .unwrap();
        assert_eq!(selected.executable, shadowed);
        assert_eq!(selected.source, source);
    }
    // Earlier PATH entries win.
    let preferred = executable(&first.join(&name));
    let both = search_path(&[&first, &second]);
    assert_eq!(
        select_engine_with_search_path(&root, Some(&flag), None, None, Some(&both))
            .unwrap()
            .executable,
        preferred
    );
    // A bare name never falls back to the current directory or the project root.
    executable(&root.join(&name));
    for search in [None, Some(search_path(&[&empty]))] {
        assert!(matches!(
            select_engine_with_search_path(&root, Some(&flag), None, None, search.as_deref()),
            Err(Error::EngineNotOnPath(missing)) if missing == flag
        ));
        assert!(matches!(
            select_engine_with_search_path(&root, None, None, Some(&config), search.as_deref()),
            Err(Error::EngineNotOnPath(_))
        ));
    }
    // With a separator it is a path, even when PATH has a match.
    let dotted = PathBuf::from(format!("./{name}"));
    assert_eq!(
        select_engine_with_search_path(
            &root,
            None,
            None,
            Some(&engine_config(&dotted)),
            Some(&both)
        )
        .unwrap()
        .executable,
        fs::canonicalize(root.join(&name)).unwrap()
    );
    assert!(matches!(
        select_engine_with_search_path(&root, Some(&dotted), None, None, Some(&both)),
        Err(Error::EngineNotFound(_))
    ));
    // Directories named like the engine are skipped.
    fs::create_dir(empty.join(&name)).unwrap();
    assert_eq!(
        select_engine_with_search_path(&root, Some(&flag), None, None, Some(&path))
            .unwrap()
            .executable,
        shadowed
    );
}

#[cfg(unix)]
#[test]
fn path_lookup_skips_files_that_are_not_executable() {
    let dir = tempdir().unwrap();
    let (first, second) = (dir.path().join("first"), dir.path().join("second"));
    fs::create_dir(&first).unwrap();
    fs::create_dir(&second).unwrap();
    file(&first.join("godot"));
    let runnable = executable(&second.join("godot"));
    assert_eq!(
        select_engine_with_search_path(
            dir.path(),
            Some(Path::new("godot")),
            None,
            None,
            Some(&search_path(&[&first, &second]))
        )
        .unwrap()
        .executable,
        runnable
    );
}

fn stored(root: &Path) -> PathBuf {
    Config::load(root)
        .unwrap()
        .unwrap()
        .engine
        .unwrap()
        .executable
}

#[test]
fn write_initial_refuses_to_overwrite_and_stores_the_path_as_given() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("project");
    fs::create_dir(&root).unwrap();
    let engine = file(&dir.path().join("godot with spaces"));
    let path = Config::write_initial(&root, &engine).unwrap();
    assert_eq!(path, root.join(CONFIG_FILE_NAME));
    let original = fs::read(&path).unwrap();
    let config = Config::load(&root).unwrap().unwrap();
    // Outside the project: absolute, never `../`-relative to the project depth.
    assert_eq!(config.engine.as_ref().unwrap().executable, engine);
    assert_eq!(
        select_engine(&root, None, None, Some(&config))
            .unwrap()
            .executable,
        engine
    );
    assert!(
        matches!(Config::write_initial(&root, &engine), Err(Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::AlreadyExists)
    );
    assert_eq!(fs::read(&path).unwrap(), original);
    fs::write(&path, "user's malformed config").unwrap();
    assert!(Config::write_initial(&root, &engine).is_err());
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "user's malformed config"
    );
    fs::remove_file(&path).unwrap();
    assert!(Config::write_initial(&root, &root).is_err());
    assert!(!path.exists());
    assert!(Config::write_initial(&root, &root.join("missing")).is_err());
    assert!(!path.exists());
    // Inside the project: project-relative with `/`, and `./` at the root so the
    // stored value is not read back as a bare PATH name.
    let nested = file(&root.join("engine"));
    Config::write_initial(&root, &nested).unwrap();
    assert_eq!(stored(&root), Path::new("./engine"));
    assert_eq!(
        select_engine_with_search_path(
            &root,
            None,
            None,
            Config::load(&root).unwrap().as_ref(),
            None
        )
        .unwrap()
        .executable,
        nested
    );
    fs::remove_file(&path).unwrap();
    fs::create_dir_all(root.join("tools/bin")).unwrap();
    let deep = file(&root.join("tools/bin/godot"));
    Config::write_initial(&root, &root.join("tools/./bin/../bin/godot")).unwrap();
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .contains("executable = \"tools/bin/godot\"")
    );
    assert_eq!(
        select_engine(&root, None, None, Config::load(&root).unwrap().as_ref())
            .unwrap()
            .executable,
        deep
    );
}

#[test]
fn write_initial_stores_bare_names_bare_for_path_lookup() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("project");
    let bin = dir.path().join("bin");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&bin).unwrap();
    let name = format!("godot{}", std::env::consts::EXE_SUFFIX);
    let path = search_path(&[&bin]);
    assert!(matches!(
        Config::write_initial_with_search_path(&root, Path::new(&name), Some(&path)),
        Err(Error::EngineNotOnPath(_))
    ));
    assert!(!root.join(CONFIG_FILE_NAME).exists());
    let engine = executable(&bin.join(&name));
    Config::write_initial_with_search_path(&root, Path::new(&name), Some(&path)).unwrap();
    assert_eq!(stored(&root), Path::new(&name));
    assert_eq!(
        select_engine_with_search_path(
            &root,
            None,
            None,
            Config::load(&root).unwrap().as_ref(),
            Some(&path)
        )
        .unwrap()
        .executable,
        engine
    );
}

#[cfg(unix)]
#[test]
fn write_initial_keeps_symlinks_and_the_absolute_path_as_given() {
    use std::os::unix::fs::symlink;
    let dir = tempdir().unwrap();
    let root = dir.path().join("project");
    fs::create_dir(&root).unwrap();
    // A version-manager style shim: the stored path must keep following it.
    let versions = dir.path().join("versions");
    fs::create_dir(&versions).unwrap();
    let old = file(&versions.join("4.3"));
    let new = file(&versions.join("4.4"));
    let shim = dir.path().join("shim");
    symlink(&old, &shim).unwrap();
    Config::write_initial(&root, &shim).unwrap();
    assert_eq!(stored(&root), shim);
    fs::remove_file(&shim).unwrap();
    symlink(&new, &shim).unwrap();
    assert_eq!(
        select_engine(&root, None, None, Config::load(&root).unwrap().as_ref())
            .unwrap()
            .executable,
        new
    );
    // A project reached through a symlink still stores project-internal engines relatively.
    fs::remove_file(root.join(CONFIG_FILE_NAME)).unwrap();
    let alias = dir.path().join("alias");
    symlink(&root, &alias).unwrap();
    file(&root.join("godot"));
    Config::write_initial(&alias, &root.join("godot")).unwrap();
    assert_eq!(stored(&root), Path::new("./godot"));
}

#[cfg(unix)]
#[test]
fn symlinks_are_canonicalized_for_selection_but_never_overwritten() {
    use std::os::unix::fs::symlink;
    let dir = tempdir().unwrap();
    let engine = file(&dir.path().join("engine"));
    let alias = dir.path().join("alias");
    symlink(&engine, &alias).unwrap();
    assert_eq!(
        select_engine(dir.path(), Some(&alias), None, None)
            .unwrap()
            .executable,
        engine
    );
    let config_path = dir.path().join(CONFIG_FILE_NAME);
    let target = dir.path().join("target");
    symlink(&target, &config_path).unwrap();
    assert!(matches!(Config::load(dir.path()), Err(Error::Io { .. })));
    assert!(Config::write_initial(dir.path(), &engine).is_err());
    assert!(!target.exists());
    fs::write(&target, "").unwrap();
    assert!(Config::write_initial(dir.path(), &engine).is_err());
    assert_eq!(fs::read_to_string(&target).unwrap(), "");
    assert!(
        fs::symlink_metadata(&config_path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn concurrent_initialization_has_exactly_one_winner() {
    let dir = tempdir().unwrap();
    let engine = file(&dir.path().join("engine"));
    let barrier = std::sync::Barrier::new(4);
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    Config::write_initial(dir.path(), &engine)
                })
            })
            .collect();
        let results: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        for error in results.into_iter().filter_map(Result::err) {
            assert!(
                matches!(error, Error::Io { source, .. } if source.kind() == std::io::ErrorKind::AlreadyExists)
            );
        }
    });
    assert_eq!(
        select_engine(
            dir.path(),
            None,
            None,
            Config::load(dir.path()).unwrap().as_ref()
        )
        .unwrap()
        .executable,
        engine
    );
}
