//! `check`: workspace → CheckRequest from args (+ config strict_methods, baseline via `check::read_baseline`, warning when it names another project) → `check::validate` → (engine unless --static-only) → gdproject::check::run with a stderr observer (silent in JSON mode) → emit report → Exit from report.outcome.

use std::io::Write;
use std::time::Duration;

use gdproject::check::{CheckObserver, CheckReport, CheckRequest, Outcome, PhaseId, PhaseOutcome};
use gdproject::diagnostics::{Diagnostic, Severity};

use crate::cli::*;
use crate::context::Context;
use crate::render::{self, Exit, Human};

pub fn run(ctx: &Context, args: CheckArgs) -> gdproject::Result<Exit> {
    if args.static_only && !args.script.is_empty() {
        return Err(gdproject::Error::Invalid(
            "--script runs in the engine and cannot be combined with --static-only".into(),
        ));
    }
    let workspace = ctx.workspace(&args.project)?;
    let baseline = match &args.baseline {
        Some(path) => {
            let baseline = gdproject::check::read_baseline(path)?;
            let canonical = |path: &std::path::Path| {
                std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned())
            };
            if canonical(&baseline.project.root) != canonical(workspace.root()) {
                eprintln!(
                    "warning: --baseline {} was recorded for project {}, not {}",
                    path.display(),
                    baseline.project.root.display(),
                    workspace.root().display()
                );
            }
            Some(baseline)
        }
        None => None,
    };
    let scripts = args
        .script
        .iter()
        .map(|script| gdview::ResPath::parse(script))
        .collect::<Result<_, _>>()?;
    let request = CheckRequest {
        slice: args.slice,
        strict_methods: args.strict_methods.then_some(true),
        scripts,
        script_deadline: Duration::from_secs(args.script_timeout),
        phase_deadline: Duration::from_secs(args.phase_timeout),
        static_only: args.static_only,
        baseline,
    };
    // Unusable requests (e.g. a missing --slice) fail before the engine is probed.
    gdproject::check::validate(&workspace, &request)?;
    let engine = if args.static_only {
        None
    } else {
        Some(ctx.engine(&workspace, &args.project)?)
    };
    let report = if ctx.json() {
        gdproject::check::run(
            &workspace,
            engine.as_ref(),
            &request,
            &mut gdproject::check::NoObserver,
        )?
    } else {
        gdproject::check::run(&workspace, engine.as_ref(), &request, &mut StderrProgress)?
    };
    // The observer has no raw-stream callback; replay persisted streams after completion.
    if args.verbose && !ctx.json() {
        let mut stderr = std::io::stderr().lock();
        for phase in &report.phases {
            for path in &phase.artifacts {
                if matches!(
                    path.extension().and_then(|s| s.to_str()),
                    Some("stdout" | "stderr")
                ) {
                    let bytes = std::fs::read(path).map_err(|source| gdproject::Error::Io {
                        path: path.clone(),
                        source,
                    })?;
                    writeln!(stderr, "{} ({}):", phase.id.id, path.display())
                        .and_then(|()| stderr.write_all(&bytes))
                        .map_err(|source| gdproject::Error::Io {
                            path: "<stderr>".into(),
                            source,
                        })?;
                }
            }
        }
    }
    render::emit(ctx.output, &report).map_err(|source| gdproject::Error::Io {
        path: "<stdout>".into(),
        source,
    })?;
    Ok(match report.outcome {
        Outcome::Passed => Exit::Ok,
        Outcome::Failed | Outcome::Incomplete => Exit::Failed,
    })
}

struct StderrProgress;

impl CheckObserver for StderrProgress {
    fn phase_started(&mut self, phase: &PhaseId) {
        eprintln!("{}...", phase.id);
    }
    fn phase_finished(&mut self, phase: &PhaseId, outcome: PhaseOutcome, elapsed: Duration) {
        eprintln!(
            "{}: {} in {} ms",
            phase.id,
            outcome_word(outcome),
            elapsed.as_millis()
        );
    }
}

fn outcome_word(outcome: PhaseOutcome) -> &'static str {
    match outcome {
        PhaseOutcome::Completed => "completed",
        PhaseOutcome::Failed => "failed",
        PhaseOutcome::TimedOut => "timed out",
        PhaseOutcome::Skipped => "skipped",
    }
}

impl Human for CheckReport {
    fn human(&self, out: &mut dyn Write) -> std::io::Result<()> {
        for phase in &self.phases {
            if let Some(reason) = &phase.skipped_reason {
                writeln!(out, "{}: skipped ({reason})", phase.id.id)?;
            }
            for diagnostic in &phase.diagnostics {
                write_diagnostic(out, diagnostic)?;
            }
        }
        for failure in &self.failures {
            let phase = failure
                .phase
                .as_ref()
                .map_or("check", |phase| phase.id.as_str());
            writeln!(out, "{phase}: failure: {}", failure.message)?;
        }
        if let Some(path) = &self.artifact_dir {
            writeln!(out, "artifacts: {}", path.display())?;
        }
        if let Some(baseline) = &self.baseline {
            writeln!(
                out,
                "baseline: {} new, {} carried, {} resolved",
                baseline.new.len(),
                baseline.carried.len(),
                baseline.resolved.len()
            )?;
            for diagnostic in &baseline.new {
                write!(out, "new: ")?;
                write_diagnostic(out, diagnostic)?;
            }
        }
        let verdict = match self.outcome {
            Outcome::Passed => "passed",
            Outcome::Failed => "FAILED",
            Outcome::Incomplete => "INCOMPLETE",
        };
        // Every failure counts (engine diagnostics, timeouts, ...), not only static findings.
        let counts = self.counts.as_ref().map_or_else(String::new, |c| {
            format!(
                " ({} static finding(s)) across {} script(s), {} scene(s), {} resource(s)",
                c.static_findings, c.scripts, c.scenes, c.resources
            )
        });
        writeln!(
            out,
            "check {verdict}: {} failure(s){counts}",
            self.failures.len()
        )
    }
}

fn write_diagnostic(out: &mut dyn Write, diagnostic: &Diagnostic) -> std::io::Result<()> {
    let severity = match diagnostic.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    };
    let location = match (&diagnostic.resource, diagnostic.line) {
        (Some(resource), Some(line)) => format!("{resource}:{line}: "),
        (Some(resource), None) => format!("{resource}: "),
        _ => String::new(),
    };
    let code = diagnostic
        .code
        .as_deref()
        .map_or_else(String::new, |code| format!(" [{code}]"));
    writeln!(out, "{location}{severity}: {}{code}", diagnostic.message)?;
    if !diagnostic.suggestions.is_empty() {
        writeln!(out, "  did you mean: {}", diagnostic.suggestions.join(", "))?;
    }
    Ok(())
}
