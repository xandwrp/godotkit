//! `check`: workspace → (engine unless --static-only) → CheckRequest from args (+ config strict_methods, baseline file parsed as a CheckReport) → gdproject::check::run with a stderr observer (silent in JSON mode) → emit report → Exit from report.outcome.
//!
//! Only `--static-only` is implemented; the engine phases land next.

use std::io::Write;
use std::time::Duration;

use gdproject::check::{CheckObserver, CheckReport, CheckRequest, Outcome, PhaseId, PhaseOutcome};
use gdproject::diagnostics::{Diagnostic, Severity};

use crate::cli::*;
use crate::context::Context;
use crate::render::{self, Exit, Human};

pub fn run(ctx: &Context, args: CheckArgs) -> gdproject::Result<Exit> {
    if !args.static_only {
        return Err(gdproject::Error::Invalid(
            "`check` without --static-only needs the engine phases, which are not implemented in this build; run `gdkit check --static-only`".into(),
        ));
    }
    if !args.script.is_empty() {
        return Err(gdproject::Error::Invalid("--script runs in the engine and cannot be combined with --static-only".into()));
    }
    let workspace = ctx.workspace(&args.project)?;
    let baseline = match &args.baseline {
        Some(path) => {
            let text = std::fs::read_to_string(path).map_err(|source| gdproject::Error::Io { path: path.clone(), source })?;
            Some(serde_json::from_str::<CheckReport>(&text)?)
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
    let report = if ctx.json() {
        gdproject::check::run(&workspace, None, &request, &mut gdproject::check::NoObserver)?
    } else {
        gdproject::check::run(&workspace, None, &request, &mut StderrProgress)?
    };
    render::emit(ctx.output, &report).map_err(|source| gdproject::Error::Io { path: "<stdout>".into(), source })?;
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
        eprintln!("{}: {} in {} ms", phase.id, outcome_word(outcome), elapsed.as_millis());
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
        let counts = self.counts.as_ref().map_or_else(String::new, |c| {
            format!(
                ": {} finding(s) across {} script(s), {} scene(s), {} resource(s)",
                c.static_findings, c.scripts, c.scenes, c.resources
            )
        });
        writeln!(out, "check {verdict}{counts}")
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
    let code = diagnostic.code.as_deref().map_or_else(String::new, |code| format!(" [{code}]"));
    writeln!(out, "{location}{severity}: {}{code}", diagnostic.message)?;
    if !diagnostic.suggestions.is_empty() {
        writeln!(out, "  did you mean: {}", diagnostic.suggestions.join(", "))?;
    }
    Ok(())
}
