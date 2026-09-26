//! Resource schema discovery and verified creation from `gdview::variant` specs.
//!
//! `create` flow:
//! 1. Validate offline: the spec (`CreateSpec::from_json`), `check_destination`
//!    (a new, non-hidden `.tres` in an existing real directory) and
//!    `check_references` (every script and `$ref` exists). Then take the workspace lock.
//! 2. Stage as `.<stem>.gdkit-staged-<pid>.tres` beside the destination (same
//!    filesystem; hidden, so an open editor ignores it; `.tres` so Godot picks
//!    the text saver). A guard removes it on every failure path.
//! 3. `run_harness ResourceCreate <spec file> <staged res path>`: the harness
//!    builds, assigns, saves to the staged path, reloads with `CACHE_MODE_IGNORE`,
//!    and echoes each spec'd property; any divergence is an error envelope.
//! 4. `verify_echo` in Rust (second, independent verification).
//! 5. `workspace::publish_new_file(staged, destination)`: create-new, atomic.
//!
//! Echo equality is exact, with only lossless readings of a spec allowed: an
//! int for a float equal to it, a string for a StringName or NodePath, plain
//! arrays and dictionaries for typed ones, and inline resources whose echo
//! also lists the class defaults the spec left out. A float the engine stores
//! differently (`0.1` in a 32-bit `Vector2`) is a mismatch naming the stored value.
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
//! `echo_comparison_allows_only_lossless_readings_of_the_spec`,
//! `create_stages_beside_the_destination_and_publishes_the_verified_file`,
//! `echo_mismatch_is_a_verify_failure_and_nothing_is_published`,
//! `engine_diagnostics_are_reported_and_do_not_fail_create`,
//! `staged_file_is_removed_on_every_failure_path`, `invalid_requests_fail_before_the_engine_runs`.
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
use crate::workspace::{IsolatedCopy, Workspace, publish_new_file};

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
    /// Each spec'd property as the engine stored it, after the reload.
    pub properties: BTreeMap<String, serde_json::Value>,
    /// Engine errors and warnings from the run (often the project's autoloads).
    pub engine_diagnostics: Vec<Diagnostic>,
}

#[derive(Deserialize)]
struct CreatePayload {
    echo: serde_json::Map<String, serde_json::Value>,
}

pub fn create(
    workspace: &Workspace,
    engine: &Engine,
    spec: &CreateSpec,
    destination: &ResPath,
    deadline: Duration,
) -> crate::Result<CreateReport> {
    let os_path = check_destination(workspace, destination)?;
    check_references(workspace, spec)?;
    let _lock = workspace.lock()?;
    let stem = destination.file_name().trim_end_matches(".tres");
    let staged_name = format!(".{stem}.gdkit-staged-{}.tres", std::process::id());
    let staged_res = destination
        .parent()
        .unwrap_or_else(ResPath::root)
        .join(&staged_name)?;
    let staged = Staged::claim(os_path.with_file_name(&staged_name))?;

    let scratch = IsolatedCopy::bare()?;
    let spec_file = scratch.path().join("spec.json");
    let spec_json = match VariantJson::Resource(spec.as_resource_spec()).to_json() {
        serde_json::Value::Object(mut tag) => tag.remove("$resource").unwrap_or_default(),
        _ => unreachable!("a resource encodes as a $resource tag"),
    };
    std::fs::write(&spec_file, serde_json::to_vec(&spec_json)?).map_err(|source| {
        crate::Error::Io {
            path: spec_file.clone(),
            source,
        }
    })?;
    let mut invocation = Invocation::new(engine, workspace.root(), deadline);
    invocation.user_args = vec![spec_file.into_os_string(), staged_res.as_str().into()];
    let run = runner::run_harness::<CreatePayload>(&invocation, Harness::ResourceCreate)
        .map_err(|error| import_hint(workspace, error))?;
    let payload = run.envelope.payload.ok_or(crate::Error::Protocol {
        harness: Harness::ResourceCreate.name(),
        source: ProtocolError::Malformed("success envelope without a payload".into()),
    })?;
    let mut echo = BTreeMap::new();
    for (name, value) in &payload.echo {
        let decoded = VariantJson::from_json(value, &Limits::default()).map_err(|error| {
            crate::Error::Protocol {
                harness: Harness::ResourceCreate.name(),
                source: ProtocolError::Malformed(format!("echo of {name}: {error}")),
            }
        })?;
        echo.insert(name.clone(), decoded);
    }
    verify_echo(&spec.properties, &echo).map_err(|mismatch| crate::Error::Harness {
        harness: Harness::ResourceCreate.name(),
        stage: "verify".into(),
        message: mismatch.message,
        field: Some(mismatch.field),
    })?;
    if !std::fs::symlink_metadata(&staged.path).is_ok_and(|meta| meta.is_file()) {
        return Err(crate::Error::Harness {
            harness: Harness::ResourceCreate.name(),
            stage: "save".into(),
            message: format!("the engine reported success but wrote no file at {staged_res}"),
            field: None,
        });
    }
    publish_new_file(&staged.path, &os_path)?;
    Ok(CreateReport {
        path: destination.clone(),
        os_path,
        target: spec.target.clone(),
        properties_written: spec.properties.len(),
        properties: payload.echo.into_iter().collect(),
        engine_diagnostics: run.diagnostics,
    })
}

