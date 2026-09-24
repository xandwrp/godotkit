use std::{
    collections::{BTreeSet, HashMap, HashSet},
    env,
    error::Error,
    ffi::OsStr,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, ExitCode, Output},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::Deserialize;

use gdkit::report::{
    Artifact, ArtifactKind, CheckCounts, CheckFailure, CheckOutcome, CheckPhase, CheckPolicy,
    CheckReport, Diagnostic, DiagnosticSeverity, DiagnosticStream, EngineFingerprint, FailureKind,
    PhaseIdentity, ProcessIdentity, ProjectSnapshot, SkippedPhase, StackFrame,
};

use crate::cli::{CheckArgs, CheckOutput};
use crate::process::{CapturedOutput, OutputStream};

const HARNESS: &str = include_str!("check.gd");
const IMPORT_SCAN: &str = include_str!("import_scan.gd");
const SCRIPT_BOOTSTRAP: &str = include_str!("script_bootstrap.gd");
const IMPORT_SCAN_RESULT: &str = "GDKIT_IMPORT_SCAN_COMPLETE";
const RESULT_PREFIX: &str = "GDKIT_CHECK_RESULT:";

struct PhaseTimer {
    start: Instant,
    name: &'static str,
    enabled: bool,
}

impl PhaseTimer {
    fn new(name: &'static str, enabled: bool) -> Self {
        Self {
            start: Instant::now(),
            name,
            enabled,
        }
    }
}

impl Drop for PhaseTimer {
    fn drop(&mut self) {
        if self.enabled {
            let _ = writeln!(
                io::stderr().lock(),
                "timing: {} {:.1} ms",
                self.name,
                self.start.elapsed().as_secs_f64() * 1000.0
            );
        }
    }
}

#[derive(Deserialize)]
struct HarnessResult {
    counts: Counts,
    failures: Vec<String>,
}

#[derive(Deserialize)]
struct Counts {
    scripts: usize,
    scenes: usize,
    resources: usize,
}

#[derive(Debug, PartialEq, Eq)]
struct ScriptClassCacheIssue {
    message: String,
    resource: Option<String>,
    line: Option<u32>,
}

fn cache_string(line: &str, key: &str) -> Result<Option<String>, String> {
    let Some(value) = line.trim().strip_prefix(&format!("\"{key}\":")) else {
        return Ok(None);
    };
    let value = value.trim().trim_end_matches(',').trim();
    let value = value.strip_prefix('&').unwrap_or(value);
    serde_json::from_str(value)
        .map(Some)
        .map_err(|error| format!("invalid {key} entry: {error}"))
}

fn cached_script_classes(project: &Path) -> Result<Option<HashMap<String, String>>, String> {
    let path = project.join(".godot/global_script_class_cache.cfg");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "could not read {}: {error}",
                crate::engine::display_path(&path)
            ));
        }
    };
    let mut classes = HashMap::new();
    let mut class = None;
    let mut script_path = None;
    for line in text.lines() {
        if let Some(value) = cache_string(line, "class")? {
            class = Some(value);
        }
        if let Some(value) = cache_string(line, "path")? {
            script_path = Some(value);
        }
        if line.trim_start().starts_with('}') {
            match (class.take(), script_path.take()) {
                (Some(class), Some(path)) if path.ends_with(".gd") => {
                    classes.insert(class, path);
                }
                (Some(_), Some(_)) | (None, None) => {}
                _ => return Err("incomplete global script class cache entry".into()),
            }
        }
    }
    if class.is_some() || script_path.is_some() {
        return Err("incomplete global script class cache entry".into());
    }
    Ok(Some(classes))
}

fn script_class_cache_issues(project: &Path) -> Result<Vec<ScriptClassCacheIssue>, Box<dyn Error>> {
    let declarations = crate::api::index_project(project)?;
    if declarations.is_empty() {
        return Ok(Vec::new());
    }
    let cached = match cached_script_classes(project) {
        Ok(Some(cached)) => cached,
        Ok(None) => {
            return Ok(vec![ScriptClassCacheIssue {
                message: format!(
                    "Godot's global script class cache is missing; {} project class_name declaration(s) are not registered. Run `gdkit cache refresh`; use `gdkit cache rebuild` if the cache remains missing.",
                    declarations.len()
                ),
                resource: None,
                line: None,
            }]);
        }
        Err(error) => {
            return Ok(vec![ScriptClassCacheIssue {
                message: format!(
                    "Godot's global script class cache is unreadable: {error}. Run `gdkit cache rebuild` to regenerate it."
                ),
                resource: None,
                line: None,
            }]);
        }
    };
    let declared: HashMap<_, _> = declarations
        .iter()
        .map(|class| (class.name.as_str(), class.path.as_str()))
        .collect();
    let mut issues = Vec::new();
    for class in &declarations {
        match cached.get(&class.name) {
            None => issues.push(ScriptClassCacheIssue {
                message: format!(
                    "global class {} is declared at {}:{} but is missing from Godot's global script class cache. Run `gdkit cache refresh`; use `gdkit cache rebuild` if the mismatch persists.",
                    class.name, class.path, class.line
                ),
                resource: Some(class.path.clone()),
                line: u32::try_from(class.line).ok(),
            }),
            Some(path) if path != &class.path => issues.push(ScriptClassCacheIssue {
                message: format!(
                    "global class {} is declared at {}:{} but Godot's global script class cache maps it to {}. Run `gdkit cache refresh`; use `gdkit cache rebuild` if the mismatch persists.",
                    class.name, class.path, class.line, path
                ),
                resource: Some(class.path.clone()),
                line: u32::try_from(class.line).ok(),
            }),
            Some(_) => {}
        }
    }
    for (class, path) in cached {
        if !declared.contains_key(class.as_str())
            && !project.join(path.trim_start_matches("res://")).is_file()
        {
            issues.push(ScriptClassCacheIssue {
                message: format!(
                    "Godot's global script class cache still maps global class {class} to {path}, but no matching class_name declaration exists on disk. Run `gdkit cache refresh`; use `gdkit cache rebuild` if the stale entry persists."
                ),
                resource: Some(path),
                line: None,
            });
        }
    }
    issues.sort_by(|left, right| left.message.cmp(&right.message));
    Ok(issues)
}

struct TemporaryScript(PathBuf);

impl TemporaryScript {
    fn create(contents: &[u8], extension: &str) -> io::Result<Self> {
        for attempt in 0..100 {
            let path = env::temp_dir().join(format!(
                "gdkit-check-{}-{attempt}.{extension}",
                std::process::id()
            ));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    let temporary = Self(path);
                    file.write_all(contents)?;
                    return Ok(temporary);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not create a temporary checker script",
        ))
    }
}

impl Drop for TemporaryScript {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub(crate) struct IsolatedProject(pub(crate) PathBuf);

impl IsolatedProject {
    pub(crate) fn empty() -> Result<Self, Box<dyn Error>> {
        for attempt in 0..100 {
            let path =
                env::temp_dir().join(format!("gdkit-isolated-{}-{attempt}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => {
                    let isolated = Self(path);
                    fs::write(isolated.0.join("project.godot"), "config_version=5\n")?;
                    return Ok(isolated);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err("could not create isolated project directory".into())
    }

    fn create(project: &Path, slice: &[PathBuf]) -> Result<Self, Box<dyn Error>> {
        let isolated = Self::empty()?;
        if slice.is_empty() {
            copy_project(project, &isolated.0)?;
        } else {
            copy_slice(project, &isolated.0, slice)?;
        }
        Ok(isolated)
    }
}

impl Drop for IsolatedProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn copy_slice(
    source: &Path,
    destination: &Path,
    selections: &[PathBuf],
) -> Result<(), Box<dyn Error>> {
    fs::write(destination.join("project.godot"), "config_version=5\n")?;
    for selection in selections {
        if selection.as_os_str().is_empty()
            || selection
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err("slice paths must be relative paths without . or .. components".into());
        }
        let mut selected = source.to_path_buf();
        for part in selection.components() {
            let name = part.as_os_str();
            if name == ".godot" || name == ".git" {
                return Err("slice paths cannot include .godot or .git".into());
            }
            selected.push(name);
            if fs::symlink_metadata(&selected)?.file_type().is_symlink() {
                return Err(format!(
                    "slice paths cannot follow symbolic links: {}",
                    selected.display()
                )
                .into());
            }
        }
        let target = destination.join(selection);
        if selected.is_dir() {
            fs::create_dir_all(&target)?;
            copy_project(&selected, &target)?;
        } else {
            fs::create_dir_all(target.parent().ok_or("slice path has no parent")?)?;
            fs::copy(&selected, &target)?;
        }
    }
    Ok(())
}

fn copy_project(source: &Path, destination: &Path) -> Result<(), Box<dyn Error>> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".godot" || name == ".git" {
            continue;
        }
        let source_path = entry.path();
        let destination_path = destination.join(&name);
        let metadata = fs::symlink_metadata(&source_path)?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "isolated checks do not follow symbolic links: {}",
                source_path.display()
            )
            .into());
        }
        if metadata.is_dir() {
            fs::create_dir_all(&destination_path)?;
            copy_project(&source_path, &destination_path)?;
        } else if metadata.is_file() {
            fs::copy(&source_path, &destination_path)?;
        }
    }
    Ok(())
}

