use std::{error::Error, fs, path::Path, process::ExitCode};

use crate::{
    cli::DoctorArgs,
    engine::{ProbeCacheHealth, SelectionSource},
    import_worker::WorkerState,
};

fn setting(source: &str, name: &str) -> Option<String> {
    source.lines().find_map(|line| {
        let line = line.trim();
        let (key, value) = line.split_once('=')?;
        (key.trim() == name).then(|| value.trim().to_owned())
    })
}

fn warning_policy(
    project: &Path,
    strict_methods: Option<bool>,
) -> Result<Vec<String>, Box<dyn Error>> {
    let source = fs::read_to_string(project.join("project.godot"))?;
    let enabled = setting(&source, "gdscript/warnings/enable")
        .unwrap_or_else(|| "true (Godot default)".into());
    let excluded = setting(&source, "gdscript/warnings/exclude_addons")
        .unwrap_or_else(|| "true (Godot default)".into());
    let mut policy = vec![
        format!("GDScript warnings: {enabled}"),
        format!("exclude addon warnings: {excluded}"),
        match strict_methods {
            Some(true) => "strict methods: enabled by gdkit.toml; unsafe_method_access is an error during checks".into(),
            Some(false) => {
                "strict methods: disabled; checks retain the project warning policy".into()
            }
            None => "strict methods: unavailable because gdkit.toml is invalid".into(),
        },
    ];
    let overrides = source
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let (key, value) = line.split_once('=')?;
            key.trim()
                .strip_prefix("gdscript/warnings/")
                .filter(|name| *name != "enable" && *name != "exclude_addons")
                .map(|name| format!("{name}={}", value.trim()))
        })
        .collect::<Vec<_>>();
    if !overrides.is_empty() {
        policy.push(format!("project overrides: {}", overrides.join(", ")));
    }
    Ok(policy)
}

pub(crate) fn run(args: DoctorArgs) -> Result<ExitCode, Box<dyn Error>> {
    let project = crate::engine::project_root(&args.project)?;
    let (engine, source) = crate::engine::resolve_selected(&project, args.godot.as_deref())?;
    let (config, config_issue) = match crate::engine::read_config(&project) {
        Ok(config) => (config, None),
        Err(error) if source != SelectionSource::ProjectConfig => (None, Some(error.to_string())),
        Err(error) => return Err(error),
    };
    let strict_methods = config
        .as_ref()
        .map(|config| config.check.strict_methods)
        .or(config_issue.is_none().then_some(false));
    let ignored_import_errors = config
        .as_ref()
        .map_or(0, |config| config.check.ignore_import_errors.len());
    let checkpoint_adapter = config
        .as_ref()
        .and_then(|config| config.inspect.checkpoint_adapter.as_deref());
    let prior_cache = crate::engine::probe_cache_health(&engine, &project);
    let (version, cached) = crate::engine::validated_version(&engine, &project)?;
    let worker = crate::import_worker::inspect(&project);

    println!("project: {}", crate::engine::display_path(&project));
    println!("engine: {}", crate::engine::display_path(&engine));
    println!("version: {version}");
    let selected_by = match source {
        SelectionSource::CommandLine => "--godot".into(),
        SelectionSource::Environment => "GDKIT_GODOT".into(),
        SelectionSource::ProjectConfig => crate::engine::display_path(&project.join("gdkit.toml")),
    };
    println!("selected by: {selected_by}");
    println!(
        "probe cache: {}",
        match (prior_cache, cached) {
            (ProbeCacheHealth::Current, true) => "healthy (hit)",
            (ProbeCacheHealth::Missing, false) => "healthy (created; was missing)",
            (ProbeCacheHealth::Stale, false) => "healthy (refreshed; was stale)",
            (ProbeCacheHealth::Malformed, false) => "healthy (replaced malformed entry)",
            (ProbeCacheHealth::Unreadable, false) => "unavailable (engine was probed)",
            _ => "healthy (refreshed)",
        }
    );
    println!(
        "legacy worker: {}",
        match worker.state {
            WorkerState::None => "stopped (no saved worker)".into(),
            WorkerState::Running => format!("running (pid {})", worker.pid.unwrap_or_default()),
            WorkerState::Stale => format!(
                "stale record (pid {} is not running)",
                worker.pid.unwrap_or_default()
            ),
            WorkerState::Unknown => "saved record found; process status unavailable".into(),
            WorkerState::Malformed => "malformed saved record".into(),
        }
    );

    println!("warning policy:");
    for line in warning_policy(&project, strict_methods)? {
        println!("  {line}");
    }
    println!(
        "checkpoint adapter: {}",
        checkpoint_adapter.unwrap_or("not configured")
    );

    let mut gotchas = Vec::new();
    if let Some(issue) = config_issue {
        gotchas.push(format!(
            "gdkit.toml is invalid; the engine still came from {selected_by}, but gdkit check will fail until this is fixed: {issue}"
        ));
    }
    if worker.state == WorkerState::Stale {
        gotchas.push(
            "the saved legacy worker record is stale; run gdkit cache stop to remove it".into(),
        );
    }
    if !worker.diagnostics.is_empty() {
        gotchas.push(format!(
            "the legacy worker has {} cached import error line{}; run gdkit cache stop to remove it",
            worker.diagnostics.len(),
            if worker.diagnostics.len() == 1 {
                ""
            } else {
                "s"
            }
        ));
    }
    if ignored_import_errors > 0 {
        gotchas.push(format!(
            "gdkit.toml suppresses {ignored_import_errors} exact import error{}",
            if ignored_import_errors == 1 { "" } else { "s" }
        ));
    }
    println!("gotchas:");
    if gotchas.is_empty() {
        println!("  none detected");
    } else {
        for gotcha in gotchas {
            println!("  - {gotcha}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_warning_values_without_matching_comments_or_prefixes() {
        let source = "gdscript/warnings/enable=false\nother/gdscript/warnings/enable=true\n";
        assert_eq!(
            setting(source, "gdscript/warnings/enable"),
            Some("false".into())
        );
        assert_eq!(setting(source, "missing"), None);
    }
}
