//! Which commands work. `of` is the one table to edit when a command lands.
//!
//! - Help tags every command that isn't `Ready`, so `--help` never advertises a stub.
//! - `main` refuses a stub before dispatch: `error: ...` on stderr, exit 2.
//! - Debug builds only: `GDKIT_ALLOW_STUBS=1` skips that refusal, so the drift
//!   test can prove each stub still panics and each `Ready` command doesn't.
//!
//! Paths are subcommand names joined by spaces: `check`, `settings input`.
//! Only leaves are listed; a parent's tag is derived from its leaves.

use clap::builder::styling::{AnsiColor, Style};
use clap::{ArgMatches, Command};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ready,
    Stub,
}

pub fn of(path: &str) -> Status {
    match path {
        "check" | "init" | "api" | "config get" | "config set" | "config unset" | "config list" => {
            Status::Ready
        }
        _ => Status::Stub,
    }
}

pub fn stubs_unlocked() -> bool {
    cfg!(debug_assertions) && std::env::var_os("GDKIT_ALLOW_STUBS").is_some()
}

/// The subcommand path the user invoked, e.g. `resource create`.
pub fn invoked(matches: &ArgMatches) -> String {
    let mut names = Vec::new();
    let mut matches = matches;
    while let Some((name, sub)) = matches.subcommand() {
        names.push(name);
        matches = sub;
    }
    names.join(" ")
}

/// Every runnable path under `command`, where `command` itself sits at `path`.
pub fn leaves(command: &Command, path: &str) -> Vec<String> {
    let mut subcommands = command.get_subcommands().peekable();
    if subcommands.peek().is_none() {
        return vec![path.to_owned()];
    }
    subcommands
        .flat_map(|sub| leaves(sub, &join(path, sub.get_name())))
        .collect()
}

/// The help tag for the command at `path`, or `None` when all of it works.
pub fn tag(command: &Command, path: &str) -> Option<&'static str> {
    let leaves = leaves(command, path);
    let stubs = leaves
        .iter()
        .filter(|leaf| of(leaf) == Status::Stub)
        .count();
    match stubs {
        0 => None,
        n if n == leaves.len() => Some("not implemented"),
        _ => Some("partly implemented"),
    }
}

/// Appends each subcommand's tag to its about text. Red for stubs, yellow for
/// partial; clap drops the color when help isn't going to a terminal.
pub fn decorate(command: Command) -> Command {
    decorate_under(command, "")
}

fn decorate_under(mut command: Command, path: &str) -> Command {
    let names: Vec<String> = command
        .get_subcommands()
        .map(|sub| sub.get_name().to_owned())
        .collect();
    for name in names {
        let path = join(path, &name);
        command = command.mut_subcommand(&name, |sub| {
            let sub = decorate_under(sub, &path);
            let Some(tag) = tag(&sub, &path) else {
                return sub;
            };
            let color = if tag == "not implemented" {
                AnsiColor::Red
            } else {
                AnsiColor::Yellow
            };
            let style = Style::new().fg_color(Some(color.into()));
            let about = sub
                .get_about()
                .map(|about| format!("{about} "))
                .unwrap_or_default();
            sub.about(format!("{about}{style}({tag}){style:#}"))
        });
    }
    command
}

fn join(path: &str, name: &str) -> String {
    if path.is_empty() {
        name.to_owned()
    } else {
        format!("{path} {name}")
    }
}