struct CheckArtifacts {
    directory: PathBuf,
}

impl CheckArtifacts {
    fn create(project: &Path) -> io::Result<Self> {
        let root = project.join(".godot/gdkit/checks");
        fs::create_dir_all(&root)?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        for attempt in 0..100 {
            let directory = root.join(format!("{timestamp}-{}-{attempt}", std::process::id()));
            match fs::create_dir(&directory) {
                Ok(()) => return Ok(Self { directory }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not create a check artifact directory",
        ))
    }

    fn preserve(
        &self,
        name: &str,
        phase: &PhaseIdentity,
        output: &CapturedOutput,
    ) -> io::Result<Vec<Artifact>> {
        let stdout = self.directory.join(format!("{name}.stdout.log"));
        let stderr = self.directory.join(format!("{name}.stderr.log"));
        let event_stream = self.directory.join(format!("{name}.events.jsonl"));
        fs::write(&stdout, &output.output.stdout)?;
        fs::write(&stderr, &output.output.stderr)?;
        let mut events = Vec::new();
        for line in &output.lines {
            serde_json::to_writer(
                &mut events,
                &serde_json::json!({
                    "sequence": line.sequence,
                    "stream": match line.stream {
                        OutputStream::Stdout => "stdout",
                        OutputStream::Stderr => "stderr",
                    },
                    "observed_at_unix_ms": line.observed_at_unix_ms,
                    "text": String::from_utf8_lossy(&line.bytes)
                        .trim_end_matches(['\r', '\n']),
                }),
            )?;
            events.push(b'\n');
        }
        fs::write(&event_stream, events)?;
        Ok(vec![
            Artifact {
                kind: ArtifactKind::Stdout,
                phase: Some(phase.clone()),
                path: stdout,
            },
            Artifact {
                kind: ArtifactKind::Stderr,
                phase: Some(phase.clone()),
                path: stderr,
            },
            Artifact {
                kind: ArtifactKind::EventStream,
                phase: Some(phase.clone()),
                path: event_stream,
            },
        ])
    }

    fn preserve_report(&self, report: &mut CheckReport) -> io::Result<()> {
        let path = self.directory.join("report.json");
        report.artifacts.push(Artifact {
            kind: ArtifactKind::Report,
            phase: None,
            path: path.clone(),
        });
        let result = serde_json::to_vec_pretty(report)
            .map_err(io::Error::from)
            .and_then(|bytes| fs::write(path, bytes));
        if result.is_err() {
            report.artifacts.pop();
        }
        result
    }
}

fn phase(id: impl Into<String>, kind: CheckPhase) -> PhaseIdentity {
    PhaseIdentity {
        id: id.into(),
        kind,
    }
}

fn requested_phases(args: &CheckArgs) -> Vec<PhaseIdentity> {
    let mut phases = vec![
        phase("file_scan", CheckPhase::FileScan),
        phase("engine_validation", CheckPhase::EngineValidation),
        phase("import", CheckPhase::Import),
        phase("resource_loading", CheckPhase::ResourceLoading),
    ];
    phases.extend(args.script.iter().enumerate().map(|(index, script)| {
        phase(
            format!("project_script:{}:{script}", index + 1),
            CheckPhase::ProjectScript,
        )
    }));
    phases.extend(args.scene.iter().enumerate().map(|(index, scene)| {
        phase(
            format!("scene_smoke:{}:{scene}", index + 1),
            CheckPhase::SceneSmoke,
        )
    }));
    phases
}

fn project_fingerprint(project: &Path, paths: &[String]) -> Result<String, Box<dyn Error>> {
    let mut hasher = blake3::Hasher::new();
    for path in std::iter::once("res://project.godot")
        .chain(
            project
                .join("gdkit.toml")
                .is_file()
                .then_some("res://gdkit.toml"),
        )
        .chain(paths.iter().map(String::as_str))
    {
        hasher.update(path.as_bytes());
        hasher.update(&[0]);
        hasher.update(&fs::read(project.join(path.trim_start_matches("res://")))?);
        hasher.update(&[0]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn diagnostic_header(line: &str) -> Option<(DiagnosticSeverity, Option<String>, String)> {
    let line = line.trim_start();
    if let Some(message) = line.strip_prefix("SCRIPT ERROR:") {
        Some((
            DiagnosticSeverity::Error,
            Some("SCRIPT_ERROR".into()),
            message.trim().into(),
        ))
    } else if let Some(message) = line.strip_prefix("ERROR:") {
        Some((DiagnosticSeverity::Error, None, message.trim().into()))
    } else {
        line.strip_prefix("WARNING:")
            .map(|message| (DiagnosticSeverity::Warning, None, message.trim().into()))
    }
}

fn stack_frame(line: &str) -> Option<StackFrame> {
    let line = line.trim();
    if !line.starts_with("at:")
        && !line
            .strip_prefix('[')
            .and_then(|line| line.split_once(']'))
            .is_some_and(|(index, _)| index.parse::<usize>().is_ok())
    {
        return None;
    }
    let close = line.rfind(')')?;
    let open = line[..close].rfind('(')?;
    let mut location = &line[open + 1..close];
    let (mut resource, mut line_number, mut column) = (location, None, None);
    if let Some((before, value)) = location.rsplit_once(':')
        && let Ok(value) = value.parse::<u32>()
    {
        resource = before;
        line_number = Some(value);
        location = before;
        if let Some((before, value)) = location.rsplit_once(':')
            && let Ok(value) = value.parse::<u32>()
        {
            resource = before;
            column = line_number;
            line_number = Some(value);
        }
    }
    let mut function = line[..open].trim();
    if let Some(rest) = function.strip_prefix("at:") {
        function = rest.trim();
    }
    if let Some((_, rest)) = function
        .strip_prefix('[')
        .and_then(|function| function.split_once(']'))
    {
        function = rest.trim();
    }
    Some(StackFrame {
        function: (!function.is_empty()).then(|| function.into()),
        resource: (!resource.is_empty()).then(|| resource.into()),
        line: line_number,
        column,
    })
}

fn structured_diagnostics(
    output: &CapturedOutput,
    phase: &PhaseIdentity,
    sequence_base: u64,
    session_id: &str,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (index, event) in output.lines.iter().enumerate() {
        let text = String::from_utf8_lossy(&event.bytes);
        let Some((severity, engine_code, message)) = diagnostic_header(&text) else {
            continue;
        };
        let mut frames = Vec::new();
        for following in &output.lines[index + 1..] {
            if following.stream != event.stream {
                continue;
            }
            let text = String::from_utf8_lossy(&following.bytes);
            if diagnostic_header(&text).is_some() {
                break;
            }
            if let Some(frame) = stack_frame(&text) {
                frames.push(frame);
            }
        }
        let source = frames
            .iter()
            .find(|frame| {
                frame
                    .resource
                    .as_deref()
                    .is_some_and(|resource| resource.starts_with("res://"))
            })
            .cloned();
        let stream = match event.stream {
            OutputStream::Stdout => DiagnosticStream::Stdout,
            OutputStream::Stderr => DiagnosticStream::Stderr,
        };
        if let Some(existing) = diagnostics.iter_mut().find(|diagnostic: &&mut Diagnostic| {
            diagnostic.phase == *phase
                && diagnostic.severity == severity
                && diagnostic.stream == stream
                && diagnostic.engine_code == engine_code
                && diagnostic.message == message
                && diagnostic.resource == source.as_ref().and_then(|frame| frame.resource.clone())
                && diagnostic.line == source.as_ref().and_then(|frame| frame.line)
                && diagnostic.column == source.as_ref().and_then(|frame| frame.column)
                && diagnostic.stack_frames == frames
        }) {
            existing.occurrence_count += 1;
            continue;
        }
        diagnostics.push(Diagnostic {
            sequence: sequence_base + u64::try_from(event.sequence).unwrap_or(u64::MAX),
            phase: phase.clone(),
            severity,
            stream,
            engine_code,
            message,
            resource: source.as_ref().and_then(|frame| frame.resource.clone()),
            line: source.as_ref().and_then(|frame| frame.line),
            column: source.as_ref().and_then(|frame| frame.column),
            stack_frames: frames,
            process: Some(ProcessIdentity {
                session_id: format!("{session_id}:{}", phase.id),
                pid: (output.pid != 0).then_some(output.pid),
            }),
            timestamp_unix_ms: Some(event.observed_at_unix_ms),
            occurrence_count: 1,
        });
    }
    diagnostics
}

pub(crate) fn engine_output(
    engine: &Path,
    project: &Path,
    args: &[&OsStr],
) -> io::Result<CapturedOutput> {
    let mut command = Command::new(engine);
    command
        .args([OsStr::new("--headless"), OsStr::new("--no-header")])
        .arg("--path")
        .arg(project)
        .args(args);
    crate::process::run(&mut command, None)
}

fn smoke_output(
    engine: &Path,
    project: &Path,
    scene: &Path,
    frames: u32,
    timeout: u64,
) -> io::Result<CapturedOutput> {
    let mut command = Command::new(engine);
    command
        .args(["--headless", "--no-header", "--path"])
        .arg(project)
        .arg(scene)
        .arg("--quit-after")
        .arg(frames.to_string());
    crate::process::run(&mut command, Some(Duration::from_secs(timeout)))
}

fn script_output(
    engine: &Path,
    project: &Path,
    script: &str,
    timeout: u64,
) -> io::Result<CapturedOutput> {
    let bootstrap = TemporaryScript::create(SCRIPT_BOOTSTRAP.as_bytes(), "gd")?;
    let mut command = Command::new(engine);
    command
        .args(["--headless", "--no-header", "--path"])
        .arg(project)
        .arg("--script")
        .arg(&bootstrap.0)
        .arg("--")
        .arg(script);
    crate::process::run(&mut command, Some(Duration::from_secs(timeout)))
}

pub(crate) fn has_errors(output: &Output) -> bool {
    [output.stdout.as_slice(), output.stderr.as_slice()]
        .into_iter()
        .any(|bytes| {
            String::from_utf8_lossy(bytes).lines().any(|line| {
                let line = line.trim_start();
                line.starts_with("ERROR:") || line.starts_with("SCRIPT ERROR:")
            })
        })
}

fn filter_import_errors(
    output: &CapturedOutput,
    rules: &[crate::engine::ImportError],
) -> (CapturedOutput, usize) {
    let mut ignored = 0;
    let mut excluded = HashSet::new();
    for stream in [OutputStream::Stdout, OutputStream::Stderr] {
        let lines: Vec<_> = output
            .lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line.stream == stream)
            .map(|(global_index, line)| {
                (
                    global_index,
                    String::from_utf8_lossy(&line.bytes)
                        .trim_end_matches(['\r', '\n'])
                        .to_owned(),
                )
            })
            .collect();
        let mut index = 0;
        while index < lines.len() {
            let start = index;
            index += 1;
            let message = lines[start].1.trim();
            if message.starts_with("ERROR:") || message.starts_with("SCRIPT ERROR:") {
                while index < lines.len() {
                    let line = lines[index].1.trim();
                    let frame = line
                        .strip_prefix('[')
                        .and_then(|line| line.split_once(']'))
                        .is_some_and(|(number, _)| {
                            !number.is_empty() && number.chars().all(|c| c.is_ascii_digit())
                        });
                    if !(line.is_empty()
                        || line.starts_with("at:")
                        || line.starts_with("GDScript backtrace")
                        || frame)
                    {
                        break;
                    }
                    index += 1;
                }
                if rules.iter().any(|rule| {
                    message == rule.message
                        && lines[start + 1..index].iter().any(|(_, line)| {
                            line.contains(&format!("({}:", rule.source))
                                || line.contains(&format!("({})", rule.source))
                        })
                }) {
                    ignored += 1;
                    excluded.extend(
                        lines[start..index]
                            .iter()
                            .map(|(global_index, _)| *global_index),
                    );
                    continue;
                }
            }
        }
    }
    (
        output.retaining_lines(|index| !excluded.contains(&index)),
        ignored,
    )
}

#[derive(Default)]
struct Diagnostics {
    issues: Vec<DiagnosticMessage>,
    cleanup: Vec<DiagnosticMessage>,
}

struct DiagnosticMessage {
    text: String,
    occurrence_count: usize,
}

fn diagnostics(output: &CapturedOutput) -> Diagnostics {
    let mut result = Diagnostics::default();
    for event in &output.lines {
        let text = String::from_utf8_lossy(&event.bytes);
        let line = text.trim();
        if line.is_empty() || line.starts_with(RESULT_PREFIX) {
            continue;
        }
        if line.starts_with("at:") && !line.contains("res://") {
            continue;
        }
        let cleanup = is_cleanup_diagnostic(line);
        let target = if cleanup {
            &mut result.cleanup
        } else {
            &mut result.issues
        };
        if let Some(existing) = target.iter_mut().find(|existing| existing.text == line) {
            existing.occurrence_count += 1;
        } else {
            target.push(DiagnosticMessage {
                text: line.to_owned(),
                occurrence_count: 1,
            });
        }
    }
    result
}

fn is_cleanup_diagnostic(line: &str) -> bool {
    (line.contains("RID allocations of type") && line.ends_with("were leaked at exit."))
        || (line.contains("RIDs of type") && line.ends_with("were leaked."))
        || line.starts_with("WARNING: ObjectDB instances leaked at exit")
        || line.contains("ObjectDB instances were leaked at exit")
        || (line.starts_with("ERROR:") && line.contains("resources still in use at exit"))
}

fn unresolved_uid(line: &str) -> Option<&str> {
    let rest = line.split_once("Unrecognized UID: \"")?.1;
    rest.split_once('"').map(|(uid, _)| uid)
}

fn cleanup_message(line: &str) -> String {
    let severity = line.split_once(':').map_or("WARNING", |(value, _)| value);
    let count = line.split_whitespace().nth(1).unwrap_or("");
    let kind = if line.contains("DummyTexture") {
        Some("headless renderer textures")
    } else if line.contains("ShapedTextData") {
        Some("text layout buffers")
    } else if line.contains("FontLinkedVariation") {
        Some("font variations")
    } else if line.contains("Font") && line.contains("RID allocations") {
        Some("fonts")
    } else if line.contains("CanvasItem") {
        Some("2D canvas items")
    } else if line.contains("resources still in use at exit") {
        Some("resources")
    } else if line.contains("ObjectDB") && count.parse::<usize>().is_ok() {
        Some("objects")
    } else {
        None
    };
    match kind {
        Some(kind) => format!("{severity}: {count} {kind} still allocated at shutdown."),
        None if line.contains("ObjectDB") => format!(
            "{severity}: Objects still allocated at shutdown (Godot did not report a count)."
        ),
        None => line.to_owned(),
    }
}

fn uid_references(project: &Path, paths: &[String], uid: &str) -> Vec<String> {
    let mut references = Vec::new();
    for path in std::iter::once("res://project.godot").chain(paths.iter().map(String::as_str)) {
        let local = project.join(path.trim_start_matches("res://"));
        let Ok(source) = fs::read_to_string(local) else {
            continue;
        };
        for (index, line) in source.lines().enumerate() {
            if line.match_indices(uid).any(|(offset, _)| {
                !line[offset + uid.len()..].starts_with(|c: char| c.is_ascii_alphanumeric())
            }) {
                references.push(format!("{path}:{}: {}", index + 1, line.trim()));
            }
        }
    }
    references
}

fn write_diagnostics(
    output: &CapturedOutput,
    phase: &str,
    project: &Path,
    paths: &[String],
    verbose: bool,
) -> io::Result<()> {
    let mut stderr = io::stderr().lock();
    let report = diagnostics(output);
    if !report.issues.is_empty() {
        writeln!(stderr, "\n{phase} diagnostics:")?;
        for diagnostic in &report.issues {
            write!(stderr, "  {}", diagnostic.text)?;
            if diagnostic.occurrence_count > 1 {
                write!(stderr, " (repeated {} times)", diagnostic.occurrence_count)?;
            }
            writeln!(stderr)?;
            if let Some(uid) = unresolved_uid(&diagnostic.text) {
                writeln!(
                    stderr,
                    "    Godot cannot resolve this resource ID to a file."
                )?;
                let references = uid_references(project, paths, uid);
                if references.is_empty() {
                    writeln!(
                        stderr,
                        "    No reference found in checked text files or project.godot; it may come from a dependency or binary resource."
                    )?;
                } else {
                    writeln!(stderr, "    References (verify which one needs repair):")?;
                    for reference in references {
                        writeln!(stderr, "      {reference}")?;
                    }
                }
                writeln!(
                    stderr,
                    "    Reassign stale references to the intended resource in Godot and save the affected file."
                )?;
            }
        }
    }
    if !report.cleanup.is_empty() {
        writeln!(stderr, "\n{phase} shutdown cleanup diagnostics:")?;
        writeln!(
            stderr,
            "  Godot reported objects or resources still allocated when the headless process exited."
        )?;
        writeln!(
            stderr,
            "  These messages do not identify a source file; ERROR entries still fail this check."
        )?;
        for diagnostic in &report.cleanup {
            write!(stderr, "  {}", cleanup_message(&diagnostic.text))?;
            if diagnostic.occurrence_count > 1 {
                write!(stderr, " (repeated {} times)", diagnostic.occurrence_count)?;
            }
            writeln!(stderr)?;
        }
        writeln!(
            stderr,
            "  Use gdkit check --verbose for the original engine messages and stack traces."
        )?;
    }
    if !output.output.status.success() {
        writeln!(
            stderr,
            "\n{phase}: Godot process exited with {}.",
            output.output.status
        )?;
    }
    if verbose {
        writeln!(stderr, "\n{phase} full Godot output:")?;
        for bytes in [&output.output.stdout, &output.output.stderr] {
            for line in String::from_utf8_lossy(bytes).lines() {
                if !line.starts_with(RESULT_PREFIX) {
                    writeln!(stderr, "{line}")?;
                }
            }
        }
    }
    Ok(())
}

fn harness_result(output: &CapturedOutput) -> Result<HarnessResult, Box<dyn Error>> {
    let stdout = String::from_utf8_lossy(&output.output.stdout);
    let result = stdout
        .lines()
        .find_map(|line| line.strip_prefix(RESULT_PREFIX))
        .ok_or("Godot checker did not return a result")?;
    Ok(serde_json::from_str(result)?)
}

fn print_summary(
    failed: bool,
    resource_loading_passed: bool,
    counts: Option<&Counts>,
) -> io::Result<()> {
    let (color, status) = if failed {
        (31, "check failed")
    } else {
        (32, "check passed")
    };
    let mut output = anstream::stdout();
    write!(output, "\x1b[{color}m{status}")?;
    if let Some(counts) = counts {
        write!(
            output,
            ": resource validation {} {}, {}, {}",
            if resource_loading_passed {
                "loaded"
            } else {
                "attempted"
            },
            count_label(counts.scripts, "script"),
            count_label(counts.scenes, "scene"),
            count_label(counts.resources, "resource")
        )?;
    }
    writeln!(output, "\x1b[0m")
}

fn count_label(count: usize, noun: &str) -> String {
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
}

struct ExecutionSummary {
    counts: Option<Counts>,
    smoke_count: usize,
    strict_methods: bool,
    script_count: usize,
}

fn runtime_summary(report: &CheckReport, summary: &ExecutionSummary) -> String {
    let requested_scripts = report
        .requested_phases
        .iter()
        .filter(|phase| phase.kind == CheckPhase::ProjectScript)
        .count();
    let requested_scenes = report
        .requested_phases
        .iter()
        .filter(|phase| phase.kind == CheckPhase::SceneSmoke)
        .count();
    if requested_scripts == 0 && requested_scenes == 0 {
        return "runtime execution: none requested".into();
    }
    let mut executed = Vec::new();
    if requested_scripts > 0 {
        executed.push(format!(
            "{}/{} project script{}",
            summary.script_count,
            requested_scripts,
            if requested_scripts == 1 { "" } else { "s" }
        ));
    }
    if requested_scenes > 0 {
        executed.push(format!(
            "{}/{} gameplay scene smoke check{}",
            summary.smoke_count,
            requested_scenes,
            if requested_scenes == 1 { "" } else { "s" }
        ));
    }
    let mut text = format!("runtime execution: ran {}", executed.join(" and "));
    let runtime_skips: Vec<_> = report
        .skipped_phases
        .iter()
        .filter(|skipped| {
            matches!(
                skipped.phase.kind,
                CheckPhase::ProjectScript | CheckPhase::SceneSmoke
            )
        })
        .collect();
    if !runtime_skips.is_empty() {
        let reasons: BTreeSet<_> = runtime_skips
            .iter()
            .map(|skipped| skipped.reason.as_str())
            .collect();
        text.push_str(&format!("; {} skipped", runtime_skips.len()));
        if reasons.len() == 1 {
            text.push_str(&format!(" ({})", reasons.first().unwrap()));
        }
    }
    text
}

fn exit_code(outcome: CheckOutcome) -> ExitCode {
    match outcome {
        CheckOutcome::Passed => ExitCode::SUCCESS,
        CheckOutcome::ValidationFailed | CheckOutcome::Incomplete | CheckOutcome::Stopped => {
            ExitCode::from(1)
        }
        CheckOutcome::ToolFailed => ExitCode::from(2),
    }
}

fn emit_report(
    output: CheckOutput,
    report: &CheckReport,
    summary: Option<&ExecutionSummary>,
) -> Result<(), Box<dyn Error>> {
    match output {
        CheckOutput::Json => println!("{}", serde_json::to_string(report)?),
        CheckOutput::Human => {
            if let Some(summary) = summary {
                print_summary(
                    report.outcome != CheckOutcome::Passed,
                    !report.failures.iter().any(|failure| {
                        failure
                            .phase
                            .as_ref()
                            .is_some_and(|phase| phase.kind == CheckPhase::ResourceLoading)
                    }),
                    summary.counts.as_ref(),
                )?;
                println!("{}", runtime_summary(report, summary));
                println!(
                    "validation policy: {}",
                    if summary.strict_methods {
                        "strict method validation"
                    } else {
                        "project warning policy"
                    }
                );
            }
        }
    }
    Ok(())
}

pub fn run(args: CheckArgs) -> Result<ExitCode, Box<dyn Error>> {
    let _total = PhaseTimer::new("total", args.timings);
    if args.stop_worker {
        let project = crate::engine::project_root(&args.project)?;
        let _cache_lock = crate::cache::lock(&project)?;
        crate::import_worker::stop_project(&project)?;
        println!("import worker stopped");
        return Ok(ExitCode::SUCCESS);
    }
    let mut report = CheckReport::new(
        ProjectSnapshot {
            root: args.project.clone(),
            fingerprint: "unavailable".into(),
        },
        CheckPolicy {
            strict_methods: args.strict_methods,
            fresh_import: true,
            ignored_import_diagnostics: 0,
        },
    );
    report.requested_phases = requested_phases(&args);
    let project_result = if args.slice.is_empty() {
        crate::engine::project_root(&args.project)
    } else {
        fs::canonicalize(&args.project).map_err(Into::into)
    };
    let project = match project_result {
        Ok(project) => project,
        Err(error) => {
            report.outcome = CheckOutcome::ToolFailed;
            report.failures.push(CheckFailure {
                kind: FailureKind::Tool,
                phase: None,
                message: error.to_string(),
            });
            if args.output == CheckOutput::Json {
                eprintln!("error: {error}");
                emit_report(args.output, &report, None)?;
                return Ok(ExitCode::from(2));
            }
            return Err(error);
        }
    };
    report.project.root = project.clone();
    let _cache_lock = crate::cache::lock(&project)?;
    let artifacts = match CheckArtifacts::create(&project) {
        Ok(artifacts) => artifacts,
        Err(error) => {
            report.outcome = CheckOutcome::ToolFailed;
            report.failures.push(CheckFailure {
                kind: FailureKind::Tool,
                phase: None,
                message: error.to_string(),
            });
            if args.output == CheckOutput::Json {
                eprintln!("error: {error}");
                emit_report(args.output, &report, None)?;
                return Ok(ExitCode::from(2));
            }
            return Err(error.into());
        }
    };
    eprintln!(
        "artifacts: {}",
        crate::engine::display_path(&artifacts.directory)
    );
    let isolated = match IsolatedProject::create(&project, &args.slice) {
        Ok(isolated) => isolated,
        Err(error) => {
            report.outcome = CheckOutcome::ToolFailed;
            report.failures.push(CheckFailure {
                kind: FailureKind::Tool,
                phase: None,
                message: error.to_string(),
            });
            artifacts.preserve_report(&mut report)?;
            if args.output == CheckOutput::Json {
                eprintln!("error: {error}");
                emit_report(args.output, &report, None)?;
                return Ok(ExitCode::from(2));
            }
            return Err(error);
        }
    };
    let result = run_project(&args, &project, &isolated.0, &artifacts, &mut report);
    if let Err(error) = &result {
        report.outcome = CheckOutcome::ToolFailed;
        report.failures.push(CheckFailure {
            kind: FailureKind::Tool,
            phase: None,
            message: error.to_string(),
        });
    }
    if let Err(error) = artifacts.preserve_report(&mut report) {
        report.outcome = CheckOutcome::ToolFailed;
        report.failures.push(CheckFailure {
            kind: FailureKind::Tool,
            phase: None,
            message: error.to_string(),
        });
        if args.output == CheckOutput::Json {
            eprintln!("error: {error}");
            emit_report(args.output, &report, None)?;
            return Ok(ExitCode::from(2));
        }
        return Err(error.into());
    }
    match result {
        Ok(summary) => {
            let code = exit_code(report.outcome);
            emit_report(args.output, &report, Some(&summary))?;
            Ok(code)
        }
        Err(error) if args.output == CheckOutput::Json => {
            eprintln!("error: {error}");
            emit_report(args.output, &report, None)?;
            Ok(ExitCode::from(2))
        }
        Err(error) => Err(error),
    }
}

fn run_project(
    args: &CheckArgs,
    source_project: &Path,
    project: &Path,
    artifacts: &CheckArtifacts,
    report: &mut CheckReport,
) -> Result<ExecutionSummary, Box<dyn Error>> {
    let scan_timer = PhaseTimer::new("file scan", args.timings);
    let scenes = args
        .scene
        .iter()
        .map(|scene| {
            let path = project.join(scene.strip_prefix("res://").unwrap_or(scene));
            let path = fs::canonicalize(&path)
                .map_err(|error| format!("smoke scene {}: {error}", path.display()))?;
            if !path.starts_with(fs::canonicalize(project)?)
                || !path.is_file()
                || !path
                    .extension()
                    .is_some_and(|extension| extension == "tscn" || extension == "scn")
            {
                return Err("smoke scenes must be .tscn or .scn files inside the project".into());
            }
            Ok(path)
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    let scripts = args
        .script
        .iter()
        .map(|script| {
            let path = project.join(script.strip_prefix("res://").unwrap_or(script));
            let path = fs::canonicalize(&path)
                .map_err(|error| format!("project script {}: {error}", path.display()))?;
            if !path.starts_with(fs::canonicalize(project)?)
                || !path.is_file()
                || !path.extension().is_some_and(|extension| extension == "gd")
            {
                return Err("project scripts must be .gd files inside the project".into());
            }
            Ok(format!(
                "res://{}",
                path.strip_prefix(fs::canonicalize(project)?)?
                    .to_str()
                    .ok_or("project script path is not valid UTF-8")?
                    .replace('\\', "/")
            ))
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    let config = crate::engine::read_config(source_project)?;
    let strict_methods = args.strict_methods
        || config
            .as_ref()
            .is_some_and(|config| config.check.strict_methods);
    let rules = config.as_ref().map_or(&[][..], |config| {
        config.check.ignore_import_errors.as_slice()
    });
    let paths =
        crate::project_files::collect(project, &["gd", "tscn", "scn", "tres", "res", "gdshader"])?;
    let paths = paths
        .iter()
        .map(|path| {
            Ok(format!(
                "res://{}",
                path.strip_prefix(project)?
                    .to_str()
                    .ok_or("resource path is not valid UTF-8")?
                    .replace('\\', "/")
            ))
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    let manifest = TemporaryScript::create(&serde_json::to_vec(&paths)?, "json")?;
    report.project.fingerprint = project_fingerprint(project, &paths)?;
    report.policy = CheckPolicy {
        strict_methods,
        fresh_import: true,
        ignored_import_diagnostics: rules.len(),
    };
    report
        .completed_phases
        .push(phase("file_scan", CheckPhase::FileScan));
    drop(scan_timer);
    let mut validation_timer = PhaseTimer::new("engine validation (probe)", args.timings);
    let engine = crate::engine::resolve(source_project, args.godot.as_deref())?;
    let (version, cached) = crate::engine::validated_version(&engine, source_project)?;
    if cached {
        validation_timer.name = "engine validation (cached)";
    }
    report.engine = Some(EngineFingerprint {
        executable: engine.clone(),
        version: version.clone(),
        fingerprint: crate::engine::fingerprint(&engine)?,
    });
    report
        .completed_phases
        .push(phase("engine_validation", CheckPhase::EngineValidation));
    drop(validation_timer);
    eprintln!(
        "engine: {} ({version})",
        crate::engine::display_path(&engine)
    );
    let session_id = artifacts
        .directory
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("check");
    let mut sequence_base = 0;

    let import_timer = PhaseTimer::new("import", args.timings);
    let import_phase = phase("import", CheckPhase::Import);
    let cache_import = engine_output(
        &engine,
        project,
        &[
            OsStr::new("--editor"),
            OsStr::new("--quiet"),
            OsStr::new("--import"),
        ],
    )?;
    report
        .artifacts
        .extend(artifacts.preserve("cache-import", &import_phase, &cache_import)?);
    let (filtered_cache_import, cache_ignored) = filter_import_errors(&cache_import, rules);
    let cache_import_diagnostics = filtered_cache_import.retaining_lines(|index| {
        !is_cleanup_diagnostic(
            String::from_utf8_lossy(&filtered_cache_import.lines[index].bytes).trim(),
        )
    });
    report.diagnostics.extend(structured_diagnostics(
        &cache_import_diagnostics,
        &import_phase,
        sequence_base,
        session_id,
    ));
    sequence_base += u64::try_from(cache_import.lines.len()).unwrap_or(u64::MAX);
    let cache_import_errors = has_errors(&cache_import_diagnostics.output);
    if !cache_import.output.status.success() {
        report.failures.push(CheckFailure {
            kind: FailureKind::ProcessExit,
            phase: Some(import_phase.clone()),
            message: format!(
                "Godot cache import exited with {}",
                cache_import.output.status
            ),
        });
    }
    if cache_import_errors {
        report.failures.push(CheckFailure {
            kind: FailureKind::Diagnostic,
            phase: Some(import_phase.clone()),
            message: "cache import reported one or more errors".into(),
        });
    }
    write_diagnostics(
        &cache_import_diagnostics,
        "Cache import",
        project,
        &paths,
        args.verbose,
    )?;
    let import_scan = TemporaryScript::create(IMPORT_SCAN.as_bytes(), "gd")?;
    let import_completion = TemporaryScript::create(b"", "complete")?;
    fs::remove_file(&import_completion.0)?;
    let import = engine_output(
        &engine,
        project,
        &[
            OsStr::new("--editor"),
            OsStr::new("--quiet"),
            OsStr::new("--script"),
            import_scan.0.as_os_str(),
            OsStr::new("--"),
            manifest.0.as_os_str(),
            import_completion.0.as_os_str(),
        ],
    )?;
    drop(import_timer);
    report
        .artifacts
        .extend(artifacts.preserve("import", &import_phase, &import)?);
    let (filtered_import, ignored) = filter_import_errors(&import, rules);
    let import_completion_index = filtered_import
        .lines
        .iter()
        .position(|line| String::from_utf8_lossy(&line.bytes).contains(IMPORT_SCAN_RESULT));
    let import_diagnostics = filtered_import.retaining_lines(|index| {
        import_completion_index.is_none_or(|completion| index < completion)
            && !is_cleanup_diagnostic(
                String::from_utf8_lossy(&filtered_import.lines[index].bytes).trim(),
            )
    });
    report.suppressed_diagnostics += cache_ignored + ignored;
    report.diagnostics.extend(structured_diagnostics(
        &import_diagnostics,
        &import_phase,
        sequence_base,
        session_id,
    ));
    sequence_base += u64::try_from(import.lines.len()).unwrap_or(u64::MAX);
    let import_completed = fs::read_to_string(&import_completion.0)
        .is_ok_and(|result| result == IMPORT_SCAN_RESULT)
        && import_completion_index.is_some();
    if import_completed {
        report.completed_phases.push(import_phase.clone());
    } else {
        report.failures.push(CheckFailure {
            kind: FailureKind::MissingCompletion,
            phase: Some(import_phase.clone()),
            message: "editor import and GDScript scan did not report completion".into(),
        });
    }
    let import_errors = has_errors(&import_diagnostics.output);
    let mut failed = !cache_import.output.status.success()
        || cache_import_errors
        || !import_completed
        || import_errors;
    if !import.output.status.success() && !import_completed {
        report.failures.push(CheckFailure {
            kind: FailureKind::ProcessExit,
            phase: Some(import_phase.clone()),
            message: format!("Godot process exited with {}", import.output.status),
        });
    }
    if import_errors {
        report.failures.push(CheckFailure {
            kind: FailureKind::Diagnostic,
            phase: Some(import_phase),
            message: "import reported one or more errors".into(),
        });
    }
    if cache_ignored + ignored > 0 {
        eprintln!(
            "Import: ignored {} configured diagnostic(s); use --verbose for original output.",
            cache_ignored + ignored
        );
    }
    write_diagnostics(&import_diagnostics, "Import", project, &paths, args.verbose)?;
    let cache_issues = script_class_cache_issues(project)?;
    if !cache_issues.is_empty() {
        eprintln!("\nGlobal script class cache diagnostics:");
        for issue in &cache_issues {
            eprintln!("  ERROR: {}", issue.message);
            report.diagnostics.push(Diagnostic {
                sequence: sequence_base,
                phase: phase("import", CheckPhase::Import),
                severity: DiagnosticSeverity::Error,
                stream: DiagnosticStream::Logger,
                engine_code: Some("GDKIT_SCRIPT_CLASS_CACHE".into()),
                message: issue.message.clone(),
                resource: issue.resource.clone(),
                line: issue.line,
                column: None,
                stack_frames: Vec::new(),
                process: None,
                timestamp_unix_ms: None,
                occurrence_count: 1,
            });
            sequence_base += 1;
        }
        report.failures.push(CheckFailure {
            kind: FailureKind::Diagnostic,
            phase: Some(phase("import", CheckPhase::Import)),
            message: "global script class cache does not match project declarations".into(),
        });
    }

    let harness = TemporaryScript::create(HARNESS.as_bytes(), "gd")?;
    let loading_timer = PhaseTimer::new("resource loading", args.timings);
    let check = engine_output(
        &engine,
        project,
        &[
            OsStr::new("--script"),
            harness.0.as_os_str(),
            OsStr::new("--"),
            manifest.0.as_os_str(),
            OsStr::new(if strict_methods {
                "strict-methods"
            } else {
                "project-policy"
            }),
        ],
    )?;
    drop(loading_timer);
    let loading_phase = phase("resource_loading", CheckPhase::ResourceLoading);
    report
        .artifacts
        .extend(artifacts.preserve("resource-loading", &loading_phase, &check)?);
    report.diagnostics.extend(structured_diagnostics(
        &check,
        &loading_phase,
        sequence_base,
        session_id,
    ));
    sequence_base += u64::try_from(check.lines.len()).unwrap_or(u64::MAX);
    let check_errors = has_errors(&check.output);
    let check_failed = !check.output.status.success() || check_errors;
    if !check.output.status.success() {
        report.failures.push(CheckFailure {
            kind: FailureKind::ProcessExit,
            phase: Some(loading_phase.clone()),
            message: format!("Godot process exited with {}", check.output.status),
        });
    }
    if check_errors {
        report.failures.push(CheckFailure {
            kind: FailureKind::Diagnostic,
            phase: Some(loading_phase.clone()),
            message: "resource loading reported one or more errors".into(),
        });
    }
    write_diagnostics(&check, "Resource loading", project, &paths, args.verbose)?;
    let result = match harness_result(&check) {
        Ok(result) => {
            report.completed_phases.push(loading_phase.clone());
            result
        }
        Err(error) => {
            eprintln!("error: {error}; resource checks did not complete");
            report.failures.push(CheckFailure {
                kind: FailureKind::MissingCompletion,
                phase: Some(loading_phase),
                message: error.to_string(),
            });
            for runtime in report.requested_phases.iter().filter(|phase| {
                matches!(
                    phase.kind,
                    CheckPhase::SceneSmoke | CheckPhase::ProjectScript
                )
            }) {
                report.skipped_phases.push(SkippedPhase {
                    phase: runtime.clone(),
                    reason: "resource checks did not complete".into(),
                });
            }
            report.outcome = CheckOutcome::Incomplete;
            return Ok(ExecutionSummary {
                counts: None,
                smoke_count: 0,
                strict_methods,
                script_count: 0,
            });
        }
    };
    failed |= check_failed || !result.failures.is_empty() || !cache_issues.is_empty();
    for path in &result.failures {
        eprintln!("error: failed to load {path}");
        report.failures.push(CheckFailure {
            kind: FailureKind::ResourceLoad,
            phase: Some(loading_phase.clone()),
            message: format!("failed to load {path}"),
        });
    }

    let mut script_count = 0;
    let mut smoke_count = 0;
    let mut attempted_runtime_phases = HashSet::new();
    if !failed {
        for (index, script) in scripts.iter().enumerate() {
            let script_phase = report
                .requested_phases
                .iter()
                .find(|phase| {
                    phase.kind == CheckPhase::ProjectScript
                        && phase
                            .id
                            .starts_with(&format!("project_script:{}:", index + 1))
                })
                .cloned()
                .ok_or("project script phase was not registered")?;
            attempted_runtime_phases.insert(script_phase.id.clone());
            let label = format!("Project script {}", args.script[index]);
            let output = script_output(&engine, project, script, args.script_timeout)?;
            report.artifacts.extend(artifacts.preserve(
                &format!("project-script-{}", index + 1),
                &script_phase,
                &output,
            )?);
            report.diagnostics.extend(structured_diagnostics(
                &output,
                &script_phase,
                sequence_base,
                session_id,
            ));
            sequence_base += u64::try_from(output.lines.len()).unwrap_or(u64::MAX);
            write_diagnostics(&output, &label, project, &paths, args.verbose)?;
            if output.timed_out {
                eprintln!("error: {label} exceeded {} seconds", args.script_timeout);
                report.failures.push(CheckFailure {
                    kind: FailureKind::Timeout,
                    phase: Some(script_phase.clone()),
                    message: format!("exceeded {} seconds", args.script_timeout),
                });
            } else {
                report.completed_phases.push(script_phase.clone());
            }
            if !output.output.status.success() && !output.timed_out {
                report.failures.push(CheckFailure {
                    kind: FailureKind::ProcessExit,
                    phase: Some(script_phase.clone()),
                    message: format!("Godot process exited with {}", output.output.status),
                });
            }
            if has_errors(&output.output) {
                report.failures.push(CheckFailure {
                    kind: FailureKind::Diagnostic,
                    phase: Some(script_phase),
                    message: "project script reported one or more errors".into(),
                });
            }
            failed |=
                output.timed_out || !output.output.status.success() || has_errors(&output.output);
            script_count += 1;
            if failed {
                break;
            }
        }
    }
    if !failed {
        for (index, scene) in scenes.iter().enumerate() {
            let smoke_phase = report
                .requested_phases
                .iter()
                .find(|phase| {
                    phase.kind == CheckPhase::SceneSmoke
                        && phase.id.starts_with(&format!("scene_smoke:{}:", index + 1))
                })
                .cloned()
                .ok_or("scene smoke phase was not registered")?;
            attempted_runtime_phases.insert(smoke_phase.id.clone());
            let phase = format!("Scene smoke {}", scene.display());
            let output = smoke_output(
                &engine,
                project,
                scene,
                args.smoke_frames,
                args.smoke_timeout,
            )?;
            report.artifacts.extend(artifacts.preserve(
                &format!("scene-smoke-{}", index + 1),
                &smoke_phase,
                &output,
            )?);
            report.diagnostics.extend(structured_diagnostics(
                &output,
                &smoke_phase,
                sequence_base,
                session_id,
            ));
            sequence_base += u64::try_from(output.lines.len()).unwrap_or(u64::MAX);
            write_diagnostics(&output, &phase, project, &paths, args.verbose)?;
            if output.timed_out {
                eprintln!("error: {phase} exceeded {} seconds", args.smoke_timeout);
                report.failures.push(CheckFailure {
                    kind: FailureKind::Timeout,
                    phase: Some(smoke_phase.clone()),
                    message: format!("exceeded {} seconds", args.smoke_timeout),
                });
            } else {
                report.completed_phases.push(smoke_phase.clone());
            }
            if !output.output.status.success() && !output.timed_out {
                report.failures.push(CheckFailure {
                    kind: FailureKind::ProcessExit,
                    phase: Some(smoke_phase.clone()),
                    message: format!("Godot process exited with {}", output.output.status),
                });
            }
            if has_errors(&output.output) {
                report.failures.push(CheckFailure {
                    kind: FailureKind::Diagnostic,
                    phase: Some(smoke_phase),
                    message: "scene smoke reported one or more errors".into(),
                });
            }
            failed |=
                output.timed_out || !output.output.status.success() || has_errors(&output.output);
            smoke_count += 1;
        }
    }
    if failed {
        if script_count < scripts.len() || smoke_count < scenes.len() {
            eprintln!("Remaining runtime checks skipped because an earlier phase failed.");
        }
        let skipped: Vec<_> = report
            .requested_phases
            .iter()
            .filter(|phase| {
                matches!(
                    phase.kind,
                    CheckPhase::SceneSmoke | CheckPhase::ProjectScript
                ) && !report.completed_phases.contains(phase)
                    && !attempted_runtime_phases.contains(&phase.id)
                    && !report
                        .skipped_phases
                        .iter()
                        .any(|skipped| skipped.phase == **phase)
            })
            .cloned()
            .collect();
        for runtime in skipped {
            report.skipped_phases.push(SkippedPhase {
                phase: runtime,
                reason: "an earlier phase failed".into(),
            });
        }
    }
    report.checked = Some(CheckCounts {
        scripts: result.counts.scripts,
        scenes: result.counts.scenes,
        resources: result.counts.resources,
        smoke_scenes: smoke_count,
        project_scripts: script_count,
    });
    report.outcome = if failed {
        CheckOutcome::ValidationFailed
    } else {
        CheckOutcome::Passed
    };
    Ok(ExecutionSummary {
        counts: Some(result.counts),
        smoke_count,
        strict_methods,
        script_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolated_projects_copy_sources_without_tool_state() {
        let source = env::temp_dir().join(format!("gdkit-copy-source-{}", std::process::id()));
        let destination =
            env::temp_dir().join(format!("gdkit-copy-destination-{}", std::process::id()));
        fs::create_dir(&source).unwrap();
        fs::create_dir(&destination).unwrap();
        fs::create_dir(source.join("scripts")).unwrap();
        fs::create_dir(source.join(".godot")).unwrap();
        fs::create_dir(source.join(".git")).unwrap();
        fs::write(source.join("project.godot"), "config_version=5\n").unwrap();
        fs::write(source.join("scripts/player.gd"), "extends Node\n").unwrap();
        fs::write(source.join(".godot/cache"), "cache").unwrap();
        fs::write(source.join(".git/config"), "git").unwrap();

        copy_project(&source, &destination).unwrap();

        assert!(destination.join("project.godot").is_file());
        assert!(destination.join("scripts/player.gd").is_file());
        assert!(!destination.join(".godot").exists());
        assert!(!destination.join(".git").exists());
        fs::remove_dir_all(source).unwrap();
        fs::remove_dir_all(destination).unwrap();
    }

    #[test]
    fn zero_exit_script_errors_fail_on_either_stream() {
        for stdout in [false, true] {
            let message = b"  SCRIPT ERROR: Invalid call. Nonexistent function 'push_tornado' in base 'RichTextLabel'.\n   at: _ready (res://lobby.gd:4)\n".to_vec();
            let output = Output {
                status: Default::default(),
                stdout: if stdout { message.clone() } else { Vec::new() },
                stderr: if stdout { Vec::new() } else { message },
            };
            let output = CapturedOutput::from_output(output);
            assert!(output.output.status.success());
            assert!(has_errors(&output.output));
            assert!(
                diagnostics(&output)
                    .issues
                    .iter()
                    .any(|diagnostic| diagnostic.text.contains("res://lobby.gd:4"))
            );
        }
    }

    #[test]
    fn import_exceptions_require_message_and_own_source_frame() {
        let rules = vec![crate::engine::ImportError {
            message: "ERROR: Known plugin problem".into(),
            source: "res://addons/plugin.gd".into(),
        }];
        let output = Output {
            status: Default::default(),
            stdout: Vec::new(),
            stderr: concat!(
                "ERROR: Known plugin problem\n",
                "   at: native (core/example.cpp:10)\n",
                "   GDScript backtrace (most recent call first):\n",
                "       [0] setup (res://addons/plugin.gd:67)\n",
                "\n",
                "GDScript backtrace (most recent call first):\n",
                "    [0] setup (res://addons/plugin.gd:67)\n",
                "ERROR: Unrelated problem\n",
                "       [0] setup (res://addons/plugin.gd:68)\n",
                "ERROR: Known plugin problem\n",
                "       [0] setup (res://game.gd:10)\n",
                "ERROR: Known plugin problem\n",
                "       [0] setup (res://addons/plugin.gd.backup:10)\n",
                "ERROR: Known plugin problem\n",
                "WARNING: Separate diagnostic\n",
                "       [0] setup (res://addons/plugin.gd:70)\n",
            )
            .as_bytes()
            .to_vec(),
        };
        let output = CapturedOutput::from_output(output);
        let (filtered, count) = filter_import_errors(&output, &rules);
        assert_eq!(count, 1);
        assert!(has_errors(&filtered.output));
        let remaining = String::from_utf8(filtered.output.stderr).unwrap();
        assert!(!remaining.contains("plugin.gd:67"));
        assert!(remaining.contains("Unrelated problem"));
        assert_eq!(remaining.matches("ERROR: Known plugin problem").count(), 3);
        assert_eq!(filter_import_errors(&output, &[]).1, 0);
        let known_only = Output {
            status: Default::default(),
            stdout: b"ERROR: Known plugin problem\n   at: setup (res://addons/plugin.gd:1)\n"
                .to_vec(),
            stderr: Vec::new(),
        };
        let known_only = CapturedOutput::from_output(known_only);
        assert!(!has_errors(
            &filter_import_errors(&known_only, &rules).0.output
        ));
        assert!(has_errors(&known_only.output));
    }

    #[test]
    fn separates_cleanup_without_hiding_errors_or_project_locations() {
        let output = Output {
            status: Default::default(),
            stdout: b"GDKIT_CHECK_RESULT:{}\n".to_vec(),
            stderr: concat!(
                "ERROR: 12 RID allocations of type 'DummyTexture' were leaked at exit.\n",
                "ERROR: Unrecognized UID: \"uid://missing\".\n",
                "   at: ResourceUID::get_id_path (core/io/resource_uid.cpp:214)\n",
                "WARNING: 21 RIDs of type \"CanvasItem\" were leaked.\n",
                "WARNING: ObjectDB instances leaked at exit (run with --verbose for details).\n",
                "ERROR: 5 resources still in use at exit (run with --verbose for details).\n",
                "SCRIPT ERROR: Parse Error: Invalid type.\n",
                "   at: GDScript::reload (res://player.gd:3)\n",
                "ERROR: Unknown engine failure\n",
            )
            .as_bytes()
            .to_vec(),
        };
        let output = CapturedOutput::from_output(output);
        let report = diagnostics(&output);
        assert_eq!(report.cleanup.len(), 4);
        assert_eq!(
            cleanup_message(&report.cleanup[0].text),
            "ERROR: 12 headless renderer textures still allocated at shutdown."
        );
        assert_eq!(report.issues.len(), 4);
        assert_eq!(
            unresolved_uid(&report.issues[0].text),
            Some("uid://missing")
        );
        assert!(report.issues[2].text.contains("res://player.gd:3"));
        assert_eq!(report.issues[3].text, "ERROR: Unknown engine failure");
        assert!(has_errors(&output.output));
        let cleanup_only = Output {
            status: Default::default(),
            stdout: Vec::new(),
            stderr: report
                .cleanup
                .iter()
                .map(|diagnostic| diagnostic.text.as_str())
                .collect::<Vec<_>>()
                .join("\n")
                .into_bytes(),
        };
        assert!(has_errors(&cleanup_only));
    }

    #[test]
    fn consolidates_repeated_diagnostics_without_reordering_first_occurrences() {
        let output = Output {
            status: Default::default(),
            stdout: b"WARNING: first\nERROR: second\nWARNING: first\n".to_vec(),
            stderr: b"ERROR: second\nWARNING: third\n".to_vec(),
        };
        let output = CapturedOutput::from_output(output);
        let report = diagnostics(&output);
        assert_eq!(report.issues.len(), 3);
        assert_eq!(report.issues[0].text, "WARNING: first");
        assert_eq!(report.issues[0].occurrence_count, 2);
        assert_eq!(report.issues[1].text, "ERROR: second");
        assert_eq!(report.issues[1].occurrence_count, 2);
        assert_eq!(report.issues[2].text, "WARNING: third");
        assert_eq!(report.issues[2].occurrence_count, 1);
    }

    #[test]
    fn structured_diagnostics_keep_sources_streams_and_occurrences() {
        let output = CapturedOutput::from_output(Output {
            status: Default::default(),
            stdout: b"WARNING: first\n".to_vec(),
            stderr: concat!(
                "SCRIPT ERROR: Invalid call.\n",
                "   at: run (res://player.gd:17)\n",
                "SCRIPT ERROR: Invalid call.\n",
                "   at: run (res://player.gd:17)\n",
            )
            .as_bytes()
            .to_vec(),
        });
        let identity = phase("resource_loading", CheckPhase::ResourceLoading);
        let diagnostics = structured_diagnostics(&output, &identity, 10, "check");
        assert_eq!(diagnostics.len(), 2);
        assert_eq!(diagnostics[0].sequence, 10);
        assert_eq!(diagnostics[0].severity, DiagnosticSeverity::Warning);
        assert_eq!(diagnostics[0].stream, DiagnosticStream::Stdout);
        let error = &diagnostics[1];
        assert_eq!(error.sequence, 11);
        assert_eq!(error.stream, DiagnosticStream::Stderr);
        assert_eq!(error.resource.as_deref(), Some("res://player.gd"));
        assert_eq!(error.line, Some(17));
        assert_eq!(error.occurrence_count, 2);
        assert_eq!(error.stack_frames.len(), 1);
        assert_eq!(error.phase, identity);
        assert_eq!(
            error.process.as_ref().unwrap().session_id,
            "check:resource_loading"
        );
    }

    #[test]
    fn preserves_raw_phase_streams_as_check_artifacts() {
        let directory = env::temp_dir().join(format!("gdkit-artifacts-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let artifacts = CheckArtifacts::create(&directory).unwrap();
        let output = Output {
            status: Default::default(),
            stdout: b"raw stdout\r\n".to_vec(),
            stderr: b"raw stderr\n".to_vec(),
        };
        let output = CapturedOutput::from_output(output);
        artifacts
            .preserve(
                "resource-loading",
                &phase("resource_loading", CheckPhase::ResourceLoading),
                &output,
            )
            .unwrap();
        assert_eq!(
            fs::read(artifacts.directory.join("resource-loading.stdout.log")).unwrap(),
            output.output.stdout
        );
        assert_eq!(
            fs::read(artifacts.directory.join("resource-loading.stderr.log")).unwrap(),
            output.output.stderr
        );
        let events =
            fs::read_to_string(artifacts.directory.join("resource-loading.events.jsonl")).unwrap();
        let events: Vec<serde_json::Value> = events
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["sequence"], 0);
        assert_eq!(events[0]["stream"], "stdout");
        assert_eq!(events[0]["text"], "raw stdout");
        assert_eq!(events[1]["sequence"], 1);
        assert_eq!(events[1]["stream"], "stderr");
        assert!(events[1]["observed_at_unix_ms"].as_u64().unwrap() > 0);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn finds_uid_references_with_line_numbers_in_project_and_scripts() {
        let directory = env::temp_dir().join(format!("gdkit-uid-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        fs::write(
            directory.join("project.godot"),
            "[application]\nrun/main_scene=\"uid://abc\"\n",
        )
        .unwrap();
        fs::write(
            directory.join("player.gd"),
            "var a = preload(\"uid://abcdef\")\nvar b = preload(\"uid://abc\")\n",
        )
        .unwrap();
        let references = uid_references(&directory, &["res://player.gd".into()], "uid://abc");
        assert_eq!(references.len(), 2);
        assert!(references[0].starts_with("res://project.godot:2:"));
        assert!(references[1].starts_with("res://player.gd:2:"));
        assert!(uid_references(&directory, &[], "uid://absent").is_empty());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn diagnoses_missing_mismatched_and_stale_script_class_cache_entries() {
        let directory =
            env::temp_dir().join(format!("gdkit-script-classes-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        fs::create_dir(directory.join(".godot")).unwrap();
        fs::write(directory.join("project.godot"), "config_version=5\n").unwrap();
        fs::write(
            directory.join("good.gd"),
            "class_name GoodClass extends RefCounted\n",
        )
        .unwrap();
        fs::write(
            directory.join("missing.gd"),
            "class_name MissingClass extends RefCounted\n",
        )
        .unwrap();
        fs::write(
            directory.join("moved.gd"),
            "class_name MovedClass extends RefCounted\n",
        )
        .unwrap();
        fs::write(
            directory.join(".ignored.gd"),
            "class_name IgnoredClass extends RefCounted\n",
        )
        .unwrap();
        fs::write(
            directory.join(".godot/global_script_class_cache.cfg"),
            concat!(
                "list=[{\n\"class\": &\"GoodClass\",\n\"path\": \"res://good.gd\"\n}, {\n",
                "\"class\": &\"MovedClass\",\n\"path\": \"res://old.gd\"\n}, {\n",
                "\"class\": &\"StaleClass\",\n\"path\": \"res://stale.gd\"\n}, {\n",
                "\"class\": &\"IgnoredClass\",\n\"path\": \"res://.ignored.gd\"\n}]\n",
            ),
        )
        .unwrap();

        let issues = script_class_cache_issues(&directory).unwrap();
        assert_eq!(issues.len(), 3, "{issues:#?}");
        assert!(
            issues
                .iter()
                .any(|issue| issue.message.contains("MissingClass")
                    && issue.message.contains("is missing"))
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.message.contains("MovedClass")
                    && issue.message.contains("res://old.gd"))
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.message.contains("StaleClass")
                    && issue.message.contains("no matching class_name"))
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn distinguishes_missing_and_malformed_script_class_caches() {
        let directory =
            env::temp_dir().join(format!("gdkit-script-cache-state-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("project.godot"), "config_version=5\n").unwrap();
        fs::write(
            directory.join("actor.gd"),
            "class_name CacheActor extends RefCounted\n",
        )
        .unwrap();

        let missing = script_class_cache_issues(&directory).unwrap();
        assert_eq!(missing.len(), 1);
        assert!(missing[0].message.contains("cache is missing"));

        fs::create_dir(directory.join(".godot")).unwrap();
        fs::write(
            directory.join(".godot/global_script_class_cache.cfg"),
            "list=[{\n\"class\": nope,\n\"path\": \"res://actor.gd\"\n}]\n",
        )
        .unwrap();
        let malformed = script_class_cache_issues(&directory).unwrap();
        assert_eq!(malformed.len(), 1);
        assert!(malformed[0].message.contains("cache is unreadable"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn runtime_summary_distinguishes_unrequested_executed_and_skipped_checks() {
        let mut report = CheckReport::new(
            ProjectSnapshot {
                root: PathBuf::from("game"),
                fingerprint: "project".into(),
            },
            CheckPolicy {
                strict_methods: false,
                fresh_import: false,
                ignored_import_diagnostics: 0,
            },
        );
        let empty = ExecutionSummary {
            counts: None,
            smoke_count: 0,
            strict_methods: false,
            script_count: 0,
        };
        assert_eq!(
            runtime_summary(&report, &empty),
            "runtime execution: none requested"
        );

        report.requested_phases = vec![
            phase("project_script:1:first.gd", CheckPhase::ProjectScript),
            phase("project_script:2:second.gd", CheckPhase::ProjectScript),
            phase("scene_smoke:1:main.tscn", CheckPhase::SceneSmoke),
        ];
        report.skipped_phases.push(SkippedPhase {
            phase: report.requested_phases[1].clone(),
            reason: "an earlier phase failed".into(),
        });
        report.skipped_phases.push(SkippedPhase {
            phase: report.requested_phases[2].clone(),
            reason: "an earlier phase failed".into(),
        });
        let partial = ExecutionSummary {
            counts: None,
            smoke_count: 0,
            strict_methods: false,
            script_count: 1,
        };
        assert_eq!(
            runtime_summary(&report, &partial),
            "runtime execution: ran 1/2 project scripts and 0/1 gameplay scene smoke check; 2 skipped (an earlier phase failed)"
        );
    }
}
