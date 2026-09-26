//! `gdkit.toml` and engine selection (which also consults [`crate::global`]).
//!
//! # Tests (tests/config.rs)
//! - `load_returns_none_when_missing_and_error_when_malformed`
//! - `unknown_keys_are_rejected_with_the_offending_path`
//! - `ignore_rules_require_error_prefix_and_res_source`
//! - `checkpoint_adapter_must_be_res_gd_without_dot_dot`
//! - `select_engine_precedence_is_flag_then_env_then_config`
//! - `relative_engine_paths_resolve_against_the_config_file`
//! - `bare_names_are_looked_up_on_path_and_paths_with_separators_are_not`
//! - `path_lookup_skips_files_that_are_not_executable`
//! - `write_initial_refuses_to_overwrite_and_stores_the_path_as_given`
//! - `write_initial_stores_bare_names_bare_for_path_lookup`
//! - `write_initial_keeps_symlinks_and_the_absolute_path_as_given`
//! - `symlinks_are_canonicalized_for_selection_but_never_overwritten`
//! - `concurrent_initialization_has_exactly_one_winner`
//! - `write_initial_unpinned_leaves_the_engine_to_the_global_default`

use std::ffi::{OsStr, OsString};
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::global::GlobalConfig;

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
    /// A bare name (`godot`) is looked up on `PATH`; relative paths resolve
    /// against the directory containing `gdkit.toml`.
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
        if self.engine.as_ref().is_some_and(|engine| {
            engine
                .executable
                .to_str()
                .is_some_and(|text| text.trim().is_empty())
        }) {
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
        if let Some(adapter) = &self.run.checkpoint_adapter
            && adapter.extension() != Some("gd")
        {
            return Err(config_error(
                path,
                "run.checkpoint_adapter must name a res:// .gd script without .. segments",
            ));
        }
        Ok(())
    }

    /// Writes a fresh config pinning `engine` as the user supplied it (the raw
    /// `--godot`/`GDKIT_GODOT` value, not the canonical [`EngineSelection`]).
    /// Fails if the file exists or `engine` does not resolve to an existing file.
    ///
    /// The stored `engine.executable` stays portable across checkouts and machines,
    /// and symlinks are never resolved, so version-manager shims and aliases such as
    /// `/usr/bin/godot` keep following upgrades:
    /// - a bare name without a path separator (`godot`) is stored bare and looked
    ///   up on `PATH` at selection;
    /// - a path inside the project is stored project-relative with `/` separators
    ///   (`./godot` at the project root, so it is not read back as a bare name);
    /// - any other absolute path is stored verbatim; any other relative path is
    ///   made absolute against the current directory.
    pub fn write_initial(root: &Path, engine: &Path) -> crate::Result<PathBuf> {
        Self::write_initial_with_search_path(root, engine, std::env::var_os("PATH").as_deref())
    }

    /// [`Config::write_initial`] with an explicit `PATH` value for bare names.
    pub fn write_initial_with_search_path(
        root: &Path,
        engine: &Path,
        search_path: Option<&OsStr>,
    ) -> crate::Result<PathBuf> {
        let path = root.join(CONFIG_FILE_NAME);
        let io_error = |path: &Path| {
            let path = path.to_owned();
            move |source| crate::Error::Io { path, source }
        };
        fs::canonicalize(root).map_err(io_error(root))?;
        resolve_executable(engine, Path::new(""), search_path)?;
        let executable = stored_engine_path(root, engine)
            .map_err(io_error(engine))?
            .ok_or_else(|| config_error(&path, "engine.executable must be valid UTF-8"))?;
        #[derive(Serialize)]
        struct Pinned {
            engine: EngineConfig,
        }
        let engine = toml::to_string_pretty(&Pinned {
            engine: EngineConfig { executable },
        })
        .map_err(|error| config_error(&path, error))?;
        publish_new(&path, &format!("{PINNED_HEADER}{engine}"))?;
        Ok(path)
    }

    /// Writes a fresh config with no `[engine]`, so the project follows
    /// `GDKIT_GODOT` or the global default. The file carries a commented-out
    /// `[engine]` table showing how to pin one. Fails if the file exists.
    pub fn write_initial_unpinned(root: &Path) -> crate::Result<PathBuf> {
        let path = root.join(CONFIG_FILE_NAME);
        fs::canonicalize(root).map_err(|source| crate::Error::Io {
            path: root.to_owned(),
            source,
        })?;
        publish_new(&path, UNPINNED_TEMPLATE)?;
        Ok(path)
    }
}

