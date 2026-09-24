//! Durable JSON records under `.godot/gdkit/**`: sessions, scenario runs, artifacts.
//!
//! Writes are temp+rename. Reads are tolerant: a bad file is reported alongside
//! the good ones, never fatal. Every record type carries `schema_version`; a
//! newer version than we know is a `RecordProblem`, not a crash.
//!
//! # Tests (tests/records.rs)
//! - `write_is_atomic_and_never_leaves_partial_files` (kill mid-write simulated by injecting a failing writer)
//! - `read_all_returns_good_records_and_lists_problems_for_corrupt_or_newer_files`
//! - `update_reads_modifies_and_rewrites_atomically`
//! - `ordering_is_by_file_name_which_is_by_creation_stamp`

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

pub trait Record: Serialize + DeserializeOwned {
    const SCHEMA_VERSION: u32;
    fn schema_version(&self) -> u32;
    /// File stem; must be unique and sortable (`<unix_ms>-<name>`).
    fn file_stem(&self) -> String;
}

pub struct RecordStore<T: Record> {
    dir: PathBuf,
    _marker: std::marker::PhantomData<T>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordProblem {
    pub path: PathBuf,
    pub message: String,
}

pub struct ReadAll<T> {
    pub records: Vec<(PathBuf, T)>,
    pub problems: Vec<RecordProblem>,
}

impl<T: Record> RecordStore<T> {
    pub fn open(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into(), _marker: std::marker::PhantomData }
    }
    pub fn dir(&self) -> &Path {
        &self.dir
    }
    pub fn write(&self, record: &T) -> crate::Result<PathBuf> {
        todo!()
    }
    pub fn read(&self, path: &Path) -> crate::Result<T> {
        todo!()
    }
    pub fn read_all(&self) -> crate::Result<ReadAll<T>> {
        todo!()
    }
    pub fn update(&self, path: &Path, edit: impl FnOnce(&mut T)) -> crate::Result<()> {
        todo!()
    }
}
