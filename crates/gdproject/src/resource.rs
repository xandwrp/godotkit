//! Resource schema discovery and verified creation from `gdview::variant` specs.
//!
//! `create` flow:
//! 1. Validate the spec offline (`VariantJson::from_json`, target shape, destination is a new `.tres` under the project).
//! 2. Stage into a temp file inside the project dir (same filesystem as the destination).
//! 3. `run_harness ResourceCreate` with the spec: harness builds, assigns, saves to the staged path, reloads
//!    with `CACHE_MODE_IGNORE`, and echoes every property back; any divergence is an error envelope.
//! 4. Compare echo to spec in Rust (second, independent verification).
//! 5. `workspace::publish_new_file(staged, destination)`.
//!
//! `schema` flow: `run_harness ResourceSchema` on the real project (read-only;
//! the harness writes nothing) → raw property entries → `gdview::property::fields`.
//!
//! Both harnesses run the project's autoloads, as any Godot `--script` run does,
//! so engine errors and warnings are reported (`engine_diagnostics`), never a
//! verdict: an autoload that logs an error must not block every resource
//! command. The harness's own checks and the echo comparison decide.
//! A script that fails to load, or an unknown class name, in a project with no
//! class cache gets a hint to import it: script classes (`class_name`) resolve
//! only after an import.
//!
//! # Tests (tests/resource.rs)
//! Offline: `schema_passes_the_target_and_derives_fields_from_the_raw_payload`,
//! `schema_errors_keep_the_field_and_hint_at_import_only_without_a_class_cache`,
//! `schema_reports_engine_diagnostics_without_failing`, `schema_rejects_a_success_envelope_without_a_payload`,
//! `spec_validation_rejects_bad_targets_paths_and_variants`, `destination_must_be_new_tres_inside_project`,
//! `references_must_exist_in_the_project`,
//! `echo_mismatch_is_a_verify_failure_and_nothing_is_published`, `warnings_do_not_fail_create`,
//! `staged_file_is_removed_on_every_failure_path`.
//! Engine (`#[ignore]`): `real_engine_round_trips_every_variant_type` (moved from legacy resource_containers),
//! `real_engine_schema_reports_fields_hints_enums_and_typed_arrays_from_hint_string`,
//! `real_engine_create_nested_resources_and_refs`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use gdview::ResPath;
pub use gdview::property::FieldSchema;
use gdview::property::{self, PropertyInfo};
use gdview::variant::{Limits, ResourceSpec, ResourceTarget, VariantJson};
use serde::{Deserialize, Serialize};

use crate::diagnostics::Diagnostic;
use crate::engine::Engine;
use crate::protocol::ProtocolError;
use crate::runner::{self, Harness, Invocation};
use crate::workspace::Workspace;

pub const DEFAULT_DEADLINE: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq)]
pub struct CreateSpec {
    pub target: ResourceTarget,
    pub properties: BTreeMap<String, VariantJson>,
}

impl CreateSpec {
    /// A spec file is one resource object, the same shape a nested `$resource`
    /// takes: `{"class"|"script": …, "properties"?: {name: value…}}`.
    /// Errors name the offending value (`at /properties/offset: …`).
    pub fn from_json(value: &serde_json::Value) -> crate::Result<Self> {
        let ResourceSpec { target, properties } =
            ResourceSpec::from_json(value, &Limits::default())?;
        Ok(Self { target, properties })
    }

    fn as_resource_spec(&self) -> ResourceSpec {
        ResourceSpec {
            target: self.target.clone(),
            properties: self.properties.clone(),
        }
    }
}

/// The OS path `destination` will be published at, once it passes: a `.tres`
/// name that is not hidden (Godot's editor skips dotfiles), inside a directory
/// that exists and is reached without symlinks, with nothing there yet.
pub fn check_destination(workspace: &Workspace, destination: &ResPath) -> crate::Result<PathBuf> {
    let invalid = |why: &str| crate::Error::Invalid(format!("--out {destination}: {why}"));
    let name = destination.file_name();
    if !name.ends_with(".tres") || name.len() == ".tres".len() {
        return Err(invalid("must name a .tres file"));
    }
    if name.starts_with('.') {
        return Err(invalid("hidden files are ignored by the Godot editor"));
    }
    let mut directory = workspace.root().to_path_buf();
    let mut at = ResPath::root();
    for segment in destination
        .parent()
        .unwrap_or_else(ResPath::root)
        .relative()
        .split('/')
    {
        if segment.is_empty() {
            continue;
        }
        directory.push(segment);
        at = at.join(segment)?;
        match std::fs::symlink_metadata(&directory) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(invalid(&format!(
                    "{at} does not exist; create the directory first"
                )));
            }
            Err(source) => {
                return Err(crate::Error::Io {
                    path: directory,
                    source,
                });
            }
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(invalid(&format!(
                    "{at} is a symlink; gdkit writes only through real directories"
                )));
            }
            Ok(meta) if !meta.is_dir() => {
                return Err(invalid(&format!("{at} is not a directory")));
            }
            Ok(_) => {}
        }
    }
    let os_path = directory.join(name);
    match std::fs::symlink_metadata(&os_path) {
        Ok(_) => Err(invalid("already exists; gdkit never overwrites a file")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(os_path),
        Err(source) => Err(crate::Error::Io {
            path: os_path,
            source,
        }),
    }
}

