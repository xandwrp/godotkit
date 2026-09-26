use super::*;
use crate::scene::{FileKind, ResourceRef};
use std::collections::BTreeSet;

/// Pure analysis of already loaded sources. No file access or engine state.
pub fn analyze(input: &NetInput<'_>) -> NetReport {
    let mut report = NetReport {
        schema_version: NET_REPORT_SCHEMA_VERSION,
        coverage: Coverage {
            scripts_scanned: input.scripts.len(),
            scenes_scanned: input.scenes.iter().filter(|(_,s)| s.kind == FileKind::Scene).count(),
            resources_scanned: input.scenes.iter().filter(|(_,s)| s.kind == FileKind::Resource).count(),
            limitations: vec![
                "Source observations only: candidates do not prove runtime RPC compatibility, delivery, authority or peer path equality.".into(),
                "Direct authored scene nodes only; scene instances, inherited scenes/scripts, runtime node creation and script/RPC configuration are not expanded.".into(),
                "GDScript and text scenes/resources only; C#, binary resources and GDExtension behavior are not inspected.".into(),
                "External replication config types use authored ext_resource hints; resource header types and setter order are not retained by the shared parser.".into(),
                "Replication reports spawn and mode, not synchronization intervals or visibility filters. Scene placement and observed context activation remain runtime-dependent.".into(),
            ],
        },
        endpoints: vec![], calls: vec![], anchors: vec![], contexts: vec![], spawners: vec![], synchronizers: vec![],
        authority_uses: vec![], autoloads: vec![], unknowns: input.unknowns.to_vec(),
    };
    for script in input.scripts {
        report.endpoints.extend(script.endpoints.clone());
        report.calls.extend(script.calls.clone());
        report.authority_uses.extend(script.authority_uses.clone());
        report.contexts.extend(script.contexts.clone());
        report.unknowns.extend(script.unknowns.clone());
    }
    report.endpoints.sort_by(|a, b| {
        (&a.script, &a.class, &a.method, &a.location).cmp(&(
            &b.script,
            &b.class,
            &b.method,
            &b.location,
        ))
    });
    report.calls.sort_by(|a, b| {
        (&a.location, &a.class, &a.expression).cmp(&(&b.location, &b.class, &b.expression))
    });
    report
        .contexts
        .sort_by(|a, b| (&a.location, &a.api, &a.root).cmp(&(&b.location, &b.api, &b.root)));
    report
        .authority_uses
        .sort_by(|a, b| (&a.location, &a.call).cmp(&(&b.location, &b.call)));
    anchors(input, &mut report);
    super::replication::collect(input, &mut report);
    report
        .spawners
        .sort_by(|a, b| (&a.scene, &a.node, a.line).cmp(&(&b.scene, &b.node, b.line)));
    report
        .synchronizers
        .sort_by(|a, b| (&a.scene, &a.node, a.line).cmp(&(&b.scene, &b.node, b.line)));
    for index in 0..report.calls.len() {
        let mut call = report.calls[index].clone();
        let targets = targets(input, &report, &call);
        if targets.len() > 1 {
            unknown(
                &mut report.unknowns,
                &call.location.path,
                call.location.line,
                "receiver has multiple authored scene/instance targets; all source candidates are retained",
            );
        }
        if call.form == CallForm::AmbiguousRpc
            && !targets.is_empty()
            && let Some((form, method)) = source::node_call_method(&call.expression)
        {
            call.form = form;
            call.method = method;
        }
        if let Some(method) = &call.method {
            call.candidates = report
                .endpoints
                .iter()
                .enumerate()
                .filter(|(_, endpoint)| {
                    endpoint.method == *method
                        && targets.contains(&(endpoint.script.clone(), endpoint.class.clone()))
                })
                .map(|(i, _)| i)
                .collect();
        }
        if call.candidates.is_empty() {
            unknown(
                &mut report.unknowns,
                &call.location.path,
                call.location.line,
                "no source endpoint resolved for this call; dynamic receivers, missing or inherited endpoints remain unknown",
            );
        }
        report.calls[index] = call;
    }
    for autoload in input.autoloads {
        let networked = autoload.path.as_ref().is_some_and(|path| {
            let scripts: BTreeSet<_> = report
                .anchors
                .iter()
                .filter(|a| a.scene.as_ref() == Some(path))
                .map(|a| &a.script)
                .collect();
            report
                .endpoints
                .iter()
                .any(|e| &e.script == path || scripts.contains(&e.script))
                || report
                    .calls
                    .iter()
                    .any(|c| &c.location.path == path || scripts.contains(&c.location.path))
                || report
                    .authority_uses
                    .iter()
                    .any(|c| &c.location.path == path || scripts.contains(&c.location.path))
                || report
                    .contexts
                    .iter()
                    .any(|c| &c.location.path == path || scripts.contains(&c.location.path))
                || report.spawners.iter().any(|s| &s.scene == path)
                || report.synchronizers.iter().any(|s| &s.scene == path)
        });
        report.autoloads.push(NetAutoload {
            autoload: autoload.clone(),
            networked,
        });
        if !autoload.exists || autoload.path.is_none() {
            unknown(
                &mut report.unknowns,
                &ResPath::parse("res://project.godot").unwrap(),
                1,
                format!(
                    "autoload {} has a missing or unresolved target",
                    autoload.name
                ),
            );
        }
    }
    report.autoloads.sort_by_key(|a| a.autoload.order);
    report.unknowns.sort();
    report.unknowns.dedup();
    report
}

