//! Read-only Godot project introspection.
//!
//! Rules for this crate:
//! - Never spawns a process. Never needs a Godot executable.
//! - Never writes to disk. Every function takes paths or bytes and returns data.
//! - Every module is testable with fixture files or in-memory strings alone.
//!
//! The crate is layered bottom-up. Lower modules never import higher ones.
//!
//! ```text
//! respath, settings, syntax, variant, api          (pure: bytes/str in, data out)
//!   └─ files, autoload, scene, declarations, uid   (need a Project or a parsed file)
//!        └─ xref, net                              (cross-file analysis over the above)
//! ```
//!
//! `project::Project` is the only type that touches the filesystem, and it only reads.

pub mod api;
pub mod autoload;
pub mod declarations;
pub mod error;
pub mod files;
pub mod net;
pub mod project;
pub mod respath;
pub mod scene;
pub mod settings;
pub mod syntax;
pub mod uid;
pub mod variant;
pub mod xref;

pub use error::{Error, Result};
pub use project::Project;
pub use respath::ResPath;
