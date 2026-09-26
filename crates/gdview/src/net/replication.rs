use super::*;
use crate::scene::{FileKind, Properties, ResourceRef, Value};
use std::collections::BTreeMap;

pub(super) fn collect(input: &NetInput<'_>, report: &mut NetReport) {
    for (path, scene) in input.scenes {
        if scene.kind != FileKind::Scene {
            continue;
        }
        for node in &scene.nodes {
            let at = node.path();
            match node.type_name.as_deref() {
                Some("MultiplayerSpawner") => {
                    let spawn_path = path_property(
                        &node.properties,
                        "spawn_path",
                        "",
                        path,
                        node.line,
                        &mut report.unknowns,
                    );
                    let mut auto_spawn_list = vec![];
                    if let Some(value) = node.properties.get("_spawnable_scenes") {
                        let values = match value {
                            Value::Array(values) => Some(values),
                            Value::Call { name, args } if name == "PackedStringArray" => Some(args),
                            _ => None,
                        };
                        if let Some(values) = values {
                            for value in values {
                                let target = value.as_str().and_then(|text| {
                                    if text.starts_with("uid://") {
                                        input
                                            .uids
                                            .resolve(&crate::respath::Uid(text.into()))
                                            .cloned()
                                    } else {
                                        ResPath::parse(text).ok()
                                    }
                                });
                                match target {
                                    Some(target) => auto_spawn_list.push(target),
                                    None => unknown(
                                        &mut report.unknowns,
                                        path,
                                        node.line,
                                        format!("{at:?}: invalid or unresolved spawnable scene"),
                                    ),
                                }
                            }
                        } else {
                            unknown(
                                &mut report.unknowns,
                                path,
                                node.line,
                                "invalid spawner scene list",
                            );
                        }
                    }
                    report.spawners.push(Spawner {
                        scene: path.clone(),
                        node: at,
                        spawn_path,
                        auto_spawn_list,
                        line: node.line,
                    });
                }
                Some("MultiplayerSynchronizer") => {
                    let root_path = path_property(
                        &node.properties,
                        "root_path",
                        "..",
                        path,
                        node.line,
                        &mut report.unknowns,
                    );
                    let mut properties = vec![];
                    if let Some(value) = node.properties.get("replication_config")
                        && value != &Value::Null
                    {
                        let config = value
                            .as_resource_ref()
                            .and_then(|r| config(input, scene, &r));
                        match config {
                            Some(props) => {
                                properties =
                                    synced_properties(props, path, node.line, &mut report.unknowns)
                            }
                            None => unknown(
                                &mut report.unknowns,
                                path,
                                node.line,
                                format!(
                                    "{}: replication config is missing, binary, mistyped or unsupported",
                                    at.0
                                ),
                            ),
                        }
                    }
                    report.synchronizers.push(Synchronizer {
                        scene: path.clone(),
                        node: at,
                        root_path,
                        properties,
                        line: node.line,
                    });
                }
                _ => {}
            }
        }
    }
}

fn config<'a>(
    input: &NetInput<'a>,
    scene: &'a SceneFile,
    reference: &ResourceRef,
) -> Option<&'a Properties> {
    match reference {
        ResourceRef::Sub(id) => {
            let sub = scene.sub_resource(id)?;
            (sub.type_name == "SceneReplicationConfig").then_some(&sub.properties)
        }
        ResourceRef::Ext(id) => {
            let ext = scene.ext_resource(id)?;
            if ext.type_name != "SceneReplicationConfig" {
                return None;
            }
            let target = ext
                .uid
                .as_ref()
                .and_then(|uid| input.uids.resolve(uid))
                .unwrap_or(&ext.path);
            let (_, file) = input.scenes.iter().find(|(path, _)| path == target)?;
            file.resource.as_ref()
        }
    }
}

pub(super) fn value_path(value: &Value) -> Option<NodePath> {
    match value {
        Value::Call { name, args } if name == "NodePath" => match args.as_slice() {
            [Value::Str(text)] => Some(NodePath(text.clone())),
            _ => None,
        },
        _ => None,
    }
}

fn path_property(
    props: &Properties,
    key: &str,
    default: &str,
    path: &ResPath,
    line: usize,
    unknowns: &mut Vec<Unknown>,
) -> Option<NodePath> {
    match props.get(key) {
        None => Some(NodePath(default.into())),
        Some(value) => {
            let result = value_path(value);
            if result.is_none() {
                unknown(
                    unknowns,
                    path,
                    line,
                    format!("invalid {key}; default not assumed"),
                );
            }
            result
        }
    }
}

fn synced_properties(
    props: &Properties,
    path: &ResPath,
    line: usize,
    unknowns: &mut Vec<Unknown>,
) -> Vec<SyncedProperty> {
    let mut groups: BTreeMap<usize, BTreeMap<&str, &Value>> = BTreeMap::new();
    for (key, value) in props {
        let Some(rest) = key.strip_prefix("properties/") else {
            continue;
        };
        let Some((index, field)) = rest.split_once('/') else {
            unknown(
                unknowns,
                path,
                line,
                format!("invalid replication field {key}"),
            );
            continue;
        };
        let Ok(index) = index.parse::<usize>() else {
            unknown(
                unknowns,
                path,
                line,
                format!("invalid replication property index in {key}"),
            );
            continue;
        };
        groups.entry(index).or_default().insert(field, value);
    }
    let mut result = vec![];
    for (index, fields) in groups {
        let Some(property_path) = fields.get("path").and_then(|v| value_path(v)) else {
            unknown(
                unknowns,
                path,
                line,
                format!("replication property {index} has no valid NodePath"),
            );
            continue;
        };
        let boolean = |key: &str, default: bool| match fields.get(key) {
            None => Some(default),
            Some(Value::Bool(value)) => Some(*value),
            _ => None,
        };
        let spawn = boolean("spawn", true);
        if spawn.is_none() {
            unknown(
                unknowns,
                path,
                line,
                format!("replication property {index} has invalid spawn flag"),
            );
        }
        let modern = fields.get("replication_mode");
        let sync = fields.get("sync");
        let watch = fields.get("watch");
        let mode = if (modern.is_some() && (sync.is_some() || watch.is_some()))
            || (sync.is_some() && watch.is_some())
        {
            unknown(
                unknowns,
                path,
                line,
                format!(
                    "replication property {index}: legacy/mixed setters may depend on source order, which is not retained"
                ),
            );
            None
        } else if let Some(value) = modern {
            match value {
                Value::Int(0) => Some(SyncMode::Never),
                Value::Int(1) => Some(SyncMode::Always),
                Value::Int(2) => Some(SyncMode::OnChange),
                _ => None,
            }
        } else if sync.is_some() {
            boolean("sync", true).map(|enabled| {
                if enabled {
                    SyncMode::Always
                } else {
                    SyncMode::Never
                }
            })
        } else if watch.is_some() {
            boolean("watch", false).map(|enabled| {
                if enabled {
                    SyncMode::OnChange
                } else {
                    SyncMode::Always
                }
            })
        } else {
            Some(SyncMode::Always)
        };
        if mode.is_none() {
            unknown(
                unknowns,
                path,
                line,
                format!("replication property {index} mode is unresolved; default not assumed"),
            );
        }
        result.push(SyncedProperty {
            path: property_path,
            spawn,
            mode,
        });
    }
    result
}
