//! `uid://` resolution from `.uid` sidecar files and `.import` metadata. Offline.
//!
//! Godot 4.4+ writes `<file>.uid` next to every script and shader, and stores
//! imported-asset uids in `<file>.import`. Scenes and resources carry `uid=` in
//! their header. Together that covers every uid a project can reference without
//! reading `.godot/uid_cache.bin`.
//!
//! # Tests (tests/uid.rs)
//! - `builds_map_from_sidecars_import_files_and_scene_headers`
//! - `resolve_returns_none_for_unknown_uid_not_error`
//! - `duplicate_uids_are_reported_with_both_paths`
//! - `uid_of_path_is_inverse_of_resolve`

use std::collections::BTreeMap;

use serde::Serialize;

use crate::project::Project;
use crate::respath::{ResPath, Uid};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct UidMap {
    pub by_uid: BTreeMap<Uid, ResPath>,
    pub by_path: BTreeMap<ResPath, Uid>,
    /// Same uid claimed by more than one file.
    pub duplicates: Vec<(Uid, Vec<ResPath>)>,
}

impl UidMap {
    pub fn build(project: &Project) -> crate::Result<Self> {
        todo!()
    }
    pub fn resolve(&self, uid: &Uid) -> Option<&ResPath> {
        self.by_uid.get(uid)
    }
    pub fn uid_of(&self, path: &ResPath) -> Option<&Uid> {
        self.by_path.get(path)
    }
}
