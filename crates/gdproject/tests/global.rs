// Acceptance tests for gdproject::global. No environment mutation: `locate`
// takes a lookup function and PATH lookups receive an explicit search path.
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use gdproject::Error;
use gdproject::config::{Config, EngineConfig, SelectionSource, select_engine};
use gdproject::global::{GLOBAL_CONFIG_FILE_NAME, GlobalConfig};
use tempfile::tempdir;

fn file(path: &Path) -> PathBuf {
    fs::write(path, "not an actual engine; selection must not execute it").unwrap();
    fs::canonicalize(path).unwrap()
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

fn global(path: &Path, executable: impl Into<PathBuf>) -> GlobalConfig {
    GlobalConfig {
        path: path.to_owned(),
        engine: Some(EngineConfig {
            executable: executable.into(),
        }),
    }
}

fn stored(path: &Path) -> Option<PathBuf> {
    GlobalConfig::load(path)
        .unwrap()
        .unwrap()
        .engine
        .map(|engine| engine.executable)
}

#[test]
fn locate_prefers_gdkit_config_dir_then_the_platform_default() {
    let env = |pairs: &'static [(&'static str, &'static str)]| {
        move |name: &str| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| OsString::from(value))
        }
    };
    assert_eq!(
        GlobalConfig::locate(env(&[
            ("GDKIT_CONFIG_DIR", "/custom"),
            ("XDG_CONFIG_HOME", "/xdg"),
            ("HOME", "/home/me"),
            ("APPDATA", "C:\\AppData"),
        ])),
        Some(Path::new("/custom").join(GLOBAL_CONFIG_FILE_NAME))
    );
    if cfg!(windows) {
        assert_eq!(
            GlobalConfig::locate(env(&[("GDKIT_CONFIG_DIR", ""), ("APPDATA", "C:\\AppData")])),
            Some(Path::new("C:\\AppData\\gdkit").join(GLOBAL_CONFIG_FILE_NAME))
        );
        assert_eq!(GlobalConfig::locate(env(&[("HOME", "/home/me")])), None);
    } else {
        assert_eq!(
            GlobalConfig::locate(env(&[
                ("GDKIT_CONFIG_DIR", ""),
                ("XDG_CONFIG_HOME", "/xdg"),
                ("HOME", "/home/me"),
            ])),
            Some(Path::new("/xdg/gdkit").join(GLOBAL_CONFIG_FILE_NAME))
        );
        // The XDG spec says to ignore a relative XDG_CONFIG_HOME.
        for xdg in ["", "relative"] {
            let lookup = move |name: &str| match name {
                "XDG_CONFIG_HOME" => Some(OsString::from(xdg)),
                "HOME" => Some(OsString::from("/home/me")),
                _ => None,
            };
            assert_eq!(
                GlobalConfig::locate(lookup),
                Some(Path::new("/home/me/.config/gdkit").join(GLOBAL_CONFIG_FILE_NAME))
            );
        }
        assert_eq!(GlobalConfig::locate(env(&[("HOME", "")])), None);
    }
    assert_eq!(GlobalConfig::locate(|_| None), None);
}

#[test]
fn load_returns_none_when_missing_and_rejects_unknown_keys() {
    let dir = tempdir().unwrap();
    let path = dir.path().join(GLOBAL_CONFIG_FILE_NAME);
    assert!(GlobalConfig::load(&path).unwrap().is_none());
    fs::write(&path, "").unwrap();
    let config = GlobalConfig::load(&path).unwrap().unwrap();
    assert!(config.engine.is_none());
    assert_eq!(config.path, path);
    fs::write(&path, "[engine]\nexecutable = '/usr/bin/godot'\n").unwrap();
    assert_eq!(stored(&path).unwrap(), Path::new("/usr/bin/godot"));
    for (text, needle) in [
        ("[engine", "engine"),
        ("engine = 42", "engine"),
        ("[engine]\nexecutable = ' '", "must not be empty"),
        (
            "[engine]\nexecutable = '/g'\nversion = '4'",
            "engine.version",
        ),
        ("[check]\nstrict_methods = true", "`check`"),
    ] {
        fs::write(&path, text).unwrap();
        match GlobalConfig::load(&path).unwrap_err() {
            Error::Config {
                path: reported,
                message,
            } => {
                assert_eq!(reported, path);
                assert!(message.contains(needle), "{text:?}: {message}");
            }
            other => panic!("expected config error, got {other:?}"),
        }
    }
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(matches!(GlobalConfig::load(&path), Err(Error::Io { .. })));
    #[cfg(unix)]
    {
        let dangling = dir.path().join("dangling.toml");
        std::os::unix::fs::symlink(dir.path().join("gone"), &dangling).unwrap();
        assert!(matches!(
            GlobalConfig::load(&dangling),
            Err(Error::Io { .. })
        ));
    }
}

