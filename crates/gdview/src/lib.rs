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
//! respath, settings, syntax, gltf, variant         (pure: bytes/str in, data out)
//!   └─ files, autoload, scene, declarations, api   (need a Project or a parsed file)
//!        └─ net                                    (needs declarations + scenes)
//! ```
//!
//! `project::Project` is the only type that touches the filesystem, and it only reads.

pub mod api;
pub mod autoload;
pub mod declarations;
pub mod error;
pub mod files;
pub mod gltf;
pub mod net;
pub mod project;
pub mod respath;
pub mod scene;
pub mod settings;
pub mod syntax;
pub mod variant;

pub use error::{Error, Result};
pub use project::Project;
pub use respath::ResPath;
