//! Human rendering of an `api` report: pre-rendered signatures and prose from
//! `gdview::api::answer`, laid out for a terminal.

use std::io::Write;

use gdview::api::answer::{Answer, MemberLine};

use super::Report;
use crate::render::Human;

impl Human for Report {
    fn human(&self, out: &mut dyn Write) -> std::io::Result<()> {
        match &self.answer {
            Answer::Class(class) => {
                let origin = match &class.script {
                    Some(script) => format!("script {}", location(&script.path, script.line)),
                    None => class.api_type.clone(),
                };
                writeln!(out, "{} ({origin})", class.name)?;
                if !class.inherits.is_empty() {
                    writeln!(out, "inherits {}", class.inherits.join(" < "))?;
                }
                if let Some(singleton) = &class.singleton {
                    writeln!(out, "singleton: {singleton}")?;
                }
                lifecycle(out, &class.deprecated, &class.experimental)?;
                prose(out, class.brief.as_ref())?;
                prose(out, class.description.as_ref())?;
                for (title, lines) in [
                    ("Constructors", &class.constructors),
                    ("Methods", &class.methods),
                    ("Properties", &class.properties),
                    ("Signals", &class.signals),
                    ("Constants", &class.constants),
                    ("Enums", &class.enums),
                ] {
                    section(out, title, lines)?;
                }
                see_also(out, &class.see_also)?;
                self.fallback_note(
                    out,
                    class
                        .script
                        .as_ref()
                        .map(|s| (s.path.as_str(), s.from_engine)),
                )
            }
            Answer::Member(member) => {
                writeln!(out, "{}", member.signature)?;
                if let Some(script) = &member.script {
                    writeln!(out, "declared at {}", location(&script.path, script.line))?;
                } else if let Some(class) = &member.declaring_class {
                    writeln!(out, "declared in {class}")?;
                }
                lifecycle(out, &member.deprecated, &member.experimental)?;
                prose(out, member.description.as_ref())?;
                if let Some(values) = &member.values {
                    section(out, "Values", values)?;
                }
                see_also(out, &member.see_also)?;
                self.fallback_note(
                    out,
                    member
                        .script
                        .as_ref()
                        .map(|s| (s.path.as_str(), s.from_engine)),
                )
            }
            Answer::Search(search) => {
                for result in &search.results {
                    let shown = result.signature.as_deref().unwrap_or(&result.name);
                    match &result.brief {
                        Some(brief) => writeln!(out, "{:<10} {shown}  — {brief}", result.kind)?,
                        None => writeln!(out, "{:<10} {shown}", result.kind)?,
                    }
                }
                if search.results.is_empty() {
                    writeln!(out, "nothing matches `{}`", search.term)?;
                } else if search.truncated {
                    writeln!(out, "(more results; raise --limit)")?;
                }
                Ok(())
            }
            Answer::Miss(miss) => {
                match &miss.class {
                    Some(class) => writeln!(out, "`{class}` has no member `{}`", miss.query)?,
                    None if miss.missing == "class" => writeln!(out, "no class `{}`", miss.query)?,
                    None => writeln!(
                        out,
                        "no class, function, or constant named `{}`",
                        miss.query
                    )?,
                }
                if !miss.suggestions.is_empty() {
                    writeln!(out, "did you mean: {}", miss.suggestions.join(", "))?;
                }
                Ok(())
            }
        }
    }
}

impl Report {
    /// Says why a project class came from source rather than the engine.
    fn fallback_note(
        &self,
        out: &mut dyn Write,
        script: Option<(&str, bool)>,
    ) -> std::io::Result<()> {
        let (Some((path, false)), Some(scripts)) = (script, &self.project_scripts) else {
            return Ok(());
        };
        let reason = scripts
            .fallbacks
            .iter()
            .find(|fallback| fallback.path == path)
            .map_or("the engine did not document it", |fallback| {
                fallback.reason.as_str()
            });
        writeln!(
            out,
            "\nnote: read from source, without descriptions or inferred types: {reason}"
        )
    }
}

fn location(path: &str, line: Option<usize>) -> String {
    match line {
        Some(line) => format!("{path}:{line}"),
        None => path.to_owned(),
    }
}

fn lifecycle(
    out: &mut dyn Write,
    deprecated: &Option<String>,
    experimental: &Option<String>,
) -> std::io::Result<()> {
    for (label, message) in [("deprecated", deprecated), ("experimental", experimental)] {
        match message.as_deref() {
            Some("") => writeln!(out, "{label}")?,
            Some(message) => writeln!(out, "{label}: {message}")?,
            None => {}
        }
    }
    Ok(())
}

fn prose(out: &mut dyn Write, text: Option<&String>) -> std::io::Result<()> {
    match text {
        Some(text) => writeln!(out, "\n{text}"),
        None => Ok(()),
    }
}

fn section(out: &mut dyn Write, title: &str, lines: &[MemberLine]) -> std::io::Result<()> {
    if lines.is_empty() {
        return Ok(());
    }
    writeln!(out, "\n{title}")?;
    for line in lines {
        let deprecated = if line.deprecated { " (deprecated)" } else { "" };
        match &line.brief {
            Some(brief) => writeln!(out, "  {}{deprecated}  — {brief}", line.signature)?,
            None => writeln!(out, "  {}{deprecated}", line.signature)?,
        }
    }
    Ok(())
}

fn see_also(out: &mut dyn Write, references: &[String]) -> std::io::Result<()> {
    if references.is_empty() {
        return Ok(());
    }
    writeln!(out, "\nsee also: {}", references.join(", "))
}