/// Every script and `$ref` the spec names exists in the project, so a typo
/// fails before the engine starts.
pub fn check_references(workspace: &Workspace, spec: &CreateSpec) -> crate::Result<()> {
    let spec = spec.as_resource_spec();
    let mut missing: Vec<&ResPath> = spec
        .paths()
        .into_iter()
        .filter(|path| !workspace.project().globalize(path).exists())
        .collect();
    missing.sort();
    missing.dedup();
    if missing.is_empty() {
        return Ok(());
    }
    let missing: Vec<&str> = missing.iter().map(|path| path.as_str()).collect();
    Err(crate::Error::Invalid(format!(
        "the spec names files that do not exist: {}",
        missing.join(", ")
    )))
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CreateReport {
    pub path: ResPath,
    pub os_path: PathBuf,
    pub target: ResourceTarget,
    pub properties_written: usize,
    pub warnings: Vec<String>,
}

pub fn create(
    workspace: &Workspace,
    engine: &Engine,
    spec: &CreateSpec,
    destination: &ResPath,
    deadline: Duration,
) -> crate::Result<CreateReport> {
    todo!()
}

pub const SCHEMA_VERSION: u32 = 1;

const CLASS_CACHE: &str = ".godot/global_script_class_cache.cfg";

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ResourceSchema {
    pub schema_version: u32,
    pub target: ResourceTarget,
    /// The native class instantiated: the target class, or the script's base.
    pub class: String,
    /// The script's `class_name`, when it has one.
    pub script_class: Option<String>,
    pub fields: Vec<FieldSchema>,
    /// Engine errors and warnings from the run (often the project's autoloads).
    pub engine_diagnostics: Vec<Diagnostic>,
}

#[derive(Deserialize)]
struct SchemaPayload {
    class: String,
    script_class: Option<String>,
    properties: Vec<PropertyInfo>,
}

pub fn schema(
    workspace: &Workspace,
    engine: &Engine,
    target: &ResourceTarget,
    deadline: Duration,
) -> crate::Result<ResourceSchema> {
    let mut invocation = Invocation::new(engine, workspace.root(), deadline);
    invocation.user_args = match target {
        ResourceTarget::Class(class) => vec!["class".into(), class.into()],
        ResourceTarget::Script(script) => vec!["script".into(), script.as_str().into()],
    };
    let run = runner::run_harness::<SchemaPayload>(&invocation, Harness::ResourceSchema)
        .map_err(|error| import_hint(workspace, error))?;
    let payload = run.envelope.payload.ok_or(crate::Error::Protocol {
        harness: Harness::ResourceSchema.name(),
        source: ProtocolError::Malformed("success envelope without a payload".into()),
    })?;
    Ok(ResourceSchema {
        schema_version: SCHEMA_VERSION,
        target: target.clone(),
        class: payload.class,
        script_class: payload.script_class,
        fields: property::fields(&payload.properties)?,
        engine_diagnostics: run.diagnostics,
    })
}

/// In a never-imported project, a script that fails to load, or a class name
/// the engine does not know, usually involves a script class with no cache yet.
fn import_hint(workspace: &Workspace, error: crate::Error) -> crate::Error {
    match error {
        crate::Error::Harness {
            harness,
            stage,
            message,
            field,
        } if stage == "target"
            && (field.as_deref() == Some("script") || message.starts_with("Unknown class"))
            && !workspace.root().join(CLASS_CACHE).is_file() =>
        {
            crate::Error::Harness {
                harness,
                stage,
                message: format!(
                    "{message} (the project has not been imported, so script classes do not \
                     resolve; open it in the Godot editor once, or run `godot --headless \
                     --editor --import`)"
                ),
                field,
            }
        }
        other => other,
    }
}
