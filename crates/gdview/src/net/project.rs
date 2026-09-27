use super::*;
use crate::{Project, files::FileQuery};

/// Read-only discovery. Never consults engine configuration, creates caches, or
/// opens a gdproject workspace. Source damage is retained as coverage unknowns.
pub fn analyze_project(project: &Project) -> crate::Result<NetReport> {
    let mut uids = UidMap::build(project)?;
    let mut unknowns = vec![];
    for (uid, paths) in &uids.duplicates {
        uids.by_uid.remove(uid);
        for path in paths {
            unknown(
                &mut unknowns,
                path,
                1,
                format!(
                    "duplicate {}: UID-only targets cannot be resolved uniquely",
                    uid.0
                ),
            );
        }
    }
    let autoloads = project.settings()?.autoloads()?.resolve(project, &uids);
    let mut scripts = vec![];
    let mut scenes = vec![];
    for file in project.files(&FileQuery::default())? {
        let Ok(path) = project.localize(&file) else {
            continue;
        };
        let extension = path.extension().unwrap_or_default().to_ascii_lowercase();
        if matches!(extension.as_str(), "cs" | "scn" | "res" | "gdextension") {
            unknown(
                &mut unknowns,
                &path,
                1,
                "unsupported language/binary/extension file; multiplayer behavior is not inspected",
            );
            continue;
        }
        if !matches!(extension.as_str(), "gd" | "tscn" | "tres") {
            continue;
        }
        let source = match project.read_to_string(&path) {
            Ok(source) => source,
            Err(crate::Error::Io { source, .. })
                if source.kind() == std::io::ErrorKind::InvalidData =>
            {
                unknown(&mut unknowns, &path, 1, "source is not valid UTF-8");
                continue;
            }
            Err(error) => return Err(error),
        };
        if extension == "gd" {
            scripts.push(scan_script(path, &source));
        } else {
            match crate::scene::parse(&source) {
                Ok(scene) => scenes.push((path, scene)),
                Err(error) => {
                    let line = match &error {
                        crate::Error::Parse { line, .. } => *line,
                        _ => 1,
                    };
                    unknown(
                        &mut unknowns,
                        &path,
                        line,
                        format!("scene/resource parse failed: {error}"),
                    );
                }
            }
        }
    }
    Ok(analyze(&NetInput {
        scripts: &scripts,
        scenes: &scenes,
        autoloads: &autoloads,
        uids: &uids,
        unknowns: &unknowns,
    }))
}
