//! `gdkit doctor`: which engine a project resolves to and why, and the state gdkit
//! keeps for it. Every step runs even when an earlier one fails. A failure becomes
//! a [`Problem`], not an error, so one report explains a broken setup.
//!
//! Steps: `gdkit.toml` → global config → engine candidates and selection →
//! probe-cache health → [`Engine::attach`] (a miss probes and writes the cache,
//! as every engine-backed command does) → API cache health → warning policy →
//! check artifacts. Nothing else is written; no API dump or docs run happens.
//!
//! # Tests (tests/doctor.rs, offline with `fake-godot`)
//! - `healthy_project_probes_once_then_reports_a_cache_hit`
//! - `candidates_list_every_configured_source_in_precedence_order`
//! - `invalid_configs_are_problems_and_selection_falls_through`
//! - `missing_engines_and_failed_probes_are_problems_with_the_probe_output`
//! - `api_cache_health_follows_the_engine_and_the_scripts`
//! - `warning_policy_and_strict_methods_are_reported_and_bad_settings_are_problems`
//! - `check_artifacts_report_the_count_and_the_newest_run`

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gdview::settings::WarningPolicy;
use serde::Serialize;

use crate::config::{CONFIG_FILE_NAME, Config, SelectionSource};
use crate::engine::{CacheHealth, Engine, probe_cache_health};
use crate::global::GlobalConfig;
use crate::workspace::Workspace;

/// What selects the engine, besides the project's own `gdkit.toml`.
pub struct Inputs<'a> {
    /// `--godot`
    pub explicit: Option<&'a Path>,
    /// `GDKIT_GODOT`
    pub env: Option<&'a OsString>,
    /// The global config file, whether or not it exists. `None` when no config
    /// directory is known.
    pub global_config: Option<&'a Path>,
    pub probe_deadline: Duration,
}

#[derive(Debug, Serialize)]
pub struct DoctorReport {
    pub project: ProjectState,
    pub config: ConfigState,
    pub global_config: ConfigState,
    pub engine: EngineReport,
    /// `None` when `project.godot`'s warning settings cannot be read (see problems).
    pub warnings: Option<WarningPolicy>,
    /// `[check] strict_methods` from `gdkit.toml`: check raises `unsafe_method_access` to an error.
    pub strict_methods: bool,
    /// `None` until an engine is attached; both caches are keyed by it.
    pub api_cache: Option<ApiCacheReport>,
    pub check_artifacts: ArtifactsReport,
    /// Empty when every engine-backed command can start.
    pub problems: Vec<Problem>,
}