const PINNED_HEADER: &str = "\
# gdkit project config.
#
# [engine] pins this project's Godot editor. Remove it to use the global
# default instead (`gdkit config set godot <path>`).

";

const UNPINNED_TEMPLATE: &str = "\
# gdkit project config.
#
# No [engine] table, so this project uses GDKIT_GODOT or the global default
# (`gdkit config set godot <path>`).
#
# To check the global default:
# `gdkit config get godot`
#
# To pin this project to one engine:
# [engine]
# executable = \"/path/to/godot\"
";

fn publish_new(path: &Path, text: &str) -> crate::Result<()> {
    let io_error = |source| crate::Error::Io {
        path: path.to_owned(),
        source,
    };
    // create_new also refuses symlinks, including dangling ones, without a check/write race.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(io_error)?;
    file.write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(io_error)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionSource {
    CommandLine,
    Environment,
    ProjectConfig,
    GlobalConfig,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineSelection {
    /// Canonicalized, verified to be a file.
    pub executable: PathBuf,
    pub source: SelectionSource,
}

/// Precedence: `explicit` (`--godot`) > `env` (`GDKIT_GODOT`, passed in for
/// testability; empty or whitespace-only counts as unset) > `[engine] executable`
/// in `gdkit.toml` > `[engine] executable` in the user's global config.
///
/// The winning value is then resolved: a bare name without a path separator
/// (`godot`) is looked up on the process `PATH` (honouring `PATHEXT` on Windows).
/// Anything with a separator is a path: flag and environment values are relative
/// to the current directory, config values to the directory containing their
/// config file. The result is canonicalized.
pub fn select_engine(
    root: &Path,
    explicit: Option<&Path>,
    env: Option<&OsString>,
    config: Option<&Config>,
    global: Option<&GlobalConfig>,
) -> crate::Result<EngineSelection> {
    select_engine_with_search_path(
        root,
        explicit,
        env,
        config,
        global,
        std::env::var_os("PATH").as_deref(),
    )
}

/// [`select_engine`] with an explicit `PATH` value for bare names.
pub fn select_engine_with_search_path(
    root: &Path,
    explicit: Option<&Path>,
    env: Option<&OsString>,
    config: Option<&Config>,
    global: Option<&GlobalConfig>,
    search_path: Option<&OsStr>,
) -> crate::Result<EngineSelection> {
    let env = env.filter(|value| !value.to_str().is_some_and(|text| text.trim().is_empty()));
    let (value, base, source) = if let Some(explicit) = explicit {
        (explicit, Path::new(""), SelectionSource::CommandLine)
    } else if let Some(env) = env {
        (Path::new(env), Path::new(""), SelectionSource::Environment)
    } else if let Some(engine) = config.and_then(|config| config.engine.as_ref()) {
        if engine.executable.as_os_str().is_empty() {
            return Err(crate::Error::EngineNotFound(engine.executable.clone()));
        }
        (
            engine.executable.as_path(),
            root,
            SelectionSource::ProjectConfig,
        )
    } else if let Some((engine, directory)) = global.and_then(|global| {
        let directory = global.path.parent()?;
        Some((global.engine.as_ref()?, directory))
    }) {
        (
            engine.executable.as_path(),
            directory,
            SelectionSource::GlobalConfig,
        )
    } else {
        return Err(crate::Error::NoEngine);
    };
    Ok(EngineSelection {
        executable: resolve_executable(value, base, search_path)?,
        source,
    })
}

pub(crate) fn config_error(path: &Path, message: impl std::fmt::Display) -> crate::Error {
    crate::Error::Config {
        path: path.to_owned(),
        message: message.to_string(),
    }
}

pub(crate) fn reject_unknown_keys(
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

/// Bare names use `search_path`; other relative paths are joined to `base`.
pub(crate) fn resolve_executable(
    value: &Path,
    base: &Path,
    search_path: Option<&OsStr>,
) -> crate::Result<PathBuf> {
    if is_bare_name(value) {
        let found = find_on_path(value.as_os_str(), search_path)
            .ok_or_else(|| crate::Error::EngineNotOnPath(value.to_owned()))?;
        return canonical_engine(&found);
    }
    canonical_engine(&base.join(value))
}

fn canonical_engine(path: &Path) -> crate::Result<PathBuf> {
    let canonical =
        fs::canonicalize(path).map_err(|_| crate::Error::EngineNotFound(path.to_owned()))?;
    if !canonical.is_file() {
        return Err(crate::Error::EngineNotFound(path.to_owned()));
    }
    Ok(canonical)
}

/// One normal component and no separator anywhere (`godot`, not `./godot` or `godot/`).
pub(crate) fn is_bare_name(path: &Path) -> bool {
    let mut components = path.components();
    matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(_)), None)
    ) && !path
        .as_os_str()
        .as_encoded_bytes()
        .iter()
        .any(|&byte| std::path::is_separator(char::from(byte)))
}

