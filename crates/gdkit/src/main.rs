//! gdkit: thin CLI over gdproject and gdview.
//!
//! Rules for this crate:
//! - No logic. A command file parses args, opens a Workspace/Engine through `Context`,
//!   calls one or two library functions, and renders. If a command file grows past
//!   ~150 lines, something belongs in gdproject.
//! - stdout carries the result (human text or one JSON document). stderr carries
//!   progress and errors. `--output json` guarantees stdout is exactly one JSON value.
//! - Exit codes: 0 the thing passed, 1 the thing failed (validation, check, inspection
//!   findings), 2 gdkit could not do its job (config, engine, I/O, protocol).
//!
//! # Tests (tests/cli.rs, all offline, drive the built binary)
//! - `no_arguments_prints_help_and_exits_zero`
//! - `every_project_command_accepts_project_godot_and_output_flags_uniformly`
//! - `json_mode_writes_exactly_one_json_document_to_stdout_and_nothing_else`
//! - `tool_errors_exit_2_with_error_prefix_on_stderr`
//! - `check_exit_code_follows_report_outcome` (via fake-godot)
//! - `scene_tree_autoloads_animation_list_and_cache_status_need_no_engine`
//! - `init_writes_config_and_refuses_to_overwrite` (via fake-godot)

mod cli;
mod commands;
mod context;
mod render;

use std::process::ExitCode;

use clap::Parser;

fn main() -> ExitCode {
    let cli = cli::Cli::parse();
    let context = context::Context::from_env(cli.output);
    match commands::dispatch(&context, cli.command) {
        Ok(exit) => exit.into(),
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(2)
        }
    }
}
