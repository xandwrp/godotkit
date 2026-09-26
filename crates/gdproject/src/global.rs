//! The user's global config: machine-wide defaults under every project.
//!
//! One file, `config.toml`, in `$GDKIT_CONFIG_DIR` when set, else
//! `$XDG_CONFIG_HOME/gdkit` or `~/.config/gdkit` on Unix (macOS included) and
//! `%APPDATA%\gdkit` on Windows. It takes the same `[engine] executable` shape
//! as `gdkit.toml`; a project's own entry wins (see [`crate::config::select_engine`]).
//!
//! `gdkit config set`/`unset` edit the file in place, keeping comments and
//! formatting, and refuse to write a result that would not load.
//!
//! # Tests (tests/global.rs)
//! - `locate_prefers_gdkit_config_dir_then_the_platform_default`
//! - `load_returns_none_when_missing_and_rejects_unknown_keys`
//! - `global_default_applies_only_when_nothing_else_selects_an_engine`
//! - `relative_executables_resolve_against_the_config_directory`
//! - `set_creates_the_directory_and_stores_the_path_as_given`
//! - `set_and_unset_keep_comments_and_formatting`
//! - `edits_refuse_a_malformed_file_and_leave_it_untouched`
//! - `set_refuses_a_missing_executable`
//! - `set_writes_through_a_symlinked_config_file` (Unix)

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::config::{
    EngineConfig, config_error, is_bare_name, lexical_absolute, reject_unknown_keys,
    resolve_executable,
};

pub const GLOBAL_CONFIG_FILE_NAME: &str = "config.toml";
pub const CONFIG_DIR_ENV: &str = "GDKIT_CONFIG_DIR";

#[derive(Debug)]
pub struct GlobalConfig {
    /// The file this was loaded from. Relative engine paths resolve against its directory.
    pub path: PathBuf,
    pub engine: Option<EngineConfig>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    engine: Option<EngineConfig>,
}

impl GlobalConfig {
    /// Where the global config lives, from environment lookups (passed in for
    /// testability). Empty values count as unset. `None` when no home or config
    /// directory is known.
    pub fn locate(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
        let var = |name: &str| env(name).filter(|value| !value.is_empty());
        let directory = if let Some(directory) = var(CONFIG_DIR_ENV) {
            PathBuf::from(directory)
        } else if cfg!(windows) {
            PathBuf::from(var("APPDATA")?).join("gdkit")
        } else if let Some(xdg) = var("XDG_CONFIG_HOME").filter(|xdg| Path::new(xdg).is_absolute())
        {
            PathBuf::from(xdg).join("gdkit")
        } else {
            PathBuf::from(var("HOME")?).join(".config").join("gdkit")
        };
        Some(directory.join(GLOBAL_CONFIG_FILE_NAME))
    }

    /// Reads and validates the file at `path`. `Ok(None)` when absent.
    pub fn load(path: &Path) -> crate::Result<Option<Self>> {
        let Some(text) = read(path)? else {
            return Ok(None);
        };
        parse(path, &text).map(Some)
    }

    /// Sets `[engine] executable` to `engine` as the user supplied it and returns
    /// the stored value. A bare name (`godot`) is stored bare for `PATH` lookup;
    /// an absolute path is stored verbatim, so symlinks such as `/usr/bin/godot`
    /// keep following upgrades; a relative path is made absolute against the
    /// current directory. Fails if `engine` does not resolve to a file.
    pub fn set_engine(path: &Path, engine: &Path) -> crate::Result<String> {
        Self::set_engine_with_search_path(path, engine, std::env::var_os("PATH").as_deref())
    }

    /// [`GlobalConfig::set_engine`] with an explicit `PATH` value for bare names.
    pub fn set_engine_with_search_path(
        path: &Path,
        engine: &Path,
        search_path: Option<&OsStr>,
    ) -> crate::Result<String> {
        resolve_executable(engine, Path::new(""), search_path)?;
        let stored = if is_bare_name(engine) || engine.is_absolute() {
            engine.to_owned()
        } else {
            lexical_absolute(engine).map_err(|source| crate::Error::Io {
                path: engine.to_owned(),
                source,
            })?
        };
        let stored = stored
            .into_os_string()
            .into_string()
            .map_err(|_| config_error(path, "engine.executable must be valid UTF-8"))?;
        edit(path, |document| {
            if !document.contains_key("engine") {
                // Comments in a file without tables are trailing trivia; keep them above the new table.
                let mut table = toml_edit::Table::new();
                table.decor_mut().set_prefix(raw(document.trailing()));
                document.set_trailing("");
                document.insert("engine", toml_edit::Item::Table(table));
            }
            let engine = document
                .get_mut("engine")
                .and_then(toml_edit::Item::as_table_like_mut)
                .ok_or_else(|| config_error(path, "`engine` must be a table"))?;
            match engine
                .get_mut("executable")
                .and_then(toml_edit::Item::as_value_mut)
            {
                // Replace only the value, so the key's comments and a trailing comment survive.
                Some(value) => {
                    let decor = value.decor().clone();
                    *value = stored.as_str().into();
                    *value.decor_mut() = decor;
                }
                None => {
                    engine.insert("executable", toml_edit::value(stored.as_str()));
                }
            }
            Ok(true)
        })?;
        Ok(stored)
    }

