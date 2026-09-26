//! gdkit: thin CLI over gdproject and gdview.
//!
//! Rules for this crate:
//! - No logic. A command file parses args, opens a Workspace/Engine through `Context`,
//!   calls one or two library functions, and renders. If a command file grows past
//!   ~150 lines, something belongs in gdproject.
//! - stdout carries the result (human text or one JSON document). stderr carries
//!   progress and errors. `--output json` guarantees stdout is exactly one JSON value.
//! - Exit codes: 0 the thing passed, 1 the thing failed (validation, check, run
//!   verdict), 2 gdkit could not do its job (config, engine, I/O, protocol).
//! - A command is a stub until `status::of` marks it `Ready`. Stubs are tagged in
//!   help and refused with exit 2 before dispatch.
//!
//! # Tests (tests/cli.rs, all offline, drive the built binary)
//! - `no_arguments_prints_help_and_exits_zero`
//! - `every_project_command_accepts_project_godot_and_output_flags_uniformly`
//! - `json_mode_writes_exactly_one_json_document_to_stdout_and_nothing_else`
//! - `tool_errors_exit_2_with_error_prefix_on_stderr`
//! - `check_exit_code_follows_report_outcome` (via fake-godot)
//! - `scene_tree_autoloads_refs_settings_net_and_static_check_need_no_engine`
//! - `init_writes_config_and_refuses_to_overwrite` (via fake-godot)
//! - `init_without_godot_follows_the_global_default` (via fake-godot)
//! - `config_set_get_unset_list_round_trip` (via fake-godot)
//! - `api_exit_codes_follow_the_answer_and_json_is_one_document` (via fake-godot)
//! - `status_table_matches_what_each_command_does`
//! - `help_tags_every_command_that_is_not_ready`

mod cli;
mod commands;
mod context;
mod render;
mod status;

use std::process::ExitCode;

use clap::{CommandFactory, FromArgMatches};

fn main() -> ExitCode {
    let mut command = status::decorate(cli::Cli::command());
    let matches = command.get_matches_mut();
    let cli =
        cli::Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.format(&mut command).exit());
    let path = status::invoked(&matches);
    if status::of(&path) == status::Status::Stub && !status::stubs_unlocked() {
        eprintln!("error: `gdkit {path}` is not implemented yet");
        return ExitCode::from(2);
    }
    let context = context::Context::from_env(cli.output);
    match commands::dispatch(&context, cli.command) {
        Ok(exit) => exit.into(),
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(2)
        }
    }
}