fn anchors(input: &NetInput<'_>, report: &mut NetReport) {
    for (path, scene) in input.scenes {
        if scene.kind != FileKind::Scene {
            continue;
        }
        for node in &scene.nodes {
            if node.instance.is_some()
                || node.instance_placeholder.is_some()
                || node.type_name.is_none()
            {
                unknown(
                    &mut report.unknowns,
                    path,
                    node.line,
                    format!(
                        "{}: instance/inherited node contents are not expanded",
                        node.path().0
                    ),
                );
            }
            let Some(reference) = &node.script else {
                if node
                    .properties
                    .get("script")
                    .is_some_and(|v| v != &crate::scene::Value::Null)
                {
                    unknown(
                        &mut report.unknowns,
                        path,
                        node.line,
                        "unsupported node script property",
                    );
                }
                continue;
            };
            let target = match reference {
                ResourceRef::Ext(id) => scene.ext_resource(id).map(|ext| {
                    ext.uid
                        .as_ref()
                        .and_then(|u| input.uids.resolve(u))
                        .unwrap_or(&ext.path)
                        .clone()
                }),
                _ => None,
            };
            let Some(script) = target else {
                unknown(
                    &mut report.unknowns,
                    path,
                    node.line,
                    "inline or missing node script reference is not resolved",
                );
                continue;
            };
            if !input.scripts.iter().any(|s| s.script == script) {
                unknown(
                    &mut report.unknowns,
                    path,
                    node.line,
                    format!("node script {script} was not analyzed"),
                );
            }
            report.anchors.push(ScriptAnchor {
                script,
                scene: Some(path.clone()),
                node: node.path(),
                runtime_path: None,
                context_candidates: vec![],
            });
        }
    }
    let direct = report.anchors.clone();
    for autoload in input.autoloads {
        let Some(path) = &autoload.path else { continue };
        let root = NodePath(format!("/root/{}", autoload.name));
        if matches!(autoload.kind, crate::autoload::AutoloadKind::Script) {
            report.anchors.push(ScriptAnchor {
                script: path.clone(),
                scene: None,
                node: root.clone(),
                runtime_path: Some(root),
                context_candidates: vec![],
            });
        } else {
            for anchor in direct.iter().filter(|a| a.scene.as_ref() == Some(path)) {
                let runtime = if anchor.node.0 == "." {
                    root.clone()
                } else {
                    root.join(&anchor.node.0)
                };
                report.anchors.push(ScriptAnchor {
                    runtime_path: Some(runtime),
                    ..anchor.clone()
                });
            }
        }
    }
    for anchor in &mut report.anchors {
        if let Some(path) = &anchor.runtime_path {
            anchor.context_candidates = report
                .contexts
                .iter()
                .enumerate()
                .filter(|(_, c)| {
                    c.root
                        .as_ref()
                        .is_some_and(|root| root.0.is_empty() || path_below(&path.0, &root.0))
                })
                .map(|(index, _)| index)
                .collect();
        }
    }
    report.anchors.sort_by(|a, b| {
        (&a.scene, &a.node, &a.script, &a.runtime_path).cmp(&(
            &b.scene,
            &b.node,
            &b.script,
            &b.runtime_path,
        ))
    });
}