/// A minimal `which`: the first executable file named `name` in a `PATH` entry.
/// Empty entries are skipped rather than meaning the current directory.
fn find_on_path(name: &OsStr, search_path: Option<&OsStr>) -> Option<PathBuf> {
    std::env::split_paths(search_path?)
        .filter(|directory| !directory.as_os_str().is_empty())
        .flat_map(|directory| path_candidates(&directory, name))
        .find(|candidate| is_executable_file(candidate))
}

#[cfg(not(windows))]
fn path_candidates(directory: &Path, name: &OsStr) -> Vec<PathBuf> {
    vec![directory.join(name)]
}

/// Like `cmd.exe`: a name with an extension is tried as given, then every name
/// is tried with each `PATHEXT` extension appended.
#[cfg(windows)]
fn path_candidates(directory: &Path, name: &OsStr) -> Vec<PathBuf> {
    let extensions = std::env::var_os("PATHEXT")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into());
    let mut candidates = Vec::new();
    if Path::new(name).extension().is_some() {
        candidates.push(directory.join(name));
    }
    for extension in extensions.to_string_lossy().split(';') {
        if !extension.is_empty() {
            let mut file = name.to_owned();
            file.push(extension);
            candidates.push(directory.join(file));
        }
    }
    candidates
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|metadata| metadata.is_file())
}

/// See [`Config::write_initial`]. `None` when a project-relative path is not UTF-8.
fn stored_engine_path(root: &Path, engine: &Path) -> std::io::Result<Option<PathBuf>> {
    if is_bare_name(engine) {
        return Ok(Some(engine.to_owned()));
    }
    let absolute = lexical_absolute(engine)?;
    // Lexically first, so a symlinked directory inside the project stays inside
    // it; then physically, for a project root reached through a symlink.
    let inside = absolute
        .strip_prefix(lexical_absolute(root)?)
        .ok()
        .map(Path::to_owned)
        .or_else(|| {
            let root = fs::canonicalize(root).ok()?;
            let parent = fs::canonicalize(absolute.parent()?).ok()?;
            Some(parent.strip_prefix(root).ok()?.join(absolute.file_name()?))
        });
    let Some(relative) = inside else {
        return Ok(Some(if engine.is_absolute() {
            engine.to_owned()
        } else {
            absolute
        }));
    };
    let parts = relative
        .components()
        .map(|part| part.as_os_str().to_str())
        .collect::<Option<Vec<_>>>();
    Ok(parts.map(|parts| {
        let joined = parts.join("/");
        PathBuf::from(if parts.len() == 1 {
            format!("./{joined}")
        } else {
            joined
        })
    }))
}

/// Absolute against the current directory with `.` and `..` removed, without
/// touching the filesystem, so symlinks are preserved.
pub(crate) fn lexical_absolute(path: &Path) -> std::io::Result<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in std::path::absolute(path)?.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    Ok(normalized)
}