#[test]
fn global_default_applies_only_when_nothing_else_selects_an_engine() {
    let dir = tempdir().unwrap();
    let flag = file(&dir.path().join("flag"));
    let env = file(&dir.path().join("env")).into_os_string();
    let project = file(&dir.path().join("project"));
    let default = file(&dir.path().join("default"));
    let config = Config {
        engine: Some(EngineConfig {
            executable: project.clone(),
        }),
        ..Config::default()
    };
    let global = global(&dir.path().join(GLOBAL_CONFIG_FILE_NAME), &default);
    let root = dir.path();
    let select = |explicit, env, config| select_engine(root, explicit, env, config, Some(&global));
    let selected = select(Some(flag.as_path()), Some(&env), Some(&config)).unwrap();
    assert_eq!(selected.source, SelectionSource::CommandLine);
    let selected = select(None, Some(&env), Some(&config)).unwrap();
    assert_eq!(selected.source, SelectionSource::Environment);
    let selected = select(None, None, Some(&config)).unwrap();
    assert_eq!(
        (selected.executable, selected.source),
        (project, SelectionSource::ProjectConfig)
    );
    // A gdkit.toml without [engine] (what `init` writes) and a blank GDKIT_GODOT
    // both fall through to the global default.
    let blank = OsString::from(" ");
    for config in [None, Some(&Config::default())] {
        let selected = select(None, Some(&blank), config).unwrap();
        assert_eq!(
            (selected.executable, selected.source),
            (default.clone(), SelectionSource::GlobalConfig)
        );
    }
    let empty = GlobalConfig {
        path: global.path.clone(),
        engine: None,
    };
    assert!(matches!(
        select_engine(root, None, None, None, Some(&empty)),
        Err(Error::NoEngine)
    ));
    let missing = self::global(&global.path, dir.path().join("missing"));
    assert!(matches!(
        select_engine(root, None, None, None, Some(&missing)),
        Err(Error::EngineNotFound(_))
    ));
}

#[test]
fn relative_executables_resolve_against_the_config_directory() {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().join("config");
    let project = dir.path().join("project");
    fs::create_dir_all(config_dir.join("engines")).unwrap();
    fs::create_dir(&project).unwrap();
    let engine = file(&config_dir.join("engines/godot"));
    let path = config_dir.join(GLOBAL_CONFIG_FILE_NAME);
    fs::write(&path, "[engine]\nexecutable = 'engines/godot'\n").unwrap();
    let global = GlobalConfig::load(&path).unwrap().unwrap();
    assert_eq!(
        select_engine(&project, None, None, None, Some(&global))
            .unwrap()
            .executable,
        engine
    );
}

#[test]
fn set_creates_the_directory_and_stores_the_path_as_given() {
    let dir = tempdir().unwrap();
    let path = dir
        .path()
        .join("nested/gdkit")
        .join(GLOBAL_CONFIG_FILE_NAME);
    let engine = file(&dir.path().join("godot with spaces"));
    assert_eq!(
        GlobalConfig::set_engine(&path, &engine).unwrap(),
        engine.to_str().unwrap()
    );
    assert_eq!(stored(&path).unwrap(), engine);
    // No temp files left beside the config.
    let entries: Vec<_> = fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(entries, [GLOBAL_CONFIG_FILE_NAME]);
    // Bare names stay bare for PATH lookup at selection time.
    let bin = dir.path().join("bin");
    fs::create_dir(&bin).unwrap();
    executable(&bin.join("godot-4"));
    let search = std::env::join_paths([&bin]).unwrap();
    assert_eq!(
        GlobalConfig::set_engine_with_search_path(&path, Path::new("godot-4"), Some(&search))
            .unwrap(),
        "godot-4"
    );
    assert_eq!(stored(&path).unwrap(), Path::new("godot-4"));
    assert!(matches!(
        GlobalConfig::set_engine_with_search_path(&path, Path::new("godot-5"), Some(&search)),
        Err(Error::EngineNotOnPath(_))
    ));
    assert_eq!(stored(&path).unwrap(), Path::new("godot-4"));
    #[cfg(unix)]
    {
        // Absolute symlinks are stored as given, so a version-manager shim keeps following upgrades.
        let shim = dir.path().join("shim");
        std::os::unix::fs::symlink(&engine, &shim).unwrap();
        GlobalConfig::set_engine(&path, &shim).unwrap();
        assert_eq!(stored(&path).unwrap(), shim);
    }
}

