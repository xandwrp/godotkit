use std::path::PathBuf;

/// Every failure this crate can produce. Variants are coarse on purpose: callers
/// branch on "is this a bad project" vs "is this a bad file", not on parser detail.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{path} is not inside a Godot project (no project.godot found)")]
    NotAProject { path: PathBuf },
    #[error("{path}: project.godot exists but is not a regular file")]
    ConfigNotAFile { path: PathBuf },
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid res:// path: {0}")]
    InvalidResPath(String),
    #[error("{}: line {line}: {message}", path.as_deref().map(|p| p.display().to_string()).unwrap_or_else(|| "<input>".into()))]
    Parse {
        path: Option<PathBuf>,
        line: usize,
        message: String,
    },
    #[error("scene {}: {message}", path.as_deref().map(|p| p.display().to_string()).unwrap_or_else(|| "<input>".into()))]
    Scene {
        path: Option<PathBuf>,
        message: String,
    },
    #[error("scene expansion: {0}")]
    Expansion(String),
    #[error("variant json: {0}")]
    Variant(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Attaches a file path to a `Parse` or `Scene` error that was produced from a string.
    pub fn with_path(self, path: PathBuf) -> Self {
        match self {
            Error::Parse {
                path: None,
                line,
                message,
            } => Error::Parse {
                path: Some(path),
                line,
                message,
            },
            Error::Scene {
                path: None,
                message,
            } => Error::Scene {
                path: Some(path),
                message,
            },
            other => other,
        }
    }
}
