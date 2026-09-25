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

use crate::files::FileQuery;
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
    /// Reads every `.uid` sidecar, `.import` file, and text scene/resource
    /// header. Mirrors what the engine sees: `.gitignore` is not consulted,
    /// hidden and `.gdignore`d directories are skipped.
    pub fn build(project: &Project) -> crate::Result<Self> {
        let query = FileQuery {
            respect_gitignore: false,
            ..FileQuery::with_extensions(["uid", "import", "tscn", "tres"])
        };
        let mut claims: Vec<(Uid, ResPath)> = Vec::new();
        for file in project.files(&query)? {
            let Ok(res) = project.localize(&file) else {
                continue;
            };
            let extension = res.extension().unwrap_or_default().to_ascii_lowercase();
            // Unreadable or non-UTF-8 files carry no uid we can use; the engine phase reports them.
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            let (target, uid) = match extension.as_str() {
                "uid" => (strip_suffix(&res, ".uid"), sidecar_uid(&text)),
                "import" => (strip_suffix(&res, ".import"), import_uid(&text)),
                _ => (Some(res), header_uid(&text)),
            };
            if let (Some(target), Some(uid)) = (target, uid) {
                claims.push((Uid(uid), target));
            }
        }
        Ok(Self::from_claims(claims))
    }

    /// Builds from `(uid, path)` claims in priority order; later claims of a
    /// uid already taken are reported as duplicates.
    pub fn from_claims(claims: impl IntoIterator<Item = (Uid, ResPath)>) -> Self {
        let mut map = Self::default();
        let mut claimants: BTreeMap<Uid, Vec<ResPath>> = BTreeMap::new();
        for (uid, path) in claims {
            map.by_uid
                .entry(uid.clone())
                .or_insert_with(|| path.clone());
            map.by_path
                .entry(path.clone())
                .or_insert_with(|| uid.clone());
            claimants.entry(uid).or_default().push(path);
        }
        map.duplicates = claimants
            .into_iter()
            .filter(|(_, paths)| paths.len() > 1)
            .collect();
        map
    }
    pub fn resolve(&self, uid: &Uid) -> Option<&ResPath> {
        self.by_uid.get(uid)
    }
    pub fn uid_of(&self, path: &ResPath) -> Option<&Uid> {
        self.by_path.get(path)
    }
}

fn strip_suffix(path: &ResPath, suffix: &str) -> Option<ResPath> {
    ResPath::parse(path.as_str().strip_suffix(suffix)?).ok()
}

fn is_uid(text: &str) -> bool {
    text.len() > "uid://".len()
        && text.starts_with("uid://")
        && text.bytes().all(|b| b.is_ascii_graphic() && b != b'"')
}

/// A `.uid` sidecar holds the uid on its first line.
fn sidecar_uid(text: &str) -> Option<String> {
    let uid = text.lines().next()?.trim();
    is_uid(uid).then(|| uid.to_owned())
}

/// `uid="uid://…"` in the `[remap]` section of a `.import` file.
fn import_uid(text: &str) -> Option<String> {
    let mut section = "";
    for line in text.lines() {
        let line = line.trim();
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            section = name;
        } else if section == "remap"
            && let Some(value) = line.strip_prefix("uid=")
        {
            let uid = value.trim().trim_matches('"');
            return is_uid(uid).then(|| uid.to_owned());
        }
    }
    None
}

/// `uid="uid://…"` in the `[gd_scene …]` / `[gd_resource …]` header line.
fn header_uid(text: &str) -> Option<String> {
    let header = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    if !(header.starts_with("[gd_scene") || header.starts_with("[gd_resource")) {
        return None;
    }
    let start = header.find(" uid=\"")? + " uid=\"".len();
    let uid = &header[start..start + header[start..].find('"')?];
    is_uid(uid).then(|| uid.to_owned())
}
