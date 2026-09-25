//! One engine invocation. Every operation module builds an [`Invocation`] and
//! calls a capture API, [`run_harness`], [`run_engine`], or [`spawn_game`].
//! ScriptBootstrap uses raw capture: startup is not a completion envelope.
//!
//! # Tests (tests/runner.rs, offline with `fake-godot`)
//! - `run_harness_writes_harness_and_protocol_to_a_temp_dir_and_passes_user_args_after_double_dash`
//! - `run_harness_adds_editor_flag_only_when_requested`
//! - `run_harness_decodes_envelope_and_attaches_diagnostics_and_captured_output`
//! - `run_harness_maps_error_envelope_to_error_harness_with_stage`
//! - `run_harness_maps_missing_envelope_to_error_protocol`
//! - `run_harness_enforces_deadline_and_reports_timeout_with_partial_output`
//! - `run_engine_is_the_raw_form_used_for_import_dump_and_run`
//! - `temp_files_are_removed_after_every_outcome`
//! - `harness_scratch_is_removed_when_a_panic_unwinds_while_it_is_live`

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use serde::de::DeserializeOwned;

use crate::diagnostics::Diagnostic;
use crate::engine::Engine;
use crate::process::Captured;
use crate::protocol::Envelope;

/// Embedded SceneTree harnesses. `protocol.gd` is always written alongside them.
/// ScriptBootstrap emits a startup marker, not a success envelope; RuntimeProbe
/// serves the live game protocol instead of a completion envelope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Harness {
    Probe,
    Check,
    ImportScan,
    ResourceSchema,
    ResourceCreate,
    RuntimeProbe,
    ScriptBootstrap,
}

impl Harness {
    /// Every harness, in hash order. Completeness is checked at compile time below.
    pub const ALL: [Harness; 7] = [
        Harness::Probe,
        Harness::Check,
        Harness::ImportScan,
        Harness::ResourceSchema,
        Harness::ResourceCreate,
        Harness::RuntimeProbe,
        Harness::ScriptBootstrap,
    ];

    const fn is_listed(self) -> bool {
        let mut index = 0;
        while index < Self::ALL.len() {
            if Self::ALL[index] as u8 == self as u8 {
                return true;
            }
            index += 1;
        }
        false
    }

    pub fn name(self) -> &'static str {
        match self {
            Harness::Probe => "probe",
            Harness::Check => "check",
            Harness::ImportScan => "import_scan",
            Harness::ResourceSchema => "resource_schema",
            Harness::ResourceCreate => "resource_create",
            Harness::RuntimeProbe => "runtime_probe",
            Harness::ScriptBootstrap => "script_bootstrap",
        }
    }
    pub fn source(self) -> &'static str {
        match self {
            Harness::Probe => include_str!("harness/probe.gd"),
            Harness::Check => include_str!("harness/check.gd"),
            Harness::ImportScan => include_str!("harness/import_scan.gd"),
            Harness::ResourceSchema => include_str!("harness/resource_schema.gd"),
            Harness::ResourceCreate => include_str!("harness/resource_create.gd"),
            Harness::RuntimeProbe => include_str!("harness/runtime_probe.gd"),
            Harness::ScriptBootstrap => include_str!("harness/script_bootstrap.gd"),
        }
    }
    /// Needs `--editor` (EditorInterface, importers).
    pub fn needs_editor(self) -> bool {
        matches!(self, Harness::ImportScan)
    }
}

// The exhaustive match needs an arm per variant, and each arm's inline const
// fails the build unless `Harness::ALL` lists that variant.
const _: () = match Harness::Probe {
    Harness::Probe => const { assert!(Harness::Probe.is_listed()) },
    Harness::Check => const { assert!(Harness::Check.is_listed()) },
    Harness::ImportScan => const { assert!(Harness::ImportScan.is_listed()) },
    Harness::ResourceSchema => const { assert!(Harness::ResourceSchema.is_listed()) },
    Harness::ResourceCreate => const { assert!(Harness::ResourceCreate.is_listed()) },
    Harness::RuntimeProbe => const { assert!(Harness::RuntimeProbe.is_listed()) },
    Harness::ScriptBootstrap => const { assert!(Harness::ScriptBootstrap.is_listed()) },
};

pub const PROTOCOL_SOURCE: &str = include_str!("harness/protocol.gd");

