//! An engine executable that has passed the compatibility probe.
//!
//! The probe result is cached per project under the engine's identity
//! (path, size, mtime, companion binary on Windows, and a hash of the harness
//! sources) so a rebuilt engine or a changed harness re-probes automatically.
//!
//! # Tests (tests/engine.rs, offline with `fake-godot`)
//! - `attach_probes_once_then_hits_cache`
//! - `cache_misses_when_engine_size_or_mtime_or_harness_hash_changes`
//! - `cache_is_not_written_when_probe_fails_or_engine_changes_mid_probe`
//! - `probe_rejects_engines_missing_headless_editor_flags`
//! - `probe_rejects_non_editor_or_non_4x_builds`
//! - `probe_respects_deadline` (fake engine that hangs)
//! - `console_launcher_tracks_companion_exe_on_windows`
//! - `fingerprint_is_stable_for_identical_key_and_differs_otherwise`

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::config::EngineSelection;
use crate::workspace::Workspace;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Engine {
    pub executable: PathBuf,
    pub version: String,
    /// blake3 of [`ProbeKey`]; identifies engine + harness generation.
    pub fingerprint: String,
    pub source: crate::config::SelectionSource,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeKey {
    pub files: Vec<TrackedFile>,
    pub harness_hash: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackedFile {
    pub path: PathBuf,
    pub size: u64,
    pub modified_unix_ns: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeCacheHealth {
    Missing,
    Current,
    Stale,
    Malformed,
    Unreadable,
}

pub const DEFAULT_PROBE_DEADLINE: Duration = Duration::from_secs(60);

impl Engine {
    /// Uses the cached probe when the key matches, otherwise runs [`probe`] and caches.
    /// Returns whether the cache was hit so `doctor` can say so.
    pub fn attach(
        selection: &EngineSelection,
        workspace: &Workspace,
        deadline: Duration,
    ) -> crate::Result<(Engine, bool)> {
        let key = key_for(&selection.executable)?;
        if let Ok(record) = read_cache(workspace) {
            if record.key == key && record.report.compatible() {
                return Ok((engine(selection, &key, record.report.version)?, true));
            }
        }
        let report = probe(&selection.executable, deadline)?;
        ensure_unchanged(&selection.executable, &key)?;
        let result = engine(selection, &key, report.version.clone())?;
        let _lock = workspace.lock()?;
        ensure_unchanged(&selection.executable, &key)?;
        let record = ProbeCache { key, report };
        let bytes = serde_json::to_vec(&record)?;
        let path = workspace.probe_cache_path();
        // The workspace lock serializes writers; rename keeps readers from seeing
        // a partially written record. A failed publication never leaves a temp file.
        let temporary = path.with_extension(format!(
            "json.{}.{}.tmp",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|error| crate::Error::Invalid(error.to_string()))?
                .as_nanos(),
        ));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|source| crate::Error::Io {
                path: temporary.clone(),
                source,
            })?;
        let write = (|| {
            use std::io::Write;
            file.write_all(&bytes)?;
            drop(file);
            #[cfg(windows)]
            if path.exists() {
                fs::remove_file(&path)?;
            }
            fs::rename(&temporary, &path)
        })();
        let _ = fs::remove_file(&temporary);
        write.map_err(|source| crate::Error::Io { path, source })?;
        Ok((result, false))
    }

    /// Like `attach` but with no project: probes into a temp dir, no cache.
    pub fn attach_standalone(
        selection: &EngineSelection,
        deadline: Duration,
    ) -> crate::Result<Engine> {
        let key = key_for(&selection.executable)?;
        let report = probe(&selection.executable, deadline)?;
        ensure_unchanged(&selection.executable, &key)?;
        engine(selection, &key, report.version)
    }
}

#[derive(Serialize, Deserialize)]
struct ProbeCache {
    key: ProbeKey,
    report: ProbeReport,
}

fn read_cache(workspace: &Workspace) -> Result<ProbeCache, ProbeCacheHealth> {
    let bytes = fs::read(workspace.probe_cache_path()).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ProbeCacheHealth::Missing
        } else {
            ProbeCacheHealth::Unreadable
        }
    })?;
    serde_json::from_slice(&bytes).map_err(|_| ProbeCacheHealth::Malformed)
}

fn key_for(executable: &Path) -> crate::Result<ProbeKey> {
    probe_key(executable).map_err(|source| crate::Error::Io {
        path: executable.to_owned(),
        source,
    })
}

fn ensure_unchanged(executable: &Path, key: &ProbeKey) -> crate::Result<()> {
    if probe_key(executable).as_ref().ok() != Some(key) {
        return Err(crate::Error::Probe {
            message: "engine changed during compatibility probe; retry with a stable executable"
                .into(),
            output: String::new(),
        });
    }
    Ok(())
}

fn engine(selection: &EngineSelection, key: &ProbeKey, version: String) -> crate::Result<Engine> {
    Ok(Engine {
        executable: selection.executable.clone(),
        version,
        fingerprint: blake3::hash(&serde_json::to_vec(key)?).to_hex().to_string(),
        source: selection.source,
    })
}

