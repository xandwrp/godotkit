// Acceptance tests for gdproject::config. No engine process or environment mutation.
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use gdproject::Error;
use gdproject::config::{CONFIG_FILE_NAME, Config, EngineConfig, SelectionSource, select_engine};
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
    for invalid in [
        missing.into_os_string(),
        OsString::new(),
        dir.path().as_os_str().to_owned(),
    ] {
        assert!(matches!(
            select_engine(dir.path(), None, Some(&invalid), Some(&config)),
            Err(Error::EngineNotFound(_))
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
    let cwd = std::env::current_dir().unwrap();
    let local = tempfile::Builder::new()
        .prefix("config-engine-")
        .tempfile_in(&cwd)
        .unwrap();
    let relative = Path::new(local.path().file_name().unwrap());
    let env = relative.as_os_str().to_owned();
    for (explicit, environment) in [(Some(relative), None), (None, Some(&env))] {
        assert_eq!(
            select_engine(&root, explicit, environment, None)
                .unwrap()
                .executable,
            fs::canonicalize(local.path()).unwrap()
        );
    }
}

#[test]
fn write_initial_refuses_to_overwrite_and_stores_relative_path() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("project");
    fs::create_dir(&root).unwrap();
    let engine = file(&dir.path().join("godot with spaces"));
    let path = Config::write_initial(&root, &engine).unwrap();
    assert_eq!(path, root.join(CONFIG_FILE_NAME));
    let original = fs::read(&path).unwrap();
    let config = Config::load(&root).unwrap().unwrap();
    assert_eq!(
        config.engine.as_ref().unwrap().executable,
        Path::new("../godot with spaces")
    );
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
    let nested = file(&root.join("engine"));
    Config::write_initial(&root, &nested).unwrap();
    assert_eq!(
        Config::load(&root)
            .unwrap()
            .unwrap()
            .engine
            .unwrap()
            .executable,
        Path::new("engine")
    );
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
