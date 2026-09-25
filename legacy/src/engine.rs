use std::{
    env,
    error::Error,
    fs,
    hash::{DefaultHasher, Hash, Hasher},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
    time::SystemTime,
};

use serde::{Deserialize, Serialize};

use crate::cli::InitArgs;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    engine: EngineConfig,
    #[serde(default, skip_serializing_if = "CheckConfig::is_empty")]
    pub(crate) check: CheckConfig,
    #[serde(default, skip_serializing_if = "InspectConfig::is_empty")]
    pub(crate) inspect: InspectConfig,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub(crate) scenarios: std::collections::BTreeMap<String, crate::scenario::ScenarioConfig>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SelectionSource {
    CommandLine,
    Environment,
    ProjectConfig,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProbeCacheHealth {
    Missing,
    Current,
    Stale,
    Malformed,
    Unreadable,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CheckConfig {
    #[serde(default)]
    pub(crate) strict_methods: bool,
    #[serde(default)]
    pub(crate) ignore_import_errors: Vec<ImportError>,
}

impl CheckConfig {
    fn is_empty(&self) -> bool {
        !self.strict_methods && self.ignore_import_errors.is_empty()
    }
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InspectConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) checkpoint_adapter: Option<String>,
}

impl InspectConfig {
    fn is_empty(&self) -> bool {
        self.checkpoint_adapter.is_none()
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ImportError {
    pub(crate) message: String,
    pub(crate) source: String,
}

pub(crate) fn read_config(project: &Path) -> Result<Option<Config>, Box<dyn Error>> {
    let path = project.join("gdkit.toml");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let config: Config =
        toml::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?;
    for rule in &config.check.ignore_import_errors {
        if !(rule.message.starts_with("ERROR:") || rule.message.starts_with("SCRIPT ERROR:"))
            || rule.message.contains(['\n', '\r'])
            || !rule.source.starts_with("res://")
            || rule.source.len() <= 6
            || rule.source.contains(['\n', '\r', '(', ')'])
        {
            return Err(format!("{}: ignore_import_errors requires an exact ERROR: or SCRIPT ERROR: message and a res:// source path", path.display()).into());
        }
    }
    if let Some(adapter) = &config.inspect.checkpoint_adapter
        && (!adapter.starts_with("res://")
            || !adapter.ends_with(".gd")
            || adapter.len() <= "res://.gd".len()
            || adapter.contains(['\n', '\r']))
    {
        return Err(format!(
            "{}: inspect.checkpoint_adapter must be a res:// path to a GDScript file",
            path.display()
        )
        .into());
    }
    Ok(Some(config))
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EngineConfig {
    executable: PathBuf,
}

#[derive(Deserialize)]
struct ProbeResult {
    compatible: bool,
    version: String,
}

#[derive(Deserialize, Serialize, PartialEq, Eq)]
struct EngineFile {
    path: PathBuf,
    size: u64,
    modified: SystemTime,
}

#[derive(Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct ProbeKey {
    files: Vec<EngineFile>,
    implementation: u64,
}

#[derive(Deserialize, Serialize)]
struct CachedProbe {
    key: ProbeKey,
    version: String,
}

pub(crate) fn probe_cache_health(engine: &Path, project: &Path) -> ProbeCacheHealth {
    let cache = project.join(".godot/gdkit/engine-probe.json");
    let bytes = match fs::read(&cache) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return ProbeCacheHealth::Missing;
        }
        Err(_) => return ProbeCacheHealth::Unreadable,
    };
    let cached = match serde_json::from_slice::<CachedProbe>(&bytes) {
        Ok(cached) if !cached.version.is_empty() => cached,
        _ => return ProbeCacheHealth::Malformed,
    };
    match probe_key(engine) {
        Ok(key) if cached.key == key => ProbeCacheHealth::Current,
        Ok(_) => ProbeCacheHealth::Stale,
        Err(_) => ProbeCacheHealth::Unreadable,
    }
}

pub(crate) fn probe_key(engine: &Path) -> std::io::Result<ProbeKey> {
    let engine = fs::canonicalize(engine)?;
    let mut paths = vec![engine.clone()];
    if cfg!(windows)
        && let Some(name) = engine.file_name().and_then(|name| name.to_str())
    {
        let companion = name
            .strip_suffix("_console.exe")
            .or_else(|| name.strip_suffix(".console.exe"))
            .map(|stem| engine.with_file_name(format!("{stem}.exe")));
        if let Some(path) = companion.filter(|path| path.is_file()) {
            paths.push(path);
        }
    }
    let files = paths
        .into_iter()
        .map(|path| {
            let metadata = fs::metadata(&path)?;
            Ok(EngineFile {
                path,
                size: metadata.len(),
                modified: metadata.modified()?,
            })
        })
        .collect::<std::io::Result<Vec<_>>>()?;
    let mut implementation = DefaultHasher::new();
    include_str!("engine.rs").hash(&mut implementation);
    include_str!("probe.gd").hash(&mut implementation);
    include_str!("check.gd").hash(&mut implementation);
    include_str!("runtime_probe.gd").hash(&mut implementation);
    include_str!("debugger_bridge.gd").hash(&mut implementation);
    Ok(ProbeKey {
        files,
        implementation: implementation.finish(),
    })
}

pub(crate) fn fingerprint(engine: &Path) -> Result<String, Box<dyn Error>> {
    let key = probe_key(engine)?;
    Ok(blake3::hash(&serde_json::to_vec(&key)?)
        .to_hex()
        .to_string())
}

fn cached_probe(
    engine: &Path,
    cache: &Path,
    validate: impl FnOnce(&Path) -> Result<String, Box<dyn Error>>,
) -> Result<(String, bool), Box<dyn Error>> {
    let key = probe_key(engine).ok();
    if let Some(key) = &key
        && let Some(cached) = fs::read(cache)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<CachedProbe>(&bytes).ok())
            .filter(|cached| cached.key == *key && !cached.version.is_empty())
    {
        return Ok((cached.version, true));
    }
    let version = validate(engine)?;
    if let Some(key) = key
        && probe_key(engine).ok().as_ref() == Some(&key)
    {
        let cached = CachedProbe {
            key,
            version: version.clone(),
        };
        if let (Some(parent), Ok(bytes)) = (cache.parent(), serde_json::to_vec(&cached))
            && fs::create_dir_all(parent).is_ok()
        {
            let _ = fs::write(cache, bytes);
        }
    }
    Ok((version, false))
}

