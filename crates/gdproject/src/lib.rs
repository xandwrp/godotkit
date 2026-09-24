//! A Godot project bound to its engine.
//!
//! Rules for this crate:
//! - Every engine invocation goes through [`runner`]. No `Command::new(engine)` anywhere else.
//! - Every invocation has a deadline. There is no "wait forever".
//! - Nothing outlives the gdkit invocation. A game launched by [`run`] is killed when
//!   `run` returns, so there is no pid bookkeeping across calls.
//! - Never mutates a file the user authored. Writes go to `.godot/gdkit/**`,
//!   to scratch copies, or to brand-new files published with create-new semantics.
//! - Operations return `Ok(report)` when the *tool* worked, even if the *project*
//!   failed the check. `Err` means gdkit itself could not do its job.
//!   The CLI maps that to exit 2; report verdicts map to 0/1.
//! - Every harness result crosses the process boundary as a [`protocol::Envelope`]
//!   with a protocol version that is checked on receipt.
//!
//! Layering:
//! ```text
//! config, process, protocol, diagnostics             (no engine)
//!   └─ workspace, engine                             (filesystem state, probe)
//!        └─ runner                                   (one engine invocation)
//!             └─ check, api, resource, cache, run    (operations; probe serves run)
//! ```

pub mod api;
pub mod cache;
pub mod check;
pub mod config;
pub mod diagnostics;
pub mod engine;
pub mod error;
pub mod probe;
pub mod process;
pub mod protocol;
pub mod resource;
pub mod run;
pub mod runner;
pub mod workspace;

pub use engine::Engine;
pub use error::{Error, Result};
pub use workspace::Workspace;
