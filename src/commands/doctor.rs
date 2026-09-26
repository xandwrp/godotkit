//! `doctor`: workspace → `gdproject::doctor::diagnose` (configs, engine candidates
//! and selection, probe cache before and after attaching, API cache health, warning
//! policy, check artifacts) → emit. Exits 1 when the report lists problems; only a
//! missing project or a failed write is a tool error.

use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use gdproject::config::SelectionSource;
use gdproject::doctor::{ConfigState, ConfigStatus, DoctorReport, Inputs};
use gdproject::engine::CacheHealth;
use gdview::settings::DirectoryRuleMode;

use crate::cli::*;
use crate::context::Context;
use crate::render::{self, Exit, Human};

pub fn run(ctx: &Context, args: ProjectArgs) -> gdproject::Result<Exit> {
    let workspace = ctx.workspace(&args)?;
    let report = Report(gdproject::doctor::diagnose(
        &workspace,
        &Inputs {
            explicit: args.godot.as_deref(),
            env: ctx.env_godot.as_ref(),
            global_config: ctx.global_config_path.as_deref(),
            probe_deadline: ctx.probe_deadline,
        },
    ));
    render::emit(ctx.output, &report).map_err(|source| gdproject::Error::Io {
        path: "<stdout>".into(),
        source,
    })?;
    Ok(if report.0.problems.is_empty() {
        Exit::Ok
    } else {
        Exit::Failed
    })
}

#[derive(serde::Serialize)]
#[serde(transparent)]
struct Report(DoctorReport);

impl Human for Report {
    fn human(&self, out: &mut dyn Write) -> std::io::Result<()> {
        let r = &self.0;
        let imported = if r.project.imported {
            "imported"
        } else {
            "not imported; `api` documents scripts from a copy"
        };
        writeln!(out, "project   {} ({imported})", r.project.root.display())?;

        let engine = &r.engine;
        match (&engine.attached, &engine.selected) {
            (Some(attached), _) => writeln!(
                out,
                "engine    {} ({})",
                attached.executable.display(),
                attached.version
            )?,
            (None, Some(selected)) => writeln!(
                out,
                "engine    {} (failed the probe)",
                selected.executable.display()
            )?,
            (None, None) => writeln!(out, "engine    none")?,
        }
        if let Some((chosen, rest)) = engine.candidates.split_first() {
            write!(out, "          from {}", source(chosen.source))?;
            for other in rest {
                write!(
                    out,
                    "; overrides {} ({})",
                    source(other.source),
                    other.value.display()
                )?;
            }
            writeln!(out)?;
        }
        if let Some(before) = engine.probe_cache {
            let probe = match engine.cache_hit {
                Some(true) => "current, used".to_owned(),
                Some(false) => format!("{}, probed and cached", health(before)),
                None => health(before).to_owned(),
            };
            writeln!(out, "          probe cache: {probe}")?;
        }

        let project_config = ConfigState {
            path: None,
            ..r.config.clone()
        };
        writeln!(out, "config    {}", config("gdkit.toml", &project_config))?;
        writeln!(out, "          {}", config("global", &r.global_config))?;

        if let Some(policy) = &r.warnings {
            let mut parts = vec![
                if policy.enabled {
                    "enabled"
                } else {
                    "disabled"
                }
                .to_owned(),
            ];
            parts.extend(policy.directory_rules.iter().map(|rule| {
                let mode = match rule.mode {
                    DirectoryRuleMode::Exclude => "excluded",
                    DirectoryRuleMode::Include => "included",
                };
                format!("{} {mode}", rule.path)
            }));
            parts.extend(policy.overrides.iter().map(|(name, value)| {
                let level = match value.as_str() {
                    "0" => "ignore",
                    "1" => "warn",
                    "2" => "error",
                    other => other,
                };
                format!("{name}={level}")
            }));
            writeln!(out, "warnings  {}", parts.join("; "))?;
        }
        if r.strict_methods {
            writeln!(
                out,
                "          strict_methods: check makes unsafe_method_access an error"
            )?;
        }

        if let Some(api) = &r.api_cache {
            let scripts = api.scripts.map_or("no scripts", health);
            writeln!(
                out,
                "api cache native {}; scripts {scripts}",
                health(api.native)
            )?;
        }

        let check = &r.check_artifacts;
        match (&check.latest, check.latest_unix_ms) {
            (Some(latest), created) => writeln!(
                out,
                "check     {} run{} kept; latest {}: {}",
                check.runs,
                if check.runs == 1 { "" } else { "s" },
                created.map_or("at an unknown time".to_owned(), ago),
                latest.display()
            )?,
            (None, _) => writeln!(out, "check     no runs kept")?,
        }

        if r.problems.is_empty() {
            return writeln!(out, "problems  none");
        }
        writeln!(out, "problems")?;
        for problem in &r.problems {
            writeln!(out, "  - {}", problem.message)?;
            for line in problem.output.iter().flat_map(|output| output.lines()) {
                writeln!(out, "      {line}")?;
            }
        }
        Ok(())
    }
}

fn source(source: SelectionSource) -> &'static str {
    match source {
        SelectionSource::CommandLine => "--godot",
        SelectionSource::Environment => "GDKIT_GODOT",
        SelectionSource::ProjectConfig => "gdkit.toml",
        SelectionSource::GlobalConfig => "the global default",
    }
}

fn health(health: CacheHealth) -> &'static str {
    match health {
        CacheHealth::Missing => "missing",
        CacheHealth::Current => "current",
        CacheHealth::Stale => "stale",
        CacheHealth::Malformed => "malformed",
        CacheHealth::Unreadable => "unreadable",
    }
}

fn config(name: &str, state: &ConfigState) -> String {
    let path = state
        .path
        .as_ref()
        .map_or(String::new(), |path| format!(" {}", path.display()));
    let status = match state.status {
        ConfigStatus::Absent => "absent",
        ConfigStatus::Valid => "valid",
        ConfigStatus::Invalid => "invalid",
        ConfigStatus::Unlocatable => "cannot be located; set GDKIT_CONFIG_DIR",
    };
    let engine = state.engine.as_ref().map_or(String::new(), |engine| {
        format!(", engine {}", engine.display())
    });
    format!("{name}{path}: {status}{engine}")
}

fn ago(unix_ms: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |now| now.as_millis() as u64);
    let minutes = now.saturating_sub(unix_ms) / 60_000;
    match minutes {
        0 => "just now".into(),
        1..60 => format!("{minutes}m ago"),
        60..2880 => format!("{}h ago", minutes / 60),
        _ => format!("{}d ago", minutes / 1440),
    }
}