pub fn probe_key(executable: &Path) -> std::io::Result<ProbeKey> {
    fn track(path: &Path) -> std::io::Result<TrackedFile> {
        let path = fs::canonicalize(path)?;
        let metadata = fs::metadata(&path)?;
        if !metadata.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "engine is not a file",
            ));
        }
        let modified_unix_ns = metadata
            .modified()?
            .duration_since(UNIX_EPOCH)
            .map_err(std::io::Error::other)?
            .as_nanos();
        Ok(TrackedFile {
            path,
            size: metadata.len(),
            modified_unix_ns,
        })
    }
    let files = vec![track(executable)?];
    #[cfg(windows)]
    let files = {
        let mut files = files;
        let path = &files[0].path;
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            if let Some(base) = stem
                .strip_suffix(".console")
                .or_else(|| stem.strip_suffix("_console"))
            {
                files.push(track(&path.with_file_name(format!("{base}.exe")))?);
            }
        }
        files
    };
    Ok(ProbeKey {
        files,
        harness_hash: crate::runner::harness_hash(),
    })
}

pub fn probe_cache_health(executable: &Path, workspace: &Workspace) -> ProbeCacheHealth {
    let record = match read_cache(workspace) {
        Ok(record) => record,
        Err(health) => return health,
    };
    if !record.report.compatible() {
        return ProbeCacheHealth::Malformed;
    }
    match probe_key(executable) {
        Ok(key) if key == record.key => ProbeCacheHealth::Current,
        _ => ProbeCacheHealth::Stale,
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProbeReport {
    pub version: String,
    pub editor: bool,
    pub major: u32,
}

/// Runs `--help` and the probe harness. Requires headless editor flags,
/// Godot 4, editor feature, and working resource loading.
pub fn probe(executable: &Path, deadline: Duration) -> crate::Result<ProbeReport> {
    use crate::runner::{Harness, Invocation};
    let started = Instant::now();
    let mut output = String::new();
    let failure = |message: String, output: &str| crate::Error::Probe {
        message,
        output: output.to_owned(),
    };
    let remaining = |output: &str| {
        deadline
            .checked_sub(started.elapsed())
            .filter(|d| !d.is_zero())
            .ok_or_else(|| {
                failure(
                    format!("probe exceeded its deadline of {deadline:?}"),
                    output,
                )
            })
    };
    let help = crate::process::run(
        &crate::process::Spawn::new(executable).arg("--help"),
        remaining(&output)?,
    )
    .map_err(crate::Error::Spawn)?;
    output.extend(help.lines_text().map(|line| line.into_owned()));
    if help.timed_out {
        return Err(failure(
            format!("probe help exceeded its deadline of {deadline:?}"),
            &output,
        ));
    }
    if !help.success() {
        return Err(failure(
            format!("--help exited unsuccessfully: {:?}", help.status),
            &output,
        ));
    }
    // Godot emits ANSI-colored help even when stdout is a pipe.
    let mut help_text = String::new();
    let mut chars = output.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if ('\u{40}'..='\u{7e}').contains(&c) {
                    break;
                }
            }
        } else {
            help_text.push(c);
        }
    }
    for flag in [
        "--headless",
        "--no-header",
        "--editor",
        "--path",
        "--script",
        "--import",
        "--quit",
    ] {
        if !help_text
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
            .any(|word| word == flag)
        {
            return Err(failure(
                format!("engine help is missing required flag {flag}"),
                &output,
            ));
        }
    }
    let scratch = crate::workspace::IsolatedCopy::empty()?;
    let resource = scratch.path.join("probe.tres");
    fs::write(
        &resource,
        b"[gd_resource type=\"Resource\" format=3]\n\n[resource]\n",
    )
    .map_err(|source| crate::Error::Io {
        path: resource,
        source,
    })?;
    let candidate = Engine {
        executable: executable.to_owned(),
        version: String::new(),
        fingerprint: String::new(),
        source: crate::config::SelectionSource::CommandLine,
    };
    let invocation = Invocation::new(&candidate, &scratch.path, remaining(&output)?);
    let run = crate::runner::run_harness_captured::<ProbeReport>(&invocation, Harness::Probe)?;
    output.extend(run.captured.lines_text().map(|line| line.into_owned()));
    let envelope = run
        .envelope
        .map_err(|error| failure(error.to_string(), &output))?;
    remaining(&output)?;
    let report = envelope
        .payload
        .ok_or_else(|| failure("probe returned no payload".into(), &output))?;
    if !report.compatible() {
        return Err(failure(
            format!(
                "requires a Godot 4 editor build; got version {:?}, major {}, editor {}",
                report.version, report.major, report.editor
            ),
            &output,
        ));
    }
    Ok(report)
}

impl ProbeReport {
    fn compatible(&self) -> bool {
        self.major == 4 && self.editor && !self.version.trim().is_empty()
    }
}