pub fn validated_version(engine: &Path, project: &Path) -> Result<(String, bool), Box<dyn Error>> {
    cached_probe(
        engine,
        &project.join(".godot/gdkit/engine-probe.json"),
        probe,
    )
}

struct ProbeDirectory(PathBuf);

impl Drop for ProbeDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn project_root(path: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let root = fs::canonicalize(path)?;
    let project = gdview::Project::open(&root).map_err(|error| -> Box<dyn Error> {
        match error {
            gdview::ProjectError::NotFound { .. } | gdview::ProjectError::ConfigNotAFile { .. } => format!(
            "no project.godot in {}\nRun from the directory containing project.godot, or use gdkit check <project-directory> (for example: gdkit check game).",
            display_path(&root)
            ).into(),
            error => Box::new(error),
        }
    })?;
    fs::read_to_string(project.config_path())?;
    Ok(root)
}

pub(crate) fn display_path(path: &Path) -> String {
    let text = path.display().to_string();
    if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
    }
}

pub fn resolve(project: &Path, explicit: Option<&Path>) -> Result<PathBuf, Box<dyn Error>> {
    resolve_selected(project, explicit).map(|(path, _)| path)
}

pub(crate) fn resolve_selected(
    project: &Path,
    explicit: Option<&Path>,
) -> Result<(PathBuf, SelectionSource), Box<dyn Error>> {
    let (path, source) = if let Some(path) = explicit {
        (path.to_owned(), SelectionSource::CommandLine)
    } else if let Some(path) = env::var_os("GDKIT_GODOT") {
        (PathBuf::from(path), SelectionSource::Environment)
    } else {
        let config = read_config(project)?.ok_or("no engine configured; run gdkit init --godot <path> in the project, or supply --godot or GDKIT_GODOT")?;
        (
            project.join(config.engine.executable),
            SelectionSource::ProjectConfig,
        )
    };
    if !path.is_file() {
        return Err(format!("Godot executable not found: {}", path.display()).into());
    }
    Ok((fs::canonicalize(path)?, source))
}