    /// Removes `[engine] executable`, and the `[engine]` table if that leaves it
    /// empty. Returns whether there was a value to remove; the file is untouched
    /// when there was not.
    pub fn unset_engine(path: &Path) -> crate::Result<bool> {
        edit(path, |document| {
            let Some(engine) = document
                .get_mut("engine")
                .and_then(toml_edit::Item::as_table_like_mut)
            else {
                return Ok(false);
            };
            let removed = engine.remove("executable").is_some();
            if engine.is_empty()
                && let Some(table) = document.remove("engine")
            {
                // Comments above the table header belong to the file, not the table.
                let prefix = table
                    .as_table()
                    .and_then(|table| table.decor().prefix())
                    .map(raw)
                    .unwrap_or_default();
                let trailing = raw(document.trailing());
                document.set_trailing(format!("{prefix}{trailing}"));
            }
            Ok(removed)
        })
    }
}

/// Decor text of a parsed document; only spans into an unparsed source have none.
fn raw(text: &toml_edit::RawString) -> String {
    text.as_str().unwrap_or_default().to_owned()
}

/// `Ok(None)` when the file is absent. A dangling symlink is a broken config, not an absent one.
fn read(path: &Path) -> crate::Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(source) if source.kind() == ErrorKind::NotFound => match fs::symlink_metadata(path) {
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            _ => Err(crate::Error::Io {
                path: path.to_owned(),
                source,
            }),
        },
        Err(source) => Err(crate::Error::Io {
            path: path.to_owned(),
            source,
        }),
    }
}

fn parse(path: &Path, text: &str) -> crate::Result<GlobalConfig> {
    let value: toml::Value = toml::from_str(text).map_err(|error| config_error(path, error))?;
    reject_unknown_keys(&value, "", &["engine"], path)?;
    if let Some(engine) = value.get("engine") {
        reject_unknown_keys(engine, "engine", &["executable"], path)?;
    }
    let file: File = toml::from_str(text).map_err(|error| config_error(path, error))?;
    if file.engine.as_ref().is_some_and(|engine| {
        engine
            .executable
            .to_str()
            .is_some_and(|text| text.trim().is_empty())
    }) {
        return Err(config_error(path, "engine.executable must not be empty"));
    }
    Ok(GlobalConfig {
        path: path.to_owned(),
        engine: file.engine,
    })
}

/// Applies `change` to the document at `path` (empty when absent) and, when it
/// reports a change, replaces the file atomically. The existing file and the
/// result must both load, so an edit never hides or introduces a broken config.
fn edit(
    path: &Path,
    change: impl FnOnce(&mut toml_edit::DocumentMut) -> crate::Result<bool>,
) -> crate::Result<bool> {
    let text = read(path)?.unwrap_or_default();
    parse(path, &text)?;
    let mut document: toml_edit::DocumentMut =
        text.parse().map_err(|error| config_error(path, error))?;
    if !change(&mut document)? {
        return Ok(false);
    }
    let text = document.to_string();
    parse(path, &text)?;
    // A symlinked file (dotfile managers) is updated where it points, not replaced.
    let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    replace(&target, &text)?;
    Ok(true)
}

fn replace(path: &Path, text: &str) -> crate::Result<()> {
    let io_error = |path: &Path| {
        let path = path.to_owned();
        move |source| crate::Error::Io { path, source }
    };
    let directory = path
        .parent()
        .ok_or_else(|| config_error(path, "config file has no parent directory"))?;
    fs::create_dir_all(directory).map_err(io_error(directory))?;
    let temporary = directory.join(format!(
        ".{GLOBAL_CONFIG_FILE_NAME}.{}.tmp",
        std::process::id()
    ));
    let write = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    let _ = fs::remove_file(&temporary);
    write.map_err(io_error(path))
}
