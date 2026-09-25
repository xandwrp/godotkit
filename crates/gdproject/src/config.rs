//! `gdkit.toml` and engine selection.
//!
//! # Tests (tests/config.rs)
//! - `load_returns_none_when_missing_and_error_when_malformed`
//! - `unknown_keys_are_rejected_with_the_offending_path`
//! - `ignore_rules_require_error_prefix_and_res_source`
//! - `checkpoint_adapter_must_be_res_gd_without_dot_dot`
//! - `select_engine_precedence_is_flag_then_env_then_config`
//! - `relative_engine_paths_resolve_against_the_config_file`
//! - `write_initial_refuses_to_overwrite_and_stores_relative_path`

use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const CONFIG_FILE_NAME: &str = "gdkit.toml";

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub engine: Option<EngineConfig>,
    #[serde(default)]
    pub check: CheckConfig,
    #[serde(default)]
    pub run: RunConfig,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineConfig {
    /// Relative paths resolve against the directory containing `gdkit.toml`.
    pub executable: PathBuf,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckConfig {
    #[serde(default)]
    pub strict_methods: bool,
    #[serde(default)]
    pub ignore_import_errors: Vec<IgnoreRule>,
}

/// Exact-message + source-path suppression for known third-party noise.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IgnoreRule {
    pub message: String,
    pub source: gdview::ResPath,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunConfig {
    /// Script implementing `collect_checkpoints(tree: SceneTree) -> Dictionary`.
    pub checkpoint_adapter: Option<gdview::ResPath>,
}

impl Config {
    /// Reads and validates `<root>/gdkit.toml`. `Ok(None)` when absent.
    pub fn load(root: &Path) -> crate::Result<Option<Self>> {
        let path = root.join(CONFIG_FILE_NAME);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(source) if source.kind() == ErrorKind::NotFound => {
                // A dangling symlink is a broken config, not an absent config.
                match fs::symlink_metadata(&path) {
                    Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
                    _ => return Err(crate::Error::Io { path, source }),
                }
            }
            Err(source) => return Err(crate::Error::Io { path, source }),
        };
        let value: toml::Value =
            toml::from_str(&text).map_err(|error| config_error(&path, error))?;
        reject_unknown_keys(&value, "", &["engine", "check", "run"], &path)?;
        if let Some(engine) = value.get("engine") {
            reject_unknown_keys(engine, "engine", &["executable"], &path)?;
        }
        if let Some(check) = value.get("check") {
            reject_unknown_keys(
                check,
                "check",
                &["strict_methods", "ignore_import_errors"],
                &path,
            )?;
            if let Some(rules) = check
                .get("ignore_import_errors")
                .and_then(toml::Value::as_array)
            {
                for (index, rule) in rules.iter().enumerate() {
                    reject_unknown_keys(
                        rule,
                        &format!("check.ignore_import_errors[{index}]"),
                        &["message", "source"],
                        &path,
                    )?;
                }
            }
        }
        if let Some(run) = value.get("run") {
            reject_unknown_keys(run, "run", &["checkpoint_adapter"], &path)?;
        }
        let config: Self = toml::from_str(&text).map_err(|error| config_error(&path, error))?;
        config.validate(&path)?;
        Ok(Some(config))
    }

    /// Semantic validation beyond serde: rule shapes, adapter path.
    pub fn validate(&self, path: &Path) -> crate::Result<()> {
        if self
            .engine
            .as_ref()
            .is_some_and(|engine| engine.executable.as_os_str().is_empty())
        {
            return Err(config_error(path, "engine.executable must not be empty"));
        }
        for (index, rule) in self.check.ignore_import_errors.iter().enumerate() {
            if !rule.message.starts_with("ERROR:") {
                return Err(config_error(
                    path,
                    format!("check.ignore_import_errors[{index}].message must start with ERROR:"),
                ));
            }
            if rule.source.relative().is_empty() {
                return Err(config_error(
                    path,
                    format!(
                        "check.ignore_import_errors[{index}].source must name a res:// source file"
                    ),
                ));
            }
        }
        if let Some(adapter) = &self.run.checkpoint_adapter {
            if adapter.extension() != Some("gd") {
                return Err(config_error(
                    path,
                    "run.checkpoint_adapter must name a res:// .gd script without .. segments",
                ));
            }
        }
        Ok(())
    }

    /// Writes a fresh config pinning `engine`. Fails if the file exists.
    pub fn write_initial(root: &Path, engine: &Path) -> crate::Result<PathBuf> {
        let path = root.join(CONFIG_FILE_NAME);
        let directory = fs::canonicalize(root).map_err(|source| crate::Error::Io {
            path: root.to_owned(),
            source,
        })?;
        let engine = canonical_engine(engine)?;
        let config = Self {
            engine: Some(EngineConfig {
                executable: relative_path(&engine, &directory),
            }),
            ..Self::default()
        };
        let text = toml::to_string_pretty(&config).map_err(|error| config_error(&path, error))?;
        // create_new also refuses symlinks, including dangling ones, without a check/write race.
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|source| crate::Error::Io {
                path: path.clone(),
                source,
            })?;
        file.write_all(text.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|source| crate::Error::Io {
                path: path.clone(),
                source,
            })?;
        Ok(path)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionSource {
    CommandLine,
    Environment,
    ProjectConfig,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineSelection {
    /// Canonicalized, verified to be a file.
    pub executable: PathBuf,
    pub source: SelectionSource,
}

/// Precedence: `explicit` > `env` (`GDKIT_GODOT`, passed in for testability) > config.
pub fn select_engine(
    root: &Path,
    explicit: Option<&Path>,
    env: Option<&OsString>,
    config: Option<&Config>,
) -> crate::Result<EngineSelection> {
    let (path, source) = if let Some(explicit) = explicit {
        (explicit.to_owned(), SelectionSource::CommandLine)
    } else if let Some(env) = env {
        (PathBuf::from(env), SelectionSource::Environment)
    } else if let Some(engine) = config.and_then(|config| config.engine.as_ref()) {
        if engine.executable.as_os_str().is_empty() {
            return Err(crate::Error::EngineNotFound(engine.executable.clone()));
        }
        (
            root.join(&engine.executable),
            SelectionSource::ProjectConfig,
        )
    } else {
        return Err(crate::Error::NoEngine);
    };
    Ok(EngineSelection {
        executable: canonical_engine(&path)?,
        source,
    })
}

fn config_error(path: &Path, message: impl std::fmt::Display) -> crate::Error {
    crate::Error::Config {
        path: path.to_owned(),
        message: message.to_string(),
    }
}

fn reject_unknown_keys(
    value: &toml::Value,
    prefix: &str,
    allowed: &[&str],
    path: &Path,
) -> crate::Result<()> {
    if let Some(table) = value.as_table() {
        for key in table.keys() {
            if !allowed.contains(&key.as_str()) {
                let key_path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                return Err(config_error(
                    path,
                    format!("unknown configuration key `{key_path}`"),
                ));
            }
        }
    }
    Ok(())
}

fn canonical_engine(path: &Path) -> crate::Result<PathBuf> {
    let canonical =
        fs::canonicalize(path).map_err(|_| crate::Error::EngineNotFound(path.to_owned()))?;
    if !canonical.is_file() {
        return Err(crate::Error::EngineNotFound(path.to_owned()));
    }
    Ok(canonical)
}

fn relative_path(target: &Path, base: &Path) -> PathBuf {
    let target_parts: Vec<_> = target.components().collect();
    let base_parts: Vec<_> = base.components().collect();
    // Different Windows volumes cannot be represented by a relative path.
    if target_parts.first() != base_parts.first() {
        return target.to_owned();
    }
    let common = target_parts
        .iter()
        .zip(&base_parts)
        .take_while(|(a, b)| a == b)
        .count();
    let mut relative = PathBuf::new();
    for _ in &base_parts[common..] {
        relative.push("..");
    }
    for part in &target_parts[common..] {
        relative.push(part.as_os_str());
    }
    relative
}