pub fn probe(engine: &Path) -> Result<String, Box<dyn Error>> {
    let help = Command::new(engine).arg("--help").output()?;
    let help_text = String::from_utf8_lossy(&help.stdout);
    if !help.status.success()
        || ![
            "--headless",
            "--import",
            "--script",
            "--check-only",
            "--no-header",
        ]
        .iter()
        .all(|flag| help_text.contains(flag))
    {
        return Err("engine lacks required headless editor command-line support".into());
    }
    let mut directory = None;
    for attempt in 0..100 {
        let path = env::temp_dir().join(format!("gdkit-probe-{}-{attempt}", std::process::id()));
        match fs::create_dir(&path) {
            Ok(()) => {
                directory = Some(ProbeDirectory(path));
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    let directory = directory.ok_or("could not create engine probe directory")?;
    fs::write(directory.0.join("project.godot"), "config_version=5\n")?;
    fs::write(directory.0.join("probe.gd"), include_str!("probe.gd"))?;
    fs::write(
        directory.0.join("probe.tres"),
        "[gd_resource type=\"Resource\" format=3]\n[resource]\nresource_name = \"probe\"\n",
    )?;
    fs::write(directory.0.join("check.gd"), include_str!("check.gd"))?;
    let output = Command::new(engine)
        .args(["--headless", "--path"])
        .arg(&directory.0)
        .args(["--script", "probe.gd"])
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let result = stdout
        .lines()
        .find_map(|line| line.strip_prefix("GDKIT_PROBE_RESULT:"))
        .and_then(|line| serde_json::from_str::<ProbeResult>(line).ok());
    let result = match result {
        Some(result) if output.status.success() && result.compatible && !crate::check::has_errors(&output) => result,
        _ => return Err(format!("engine compatibility probe failed; requires a Godot 4 editor with GDScript and resource loading\n{}{}", stdout, String::from_utf8_lossy(&output.stderr)).into()),
    };
    let syntax = Command::new(engine)
        .args(["--headless", "--path"])
        .arg(&directory.0)
        .args(["--script", "check.gd", "--check-only"])
        .output()?;
    if !syntax.status.success() || crate::check::has_errors(&syntax) {
        return Err(format!(
            "engine cannot parse the checker harness\n{}{}",
            String::from_utf8_lossy(&syntax.stdout),
            String::from_utf8_lossy(&syntax.stderr)
        )
        .into());
    }
    Ok(result.version)
}

pub fn init(args: InitArgs) -> Result<ExitCode, Box<dyn Error>> {
    let project = project_root(&env::current_dir()?)?;
    let config_path = project.join("gdkit.toml");
    if config_path.exists() {
        return Err(
            "gdkit.toml already exists; edit its engine.executable to change engines".into(),
        );
    }
    let engine = resolve(&project, args.godot.as_deref())?;
    let (version, _) = validated_version(&engine, &project)?;
    let relative = pathdiff::diff_paths(&engine, &project).unwrap_or_else(|| engine.clone());
    let config = Config {
        check: CheckConfig::default(),
        inspect: InspectConfig::default(),
        scenarios: std::collections::BTreeMap::new(),
        engine: EngineConfig {
            executable: relative,
        },
    };
    let text = toml::to_string_pretty(&config)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&config_path)?;
    file.write_all(text.as_bytes())?;
    println!("initialized {}", config_path.display());
    eprintln!("engine: {} ({version})", engine.display());
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn validates_checkpoint_adapter_paths() {
        let directory =
            env::temp_dir().join(format!("gdkit-checkpoint-config-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let _cleanup = ProbeDirectory(directory.clone());
        fs::write(
            directory.join("gdkit.toml"),
            "[engine]\nexecutable='godot'\n[inspect]\ncheckpoint_adapter='res://tools/checkpoints.gd'\n",
        )
        .unwrap();
        assert_eq!(
            read_config(&directory)
                .unwrap()
                .unwrap()
                .inspect
                .checkpoint_adapter
                .as_deref(),
            Some("res://tools/checkpoints.gd")
        );
        fs::write(
            directory.join("gdkit.toml"),
            "[engine]\nexecutable='godot'\n[inspect]\ncheckpoint_adapter='../checkpoints.gd'\n",
        )
        .unwrap();
        assert!(
            read_config(&directory)
                .err()
                .unwrap()
                .to_string()
                .contains("must be a res:// path")
        );
    }

    #[test]
    fn cache_reuses_only_successful_matching_probes() {
        let directory = env::temp_dir().join(format!("gdkit-probe-cache-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let _cleanup = ProbeDirectory(directory.clone());
        let engine = directory.join("engine.exe");
        let cache = directory.join("cache/probe.json");
        fs::write(&engine, "engine").unwrap();
        let calls = Cell::new(0);
        let validate = |_: &Path| {
            calls.set(calls.get() + 1);
            Ok("4.test".to_owned())
        };
        assert_eq!(
            cached_probe(&engine, &cache, validate).unwrap(),
            ("4.test".into(), false)
        );
        assert!(cached_probe(&engine, &cache, validate).unwrap().1);
        assert_eq!(calls.get(), 1);

        fs::write(&engine, "new engine").unwrap();
        assert!(!cached_probe(&engine, &cache, validate).unwrap().1);
        let modified = fs::metadata(&engine).unwrap().modified().unwrap();
        fs::File::options()
            .write(true)
            .open(&engine)
            .unwrap()
            .set_times(
                fs::FileTimes::new().set_modified(modified + std::time::Duration::from_secs(2)),
            )
            .unwrap();
        assert!(!cached_probe(&engine, &cache, validate).unwrap().1);
        let other_engine = directory.join("other.exe");
        fs::copy(&engine, &other_engine).unwrap();
        assert!(!cached_probe(&other_engine, &cache, validate).unwrap().1);

        let mut entry: CachedProbe = serde_json::from_slice(&fs::read(&cache).unwrap()).unwrap();
        entry.key.implementation ^= 1;
        fs::write(&cache, serde_json::to_vec(&entry).unwrap()).unwrap();
        assert!(!cached_probe(&other_engine, &cache, validate).unwrap().1);
        fs::write(&cache, "partial json").unwrap();
        assert!(!cached_probe(&other_engine, &cache, validate).unwrap().1);
        assert_eq!(calls.get(), 6);

        fs::remove_file(&cache).unwrap();
        assert!(cached_probe(&engine, &cache, |_| Err("failed probe".into())).is_err());
        assert!(!cache.exists());
        assert!(!cached_probe(&engine, &cache, validate).unwrap().1);
        assert!(
            !cached_probe(&engine, &engine.join("unwritable.json"), validate)
                .unwrap()
                .1
        );

        fs::remove_file(&cache).unwrap();
        cached_probe(&engine, &cache, |_| {
            fs::write(&engine, "changed during probe").unwrap();
            Ok("4.test".into())
        })
        .unwrap();
        assert!(!cache.exists());
    }

    #[test]
    fn reports_probe_cache_health() {
        let directory = env::temp_dir().join(format!("gdkit-probe-health-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let _cleanup = ProbeDirectory(directory.clone());
        let engine = directory.join("engine.exe");
        let cache = directory.join(".godot/gdkit/engine-probe.json");
        fs::write(&engine, "engine").unwrap();
        assert_eq!(
            probe_cache_health(&engine, &directory),
            ProbeCacheHealth::Missing
        );
        cached_probe(&engine, &cache, |_| Ok("4.test".into())).unwrap();
        assert_eq!(
            probe_cache_health(&engine, &directory),
            ProbeCacheHealth::Current
        );
        fs::write(&engine, "changed engine").unwrap();
        assert_eq!(
            probe_cache_health(&engine, &directory),
            ProbeCacheHealth::Stale
        );
        fs::write(&cache, "partial json").unwrap();
        assert_eq!(
            probe_cache_health(&engine, &directory),
            ProbeCacheHealth::Malformed
        );
    }

    #[test]
    #[cfg(windows)]
    fn console_cache_tracks_companion_engine() {
        let directory = env::temp_dir().join(format!("gdkit-console-cache-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let _cleanup = ProbeDirectory(directory.clone());
        for name in ["godot_console.exe", "godot.console.exe"] {
            let launcher = directory.join(name);
            let companion = directory.join("godot.exe");
            fs::write(&launcher, "launcher").unwrap();
            fs::write(&companion, "engine").unwrap();
            let before = probe_key(&launcher).unwrap();
            assert_eq!(before.files.len(), 2);
            fs::write(&companion, "updated engine").unwrap();
            assert!(before != probe_key(&launcher).unwrap());
            fs::remove_file(&companion).unwrap();
            assert!(before != probe_key(&launcher).unwrap());
        }
    }
}
