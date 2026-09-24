use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const CHECK_REPORT_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckReport {
    pub schema_version: u32,
    pub outcome: CheckOutcome,
    pub engine: Option<EngineFingerprint>,
    pub project: ProjectSnapshot,
    pub policy: CheckPolicy,
    pub requested_phases: Vec<PhaseIdentity>,
    pub completed_phases: Vec<PhaseIdentity>,
    pub skipped_phases: Vec<SkippedPhase>,
    pub checked: Option<CheckCounts>,
    pub diagnostics: Vec<Diagnostic>,
    pub failures: Vec<CheckFailure>,
    pub suppressed_diagnostics: usize,
    pub artifacts: Vec<Artifact>,
}

impl CheckReport {
    pub fn new(project: ProjectSnapshot, policy: CheckPolicy) -> Self {
        Self {
            schema_version: CHECK_REPORT_SCHEMA_VERSION,
            outcome: CheckOutcome::Incomplete,
            engine: None,
            project,
            policy,
            requested_phases: Vec::new(),
            completed_phases: Vec::new(),
            skipped_phases: Vec::new(),
            checked: None,
            diagnostics: Vec::new(),
            failures: Vec::new(),
            suppressed_diagnostics: 0,
            artifacts: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckCounts {
    pub scripts: usize,
    pub scenes: usize,
    pub resources: usize,
    pub smoke_scenes: usize,
    pub project_scripts: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckOutcome {
    Passed,
    ValidationFailed,
    ToolFailed,
    Incomplete,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineFingerprint {
    pub executable: PathBuf,
    pub version: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectSnapshot {
    pub root: PathBuf,
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckPolicy {
    pub strict_methods: bool,
    pub fresh_import: bool,
    pub ignored_import_diagnostics: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhaseIdentity {
    pub id: String,
    pub kind: CheckPhase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckPhase {
    FileScan,
    EngineValidation,
    Import,
    ResourceLoading,
    SceneSmoke,
    ProjectScript,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkippedPhase {
    pub phase: PhaseIdentity,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub sequence: u64,
    pub phase: PhaseIdentity,
    pub severity: DiagnosticSeverity,
    pub stream: DiagnosticStream,
    pub engine_code: Option<String>,
    pub message: String,
    pub resource: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub stack_frames: Vec<StackFrame>,
    pub process: Option<ProcessIdentity>,
    pub timestamp_unix_ms: Option<u64>,
    pub occurrence_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticStream {
    Logger,
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StackFrame {
    pub function: Option<String>,
    pub resource: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessIdentity {
    pub session_id: String,
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckFailure {
    pub kind: FailureKind,
    pub phase: Option<PhaseIdentity>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    Diagnostic,
    ProcessExit,
    Timeout,
    MissingCompletion,
    ResourceLoad,
    Tool,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    pub kind: ArtifactKind,
    pub phase: Option<PhaseIdentity>,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Stdout,
    Stderr,
    EventStream,
    Report,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phase(id: &str, kind: CheckPhase) -> PhaseIdentity {
        PhaseIdentity {
            id: id.into(),
            kind,
        }
    }

    #[test]
    fn new_reports_are_explicitly_incomplete() {
        let report = CheckReport::new(
            ProjectSnapshot {
                root: PathBuf::from("game"),
                fingerprint: "project-1".into(),
            },
            CheckPolicy {
                strict_methods: true,
                fresh_import: false,
                ignored_import_diagnostics: 2,
            },
        );
        assert_eq!(report.schema_version, CHECK_REPORT_SCHEMA_VERSION);
        assert_eq!(report.outcome, CheckOutcome::Incomplete);
        assert!(report.completed_phases.is_empty());
        assert!(report.failures.is_empty());
    }

    #[test]
    fn report_json_round_trips_the_full_contract() {
        let import = phase("import", CheckPhase::Import);
        let smoke = phase("scene_smoke:res://main.tscn", CheckPhase::SceneSmoke);
        let mut report = CheckReport::new(
            ProjectSnapshot {
                root: PathBuf::from("game"),
                fingerprint: "project-1".into(),
            },
            CheckPolicy {
                strict_methods: true,
                fresh_import: false,
                ignored_import_diagnostics: 1,
            },
        );
        report.outcome = CheckOutcome::ValidationFailed;
        report.engine = Some(EngineFingerprint {
            executable: PathBuf::from("godot"),
            version: "4.7.test".into(),
            fingerprint: "engine-1".into(),
        });
        report.requested_phases = vec![import.clone(), smoke.clone()];
        report.completed_phases.push(import.clone());
        report.skipped_phases.push(SkippedPhase {
            phase: smoke,
            reason: "resource validation failed".into(),
        });
        report.checked = Some(CheckCounts {
            scripts: 1,
            scenes: 1,
            resources: 1,
            smoke_scenes: 0,
            project_scripts: 0,
        });
        report.diagnostics.push(Diagnostic {
            sequence: 0,
            phase: import.clone(),
            severity: DiagnosticSeverity::Error,
            stream: DiagnosticStream::Stderr,
            engine_code: Some("SCRIPT_ERROR".into()),
            message: "Invalid call".into(),
            resource: Some("res://main.gd".into()),
            line: Some(4),
            column: None,
            stack_frames: vec![StackFrame {
                function: Some("_ready".into()),
                resource: Some("res://main.gd".into()),
                line: Some(4),
                column: None,
            }],
            process: Some(ProcessIdentity {
                session_id: "check-1".into(),
                pid: Some(42),
            }),
            timestamp_unix_ms: Some(1_700_000_000_000),
            occurrence_count: 2,
        });
        report.failures.push(CheckFailure {
            kind: FailureKind::Diagnostic,
            phase: Some(import.clone()),
            message: "one or more errors were reported".into(),
        });
        report.suppressed_diagnostics = 1;
        report.artifacts.push(Artifact {
            kind: ArtifactKind::Stderr,
            phase: Some(import),
            path: PathBuf::from("artifacts/import.stderr.log"),
        });

        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"schema_version\":2"));
        assert!(json.contains("\"outcome\":\"validation_failed\""));
        assert!(json.contains("\"kind\":\"scene_smoke\""));
        assert!(json.contains("\"stream\":\"stderr\""));
        assert_eq!(serde_json::from_str::<CheckReport>(&json).unwrap(), report);
    }
}