fn path_below(path: &str, root: &str) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn targets(
    input: &NetInput<'_>,
    report: &NetReport,
    call: &RpcCall,
) -> BTreeSet<(ResPath, Option<String>)> {
    let mut targets = BTreeSet::new();
    let receiver = call.receiver.as_deref().unwrap_or("self");
    if receiver == "self" {
        if call.form != CallForm::AmbiguousRpc {
            targets.insert((call.location.path.clone(), call.class.clone()));
        }
        return targets;
    }
    if call.receiver_is_local {
        return targets;
    }
    let mut node_path = source::receiver_path(receiver);
    if node_path.is_none()
        && let (Some(script), Some(name)) = (
            input
                .scripts
                .iter()
                .find(|s| s.script == call.location.path),
            source::receiver_name(receiver),
        )
    {
        node_path = script
            .bindings
            .iter()
            .find(|b| b.class == call.class && b.name == name)
            .map(|b| b.path.0.clone());
    }
    // A field alias takes precedence over a same-named global autoload.
    // Non-singleton autoloads are reachable only through explicit /root paths.
    for autoload in input
        .autoloads
        .iter()
        .filter(|a| node_path.is_none() && a.singleton && a.name == receiver)
    {
        if let Some(path) = &autoload.path {
            if autoload.kind == crate::autoload::AutoloadKind::Script {
                targets.insert((path.clone(), None));
            } else {
                for a in report
                    .anchors
                    .iter()
                    .filter(|a| a.scene.as_ref() == Some(path) && a.node.0 == ".")
                {
                    targets.insert((a.script.clone(), None));
                }
            }
        }
    }
    if let Some(path) = node_path {
        if path.starts_with('/') {
            for a in report
                .anchors
                .iter()
                .filter(|a| a.runtime_path.as_ref().is_some_and(|p| p.0 == path))
            {
                targets.insert((a.script.clone(), None));
            }
        } else if call.class.is_none() {
            for owner in report
                .anchors
                .iter()
                .filter(|a| a.script == call.location.path)
            {
                let Some(scene_path) = &owner.scene else {
                    continue;
                };
                let Some((_, scene)) = input.scenes.iter().find(|(p, _)| p == scene_path) else {
                    continue;
                };
                let Some(node) = resolve_path(scene, &owner.node, &path) else {
                    continue;
                };
                for anchor in report
                    .anchors
                    .iter()
                    .filter(|a| a.scene.as_ref() == Some(scene_path) && a.node == node)
                {
                    targets.insert((anchor.script.clone(), None));
                }
            }
        }
    }
    targets
}

fn resolve_path(scene: &SceneFile, from: &NodePath, path: &str) -> Option<NodePath> {
    let mut segments: Vec<String> = from
        .segments()
        .filter(|s| *s != ".")
        .map(str::to_owned)
        .collect();
    for (i, segment) in path.split('/').filter(|s| !s.is_empty()).enumerate() {
        match segment {
            "." => {}
            ".." => {
                segments.pop()?;
            }
            unique if unique.starts_with('%') => {
                if i != 0 {
                    return None;
                }
                let mut matches = scene
                    .nodes
                    .iter()
                    .filter(|n| n.unique_name_in_owner && n.name == unique[1..]);
                let node = matches.next()?;
                if matches.next().is_some() {
                    return None;
                }
                segments = node
                    .path()
                    .segments()
                    .filter(|s| *s != ".")
                    .map(str::to_owned)
                    .collect();
            }
            name => segments.push(name.into()),
        }
        // get_node traverses in order: Missing/../Target must not become a
        // confidently resolved Target merely by normalizing the text.
        let intermediate = NodePath(if segments.is_empty() {
            ".".into()
        } else {
            segments.join("/")
        });
        scene.node(&intermediate)?;
    }
    let path = NodePath(if segments.is_empty() {
        ".".into()
    } else {
        segments.join("/")
    });
    scene.node(&path).map(|_| path)
}