#[test]
fn set_and_unset_keep_comments_and_formatting() {
    let dir = tempdir().unwrap();
    let path = dir.path().join(GLOBAL_CONFIG_FILE_NAME);
    let old = file(&dir.path().join("old"));
    let new = file(&dir.path().join("new"));
    let text = format!(
        "# my machine's defaults\n\n[engine]\n# the stable build\nexecutable = {:?} # trailing\n",
        old.to_str().unwrap()
    );
    fs::write(&path, &text).unwrap();
    GlobalConfig::set_engine(&path, &new).unwrap();
    let edited = fs::read_to_string(&path).unwrap();
    assert!(
        edited.starts_with("# my machine's defaults\n\n[engine]\n# the stable build\n"),
        "{edited}"
    );
    assert!(edited.contains(" # trailing\n"), "{edited}");
    assert!(edited.contains(new.to_str().unwrap()), "{edited}");
    assert!(!edited.contains(old.to_str().unwrap()), "{edited}");
    assert!(GlobalConfig::unset_engine(&path).unwrap());
    let edited = fs::read_to_string(&path).unwrap();
    assert!(edited.starts_with("# my machine's defaults\n"), "{edited}");
    assert!(!edited.contains("[engine]"), "{edited}");
    assert!(GlobalConfig::load(&path).unwrap().unwrap().engine.is_none());
    // Unsetting what isn't set leaves the file, or its absence, alone.
    assert!(!GlobalConfig::unset_engine(&path).unwrap());
    assert_eq!(fs::read_to_string(&path).unwrap(), edited);
    let absent = dir.path().join("absent").join(GLOBAL_CONFIG_FILE_NAME);
    assert!(!GlobalConfig::unset_engine(&absent).unwrap());
    assert!(!absent.parent().unwrap().exists());
    // Inline tables are edited in place too.
    fs::write(
        &path,
        format!("engine = {{ executable = {:?} }}\n", old.to_str().unwrap()),
    )
    .unwrap();
    GlobalConfig::set_engine(&path, &new).unwrap();
    assert_eq!(stored(&path).unwrap(), new);
}

#[test]
fn edits_refuse_a_malformed_file_and_leave_it_untouched() {
    let dir = tempdir().unwrap();
    let path = dir.path().join(GLOBAL_CONFIG_FILE_NAME);
    let engine = file(&dir.path().join("godot"));
    for text in [
        "not toml",
        "engine = 42",
        "[engine]\nexecutable = '/g'\ntypo = 1\n",
        "typo = 1\n",
    ] {
        fs::write(&path, text).unwrap();
        assert!(
            matches!(
                GlobalConfig::set_engine(&path, &engine),
                Err(Error::Config { .. })
            ),
            "{text:?}"
        );
        assert!(
            matches!(GlobalConfig::unset_engine(&path), Err(Error::Config { .. })),
            "{text:?}"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
    }
}

#[test]
fn set_refuses_a_missing_executable() {
    let dir = tempdir().unwrap();
    let path = dir.path().join(GLOBAL_CONFIG_FILE_NAME);
    for engine in [dir.path().join("missing"), dir.path().to_owned()] {
        assert!(matches!(
            GlobalConfig::set_engine(&path, &engine),
            Err(Error::EngineNotFound(_))
        ));
    }
    assert!(!path.exists());
}

#[cfg(unix)]
#[test]
fn set_writes_through_a_symlinked_config_file() {
    let dir = tempdir().unwrap();
    let dotfiles = dir.path().join("dotfiles");
    let config_dir = dir.path().join("config");
    fs::create_dir(&dotfiles).unwrap();
    fs::create_dir(&config_dir).unwrap();
    let real = dotfiles.join("gdkit.toml");
    fs::write(&real, "# managed by dotfiles\n").unwrap();
    let path = config_dir.join(GLOBAL_CONFIG_FILE_NAME);
    std::os::unix::fs::symlink(&real, &path).unwrap();
    let engine = file(&dir.path().join("godot"));
    GlobalConfig::set_engine(&path, &engine).unwrap();
    assert!(
        fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let text = fs::read_to_string(&real).unwrap();
    assert!(text.starts_with("# managed by dotfiles\n"), "{text}");
    assert_eq!(stored(&path).unwrap(), engine);
}
