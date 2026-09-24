//! Resource schema discovery and verified creation from `gdview::variant` specs.
//!
//! `create` flow:
//! 1. Validate the spec offline (`VariantJson::from_json`, target shape, destination is a new `.tres` under the project).
//! 2. Stage into a temp file inside the project dir (same filesystem as the destination).
//! 3. `run_harness ResourceCreate` with the spec: harness builds, assigns, saves to the staged path, reloads
//!    with `CACHE_MODE_IGNORE`, and echoes every property back; any divergence is an error envelope.
//! 4. Compare echo to spec in Rust (second, independent verification).
//! 5. `workspace::publish_new_file(staged, destination)`.
//!   A `WARNING:` in engine output is reported, never a failure. `ERROR:` fails at stage `engine`.
//!
//! # Tests (tests/resource.rs)
//! Offline: `spec_validation_rejects_bad_targets_paths_and_variants`, `destination_must_be_new_tres_inside_project`,
//! `echo_mismatch_is_a_verify_failure_and_nothing_is_published`, `warnings_do_not_fail_create`,
//! `staged_file_is_removed_on_every_failure_path`.
//! Engine (`#[ignore]`): `real_engine_round_trips_every_variant_type` (moved from legacy resource_containers),
//! `real_engine_schema_reports_fields_hints_enums_and_typed_arrays_from_hint_string`,
//! `real_engine_create_nested_resources_and_refs`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use gdview::ResPath;
use gdview::variant::{ResourceTarget, VariantJson, VariantType};
use serde::{Deserialize, Serialize};

use crate::engine::Engine;
use crate::workspace::Workspace;

pub const DEFAULT_DEADLINE: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq)]
pub struct CreateSpec {
    pub target: ResourceTarget,
    pub properties: BTreeMap<String, VariantJson>,
}

impl CreateSpec {
    pub fn from_json(value: &serde_json::Value) -> crate::Result<Self> {
        todo!()
    }
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResourceSchema {
    pub schema_version: u32,
    pub target: ResourceTarget,
    pub fields: Vec<FieldSchema>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FieldSchema {
    pub name: String,
    pub variant_type: VariantType,
    pub class_name: Option<String>,
    pub element_type: Option<VariantType>,
    pub default: serde_json::Value,
    pub hint: Option<String>,
    pub hint_string: Option<String>,
    pub enum_choices: Vec<String>,
    pub stored: bool,
    /// Accepted spec shapes for this field, as documentation for generators.
    pub accepts: Vec<String>,
}

pub fn schema(
    workspace: &Workspace,
    engine: &Engine,
    target: &ResourceTarget,
    deadline: Duration,
) -> crate::Result<ResourceSchema> {
    todo!()
}