#[derive(Debug, Serialize)]
pub struct ProjectState {
    pub root: PathBuf,
    /// Has `.godot/global_script_class_cache.cfg`, so `api` documents scripts in place.
    pub imported: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ConfigState {
    /// `None` only for a global config whose directory cannot be located.
    pub path: Option<PathBuf>,
    pub status: ConfigStatus,
    /// `[engine] executable` as written.
    pub engine: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigStatus {
    Absent,
    Valid,
    Invalid,
    Unlocatable,
}

#[derive(Debug, Serialize)]
pub struct EngineReport {
    /// Every source that names an engine, in precedence order. The first one wins.
    pub candidates: Vec<Candidate>,
    /// The winning candidate, resolved. `None` when it does not resolve.
    pub selected: Option<Selected>,
    /// The probe cache as found, before attaching.
    pub probe_cache: Option<CacheHealth>,
    pub attached: Option<Engine>,
    /// Whether attaching used the probe cache rather than probing.
    pub cache_hit: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct Candidate {
    pub source: SelectionSource,
    /// As written: a bare name, or a path relative to its config file or the current directory.
    pub value: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct Selected {
    pub executable: PathBuf,
    pub source: SelectionSource,
}

#[derive(Debug, Serialize)]
pub struct ApiCacheReport {
    pub native: CacheHealth,
    /// `None` when the project has no scripts.
    pub scripts: Option<CacheHealth>,
}

#[derive(Debug, Serialize)]
pub struct ArtifactsReport {
    /// Check runs kept under `.godot/gdkit/artifacts/check`.
    pub runs: usize,
    pub latest: Option<PathBuf>,
    pub latest_unix_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct Problem {
    pub code: ProblemCode,
    pub message: String,
    /// The end of the engine's output, for a failed probe.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProblemCode {
    /// Every engine-backed command refuses to run until it is fixed.
    ProjectConfigInvalid,
    /// Same, for every command that selects an engine.
    GlobalConfigInvalid,
    NoEngine,
    /// The winning candidate names no executable file.
    EngineNotFound,
    /// The probe ran and rejected the engine, timed out, or could not start.
    ProbeFailed,
    WarningSettingsInvalid,
    /// gdkit's own state under `.godot/gdkit` could not be inspected.
    StateUnreadable,
}

pub fn diagnose(workspace: &Workspace, inputs: &Inputs) -> DoctorReport {
    let mut problems = Problems::default();

    let (project_config, status) = match Config::load(workspace.root()) {
        Ok(Some(config)) => (Some(config), ConfigStatus::Valid),
        Ok(None) => (None, ConfigStatus::Absent),
        Err(error) => {
            problems.add(ProblemCode::ProjectConfigInvalid, error);
            (None, ConfigStatus::Invalid)
        }
    };
    let config = ConfigState {
        path: Some(workspace.root().join(CONFIG_FILE_NAME)),
        status,
        engine: project_config
            .as_ref()
            .and_then(|config| Some(config.engine.as_ref()?.executable.clone())),
    };

    let (global, status) = match inputs.global_config.map(GlobalConfig::load) {
        None => (None, ConfigStatus::Unlocatable),
        Some(Ok(Some(global))) => (Some(global), ConfigStatus::Valid),
        Some(Ok(None)) => (None, ConfigStatus::Absent),
        Some(Err(error)) => {
            problems.add(ProblemCode::GlobalConfigInvalid, error);
            (None, ConfigStatus::Invalid)
        }
    };
    let global_config = ConfigState {
        path: inputs.global_config.map(Path::to_owned),
        status,
        engine: global
            .as_ref()
            .and_then(|global| Some(global.engine.as_ref()?.executable.clone())),
    };

    // select_engine's own rule: an empty or blank GDKIT_GODOT is unset.
    let env = inputs
        .env
        .filter(|value| !value.to_str().is_some_and(|text| text.trim().is_empty()));
    let candidates = [
        (
            SelectionSource::CommandLine,
            inputs.explicit.map(Path::to_owned),
        ),
        (SelectionSource::Environment, env.map(PathBuf::from)),
        (SelectionSource::ProjectConfig, config.engine.clone()),
        (SelectionSource::GlobalConfig, global_config.engine.clone()),
    ]
    .into_iter()
    .filter_map(|(source, value)| {
        Some(Candidate {
            source,
            value: value?,
        })
    })
    .collect();
    let mut engine = EngineReport {
        candidates,
        selected: None,
        probe_cache: None,
        attached: None,
        cache_hit: None,
    };
    match crate::config::select_engine(
        workspace.root(),
        inputs.explicit,
        inputs.env,
        project_config.as_ref(),
        global.as_ref(),
    ) {
        Err(error @ crate::Error::NoEngine) => problems.add(ProblemCode::NoEngine, error),
        Err(error) => problems.add(ProblemCode::EngineNotFound, error),
        Ok(selection) => {
            engine.probe_cache = Some(probe_cache_health(&selection.executable, workspace));
            engine.selected = Some(Selected {
                executable: selection.executable.clone(),
                source: selection.source,
            });
            match Engine::attach(&selection, workspace, inputs.probe_deadline) {
                Ok((attached, hit)) => {
                    engine.attached = Some(attached);
                    engine.cache_hit = Some(hit);
                }
                Err(error) => {
                    let output = match &error {
                        crate::Error::Probe { output, .. } if !output.trim().is_empty() => {
                            Some(tail(output, 20))
                        }
                        _ => None,
                    };
                    problems.0.push(Problem {
                        code: ProblemCode::ProbeFailed,
                        message: error.to_string(),
                        output,
                    });
                }
            }
        }
    }

    let api_cache = engine.attached.as_ref().map(|attached| ApiCacheReport {
        native: crate::api::native_cache_health(workspace, attached),
        scripts: crate::api::scripts_cache_health(workspace, attached).unwrap_or_else(|error| {
            problems.add(
                ProblemCode::StateUnreadable,
                format_args!("could not hash the project's scripts for the API cache: {error}"),
            );
            None
        }),
    });

    let warnings = match workspace.project().settings().and_then(|s| s.warnings()) {
        Ok(policy) => Some(policy),
        Err(error) => {
            problems.add(ProblemCode::WarningSettingsInvalid, error);
            None
        }
    };

    let runs = workspace.artifact_runs("check").unwrap_or_else(|error| {
        problems.add(
            ProblemCode::StateUnreadable,
            format_args!("could not list check artifacts: {error}"),
        );
        Vec::new()
    });
    let latest = runs.last().cloned();
    let check_artifacts = ArtifactsReport {
        runs: runs.len(),
        latest_unix_ms: latest.as_deref().and_then(created_unix_ms),
        latest,
    };

    DoctorReport {
        project: ProjectState {
            root: workspace.root().to_owned(),
            imported: workspace
                .root()
                .join(".godot/global_script_class_cache.cfg")
                .is_file(),
        },
        config,
        global_config,
        engine,
        warnings,
        strict_methods: project_config.is_some_and(|config| config.check.strict_methods),
        api_cache,
        check_artifacts,
        problems: problems.0,
    }
}

#[derive(Default)]
struct Problems(Vec<Problem>);

impl Problems {
    fn add(&mut self, code: ProblemCode, message: impl std::fmt::Display) {
        self.0.push(Problem {
            code,
            message: message.to_string(),
            output: None,
        });
    }
}

/// Artifact directories are named `<unix_ms>-<pid>-<n>`, zero-padded.
fn created_unix_ms(run: &Path) -> Option<u64> {
    run.file_name()?.to_str()?.split('-').next()?.parse().ok()
}

fn tail(text: &str, lines: usize) -> String {
    let kept: Vec<&str> = text.lines().rev().take(lines).collect();
    kept.into_iter().rev().collect::<Vec<_>>().join("\n")
}
