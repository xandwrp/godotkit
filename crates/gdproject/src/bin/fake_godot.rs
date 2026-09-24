//! A stand-in engine for gdproject's offline integration tests.
//!
//! Behaviour is scripted by environment variables so each test can shape one run:
//! - `FAKE_GODOT_HELP`         text printed for `--help` (default: includes all required flags)
//! - `FAKE_GODOT_SCRIPT_MODE`  `envelope` | `error_envelope` | `no_envelope` | `hang` | `crash`
//! - `FAKE_GODOT_PAYLOAD`      JSON payload to wrap in an ok envelope
//! - `FAKE_GODOT_STDOUT` / `FAKE_GODOT_STDERR`  extra lines to print before the envelope
//! - `FAKE_GODOT_EXIT`         exit code (default 0)
//! - `FAKE_GODOT_DELAY_MS`     sleep before exiting (for deadline tests)
//! - `FAKE_GODOT_READY_FILE`   when acting as a game: touch this file with a probe endpoint, then serve
//!   canned probe responses on the port until killed
//!   It records every invocation's argv to `$FAKE_GODOT_LOG` (one JSON line each)
//!   so tests can assert exact command lines.
//!
//! Tests: tests/fake_godot.rs `fake_engine_honours_each_mode` (keeps the fake honest).

fn main() {
    todo!("scripted fake engine; see module docs")
}