/// Hash of every embedded harness; part of the engine probe key.
pub fn harness_hash() -> u64 {
    let mut hash = blake3::Hasher::new();
    hash.update(PROTOCOL_SOURCE.as_bytes());
    for harness in Harness::ALL {
        hash.update(harness.name().as_bytes());
        hash.update(&(harness.source().len() as u64).to_le_bytes());
        hash.update(harness.source().as_bytes());
    }
    let mut bytes = [0; 8];
    bytes.copy_from_slice(&hash.finalize().as_bytes()[..8]);
    u64::from_le_bytes(bytes)
}

pub struct Invocation<'a> {
    pub engine: &'a Engine,
    /// Directory passed to `--path`. A scratch copy for `check`, the real project otherwise.
    pub project_dir: &'a Path,
    pub deadline: Duration,
    /// Extra engine flags before `--script` (e.g. `--quiet`, `--import`, `--dump-extension-api-with-docs`).
    pub engine_args: Vec<OsString>,
    /// Arguments after `--`, visible via `OS.get_cmdline_user_args()`.
    pub user_args: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
}

impl<'a> Invocation<'a> {
    pub fn new(engine: &'a Engine, project_dir: &'a Path, deadline: Duration) -> Self {
        Self {
            engine,
            project_dir,
            deadline,
            engine_args: Vec::new(),
            user_args: Vec::new(),
            env: Vec::new(),
        }
    }
}

pub struct HarnessRun<T> {
    pub envelope: Envelope<T>,
    pub captured: Captured,
    pub diagnostics: Vec<Diagnostic>,
}

/// `<engine> --headless --no-header [--editor] --path <dir> --script <tmp>/<harness>.gd -- <user_args…>`
/// Decodes the envelope; an `ok: false` envelope becomes `Error::Harness`.
pub fn run_harness<T: DeserializeOwned>(
    invocation: &Invocation<'_>,
    harness: Harness,
) -> crate::Result<HarnessRun<T>> {
    let run = run_harness_captured(invocation, harness)?;
    Ok(HarnessRun {
        envelope: run.envelope?,
        captured: run.captured,
        diagnostics: run.diagnostics,
    })
}

/// Capture survives output limits, timeout, unsuccessful exit, and every envelope error.
/// Outer errors mean scratch setup or process I/O failed and no complete capture
/// was returned by the process layer. Diagnostics never determine this result.
#[derive(Debug)]
pub struct CapturedHarnessRun<T> {
    pub captured: Captured,
    pub diagnostics: Vec<Diagnostic>,
    pub envelope: crate::Result<Envelope<T>>,
}

/// Typed completion-harness path. Output limits and timeout are checked before
/// decoding; a successful envelope still requires normal exit zero. Use the raw API for
/// ScriptBootstrap and interpret its exact stdout marker and exit independently.
pub fn run_harness_captured<T: DeserializeOwned>(
    invocation: &Invocation<'_>,
    harness: Harness,
) -> crate::Result<CapturedHarnessRun<T>> {
    let (captured, diagnostics) = run_harness_raw(invocation, harness)?;
    let envelope = decode_completion(invocation, harness, &captured);
    Ok(CapturedHarnessRun {
        captured,
        diagnostics,
        envelope,
    })
}

/// Runs an embedded harness without interpreting protocol, exit, or timeout.
/// In particular, ScriptBootstrap success requires exactly one stdout line
/// `GDKIT_SCRIPT_STARTED`, normal exit zero, no timeout, no error envelope and no
/// engine errors on either stream. Output-limit truncation also prevents success.
/// The caller owns that specialized verdict.
pub fn run_harness_raw(
    invocation: &Invocation<'_>,
    harness: Harness,
) -> crate::Result<(Captured, Vec<Diagnostic>)> {
    let files = harness_files(harness)?;
    let mut spawn = engine_spawn(invocation, true);
    if harness.needs_editor() && !spawn.args.iter().any(|arg| arg == "--editor") {
        spawn.args.push("--editor".into());
    }
    spawn.args.extend([
        OsString::from("--script"),
        files
            .path()
            .join(format!("{}.gd", harness.name()))
            .into_os_string(),
    ]);
    append_user_args(&mut spawn, invocation);
    capture(&spawn, invocation)
}

