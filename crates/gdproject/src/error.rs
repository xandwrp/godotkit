use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    View(#[from] gdview::Error),
    #[error("{path}: {message}")]
    Config { path: PathBuf, message: String },
    #[error(
        "no engine configured; run `gdkit config set godot <path>` for a default, or pass --godot <path>, set GDKIT_GODOT, or add `[engine]` with `executable = \"<path>\"` to gdkit.toml"
    )]
    NoEngine,
    #[error("Godot executable not found: {0}")]
    EngineNotFound(PathBuf),
    #[error("Godot executable `{0}` not found on PATH")]
    EngineNotOnPath(PathBuf),
    #[error("engine compatibility probe failed: {message}")]
    Probe { message: String, output: String },
    #[error("could not run engine: {0}")]
    Spawn(#[source] std::io::Error),
    /// A raw engine run (no harness) that exited unsuccessfully or wrote nothing usable.
    #[error("{what} failed: {message}")]
    EngineRun { what: String, message: String },
    #[error("{what} exceeded its deadline of {deadline:?}")]
    Timeout { what: String, deadline: Duration },
    #[error("harness {harness}: {source}")]
    Protocol {
        harness: &'static str,
        #[source]
        source: protocol::ProtocolError,
    },
    #[error("harness {harness} failed at {stage}: {message}")]
    Harness {
        harness: &'static str,
        stage: String,
        message: String,
    },
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path}: {message}")]
    Record { path: PathBuf, message: String },
    #[error("{0}")]
    Invalid(String),
    #[error("another gdkit operation holds the project lock ({0})")]
    Locked(PathBuf),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

use crate::protocol;
