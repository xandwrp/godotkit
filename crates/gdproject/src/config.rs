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
        todo!()
    }

    /// Semantic validation beyond serde: rule shapes, adapter path.
    pub fn validate(&self, path: &Path) -> crate::Result<()> {
        todo!()
    }

    /// Writes a fresh config pinning `engine`. Fails if the file exists.
    pub fn write_initial(root: &Path, engine: &Path) -> crate::Result<PathBuf> {
        todo!()
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
    todo!()
}