fn decode_completion<T: DeserializeOwned>(
    invocation: &Invocation<'_>,
    harness: Harness,
    captured: &Captured,
) -> crate::Result<Envelope<T>> {
    if captured.output_limit_exceeded {
        return Err(crate::Error::Invalid(format!(
            "harness {} exceeded the output capture limit; output is incomplete",
            harness.name(),
        )));
    }
    if captured.timed_out {
        return Err(crate::Error::Timeout {
            what: harness.name().into(),
            deadline: invocation.deadline,
        });
    }
    let protocol_error = |source| crate::Error::Protocol {
        harness: harness.name(),
        source,
    };
    if matches!(harness, Harness::ScriptBootstrap | Harness::RuntimeProbe) {
        return Err(protocol_error(crate::protocol::ProtocolError::Malformed(
            "this harness requires raw capture, not a completion envelope".into(),
        )));
    }
    let stdout = captured.stdout();
    let envelope = crate::protocol::parse_envelope::<T>(
        String::from_utf8_lossy(&stdout).lines().map(str::to_owned),
    )
    .map_err(protocol_error)?;
    if envelope.harness != harness.name() {
        return Err(protocol_error(crate::protocol::ProtocolError::Malformed(
            format!(
                "expected harness {}, received {}",
                harness.name(),
                envelope.harness,
            ),
        )));
    }
    if !envelope.ok || envelope.error.is_some() {
        let error = envelope.error.unwrap_or(crate::protocol::HarnessError {
            stage: "unknown".into(),
            message: "harness reported failure without error details".into(),
            field: None,
        });
        return Err(crate::Error::Harness {
            harness: harness.name(),
            stage: error.stage,
            message: error.message,
        });
    }
    if !captured.success() {
        return Err(crate::Error::Harness {
            harness: harness.name(),
            stage: "exit".into(),
            message: format!("engine did not exit successfully: {:?}", captured.status),
        });
    }
    Ok(envelope)
}

fn harness_files(harness: Harness) -> crate::Result<crate::workspace::IsolatedCopy> {
    let files = crate::workspace::IsolatedCopy::empty()?;
    for (name, source) in [
        (format!("{}.gd", harness.name()), harness.source()),
        ("protocol.gd".into(), PROTOCOL_SOURCE),
    ] {
        let path = files.path().join(name);
        std::fs::write(&path, source).map_err(|source| crate::Error::Io { path, source })?;
    }
    Ok(files)
}

fn engine_spawn(invocation: &Invocation<'_>, headless: bool) -> crate::process::Spawn {
    let mut spawn = crate::process::Spawn::new(&invocation.engine.executable);
    if headless && !invocation.engine_args.iter().any(|arg| arg == "--headless") {
        spawn.args.push("--headless".into());
    }
    spawn.args.extend([
        OsString::from("--no-header"),
        OsString::from("--path"),
        invocation.project_dir.as_os_str().to_owned(),
    ]);
    spawn.args.extend(invocation.engine_args.iter().cloned());
    spawn.env.clone_from(&invocation.env);
    spawn
}

fn append_user_args(spawn: &mut crate::process::Spawn, invocation: &Invocation<'_>) {
    spawn.args.push("--".into());
    spawn.args.extend(invocation.user_args.iter().cloned());
}

/// Paths under the invocation's project directory (the isolated copy during
/// `check`) are rewritten to `res://` so diagnostic identities survive runs.
fn capture(
    spawn: &crate::process::Spawn,
    invocation: &Invocation<'_>,
) -> crate::Result<(Captured, Vec<Diagnostic>)> {
    let captured = crate::process::run(spawn, invocation.deadline).map_err(crate::Error::Spawn)?;
    let diagnostics = crate::diagnostics::parse_rooted(&captured, 0, Some(invocation.project_dir));
    Ok((captured, diagnostics))
}

/// Raw engine run with no harness: `--editor --import`, the extension-api dump, etc.
/// The caller interprets exit status and diagnostics.
pub fn run_engine(invocation: &Invocation<'_>) -> crate::Result<(Captured, Vec<Diagnostic>)> {
    let mut spawn = engine_spawn(invocation, true);
    append_user_args(&mut spawn, invocation);
    capture(&spawn, invocation)
}

/// Spawns the engine as the game for [`crate::run`]: `--path <project> [--headless] --script runtime_probe.gd [scene] -- <args>`.
/// Headless mode is opt-in via `engine_args`; the caller enforces the deadline.
/// The guard retains the embedded files until drop, after process cleanup.
pub fn spawn_game(
    invocation: &Invocation<'_>,
    log: &Path,
) -> crate::Result<crate::process::ChildGuard> {
    let files = harness_files(Harness::RuntimeProbe)?;
    let mut spawn = engine_spawn(invocation, false);
    spawn.args.extend([
        OsString::from("--script"),
        files.path().join("runtime_probe.gd").into_os_string(),
    ]);
    append_user_args(&mut spawn, invocation);
    let mut guard = crate::process::spawn(&spawn, log).map_err(crate::Error::Spawn)?;
    guard.retain(files);
    Ok(guard)
}