/// The staging file; removed on drop, which after a publish finds nothing left.
struct Staged {
    path: PathBuf,
}

impl Staged {
    /// Refuses a name that is already taken rather than letting the engine overwrite it.
    fn claim(path: PathBuf) -> crate::Result<Self> {
        if std::fs::symlink_metadata(&path).is_ok() {
            return Err(crate::Error::Invalid(format!(
                "{} already exists; remove the leftover staging file and retry",
                path.display()
            )));
        }
        Ok(Self { path })
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Where and how an echo differs from the spec.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mismatch {
    /// `properties.<name>` then `.properties.<name>` into inline resources and
    /// `[index]` / `[key]` into containers and components.
    pub field: String,
    pub message: String,
}

/// A value as a person reads it: scalars bare (`2.0`, not a tagged float).
fn show(value: &VariantJson) -> String {
    match value {
        VariantJson::Float(f) if f.is_finite() && !(*f == 0.0 && f.is_sign_negative()) => {
            serde_json::Number::from_f64(*f).map_or_else(|| f.to_string(), |n| n.to_string())
        }
        other => other.to_json().to_string(),
    }
}

/// Every spec'd property must come back as written, or as one of the
/// lossless readings the module doc lists.
pub fn verify_echo(
    spec: &BTreeMap<String, VariantJson>,
    echo: &BTreeMap<String, VariantJson>,
) -> std::result::Result<(), Mismatch> {
    properties(spec, echo, "properties")
}

fn properties(
    spec: &BTreeMap<String, VariantJson>,
    echo: &BTreeMap<String, VariantJson>,
    at: &str,
) -> std::result::Result<(), Mismatch> {
    for (name, expected) in spec {
        let field = format!("{at}.{name}");
        match echo.get(name) {
            Some(actual) => matches(expected, actual, &field)?,
            None => {
                return Err(Mismatch {
                    field,
                    message: "the engine did not echo this property".into(),
                });
            }
        }
    }
    Ok(())
}

fn matches(spec: &VariantJson, echo: &VariantJson, at: &str) -> std::result::Result<(), Mismatch> {
    use VariantJson as V;
    let differ = || Mismatch {
        field: at.to_owned(),
        message: format!(
            "the engine stored {} where the spec has {}",
            show(echo),
            show(spec)
        ),
    };
    let items = |spec: &[V], echo: &[V]| {
        if spec.len() != echo.len() {
            return Err(differ());
        }
        spec.iter()
            .zip(echo)
            .enumerate()
            .try_for_each(|(index, (spec, echo))| matches(spec, echo, &format!("{at}[{index}]")))
    };
    match (spec, echo) {
        (V::Float(a), V::Float(b)) if a.is_nan() && b.is_nan() => Ok(()),
        (V::Float(a), V::Float(b)) if a.to_bits() == b.to_bits() => Ok(()),
        // Never `==`: it equates -0.0 and 0.0.
        (V::Float(_), V::Float(_)) => Err(differ()),
        (V::Int(n), V::Float(f))
            if n.unsigned_abs() <= gdview::variant::MAX_SAFE_INTEGER as u64 && *n as f64 == *f =>
        {
            Ok(())
        }
        (V::String(text), V::Tagged { type_name, value })
            if matches!(
                type_name,
                gdview::variant::VariantType::StringName | gdview::variant::VariantType::NodePath
            ) && matches!(value.as_ref(), V::String(stored) if stored == text) =>
        {
            Ok(())
        }
        (
            V::Tagged {
                type_name: a,
                value: x,
            },
            V::Tagged {
                type_name: b,
                value: y,
            },
        ) if a == b => matches(x, y, at),
        (V::Array(spec), V::Array(echo) | V::TypedArray { items: echo, .. }) => items(spec, echo),
        (
            V::TypedArray {
                element: a,
                items: spec,
            },
            V::TypedArray {
                element: b,
                items: echo,
            },
        ) if a == b => items(spec, echo),
        (V::Dictionary(spec), V::Dictionary(echo)) => {
            if spec.len() != echo.len() {
                return Err(differ());
            }
            for (key, value) in spec {
                let at = format!("{at}[{}]", key.to_json());
                let stored = echo
                    .iter()
                    .find(|(stored, _)| matches(key, stored, &at).is_ok())
                    .ok_or_else(differ)?;
                matches(value, &stored.1, &at)?;
            }
            Ok(())
        }
        (V::Resource(spec), V::Resource(echo)) if spec.target == echo.target => properties(
            &spec.properties,
            &echo.properties,
            &format!("{at}.properties"),
        ),
        (spec, echo) if spec == echo => Ok(()),
        _ => Err(differ()),
    }
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
