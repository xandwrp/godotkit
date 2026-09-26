use super::*;
use std::collections::{BTreeMap, BTreeSet};

/// Narrows observations by method, receiver.method, scene/script, or authored
/// replication node path. All retained indexes are local to this explanation.
pub fn explain(report: &NetReport, query: &str) -> Explanation {
    let query = query.trim();
    let resource_query = query.starts_with("res://");
    let qualified = (!resource_query).then(|| query.rsplit_once('.')).flatten();
    let method = qualified.map_or(query, |(_, method)| method);
    let anchor_matches = |a: &ScriptAnchor| {
        a.scene.as_ref().is_some_and(|p| p.as_str() == query)
            || a.script.as_str() == query
            || a.node.0 == query
            || a.runtime_path.as_ref().is_some_and(|p| p.0 == query)
    };
    let mut anchor_indexes: BTreeSet<usize> = report
        .anchors
        .iter()
        .enumerate()
        .filter(|(_, a)| anchor_matches(a))
        .map(|(i, _)| i)
        .collect();
    let selected_scripts: BTreeSet<_> = anchor_indexes
        .iter()
        .map(|&i| &report.anchors[i].script)
        .collect();
    let mut endpoint_indexes: BTreeSet<usize> = report
        .endpoints
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            if resource_query {
                return e.script.as_str() == query || selected_scripts.contains(&e.script);
            }
            if selected_scripts.contains(&e.script) {
                return true;
            }
            if e.method != method {
                return false;
            }
            qualified.is_none_or(|(receiver, _)| {
                report
                    .anchors
                    .iter()
                    .any(|a| a.script == e.script && node_label(&a.node.0, receiver))
            })
        })
        .map(|(i, _)| i)
        .collect();
    let mut calls: Vec<RpcCall> = report
        .calls
        .iter()
        .filter(|c| {
            if resource_query {
                return c.location.path.as_str() == query
                    || selected_scripts.contains(&c.location.path)
                    || c.candidates.iter().any(|i| endpoint_indexes.contains(i));
            }
            if selected_scripts.contains(&c.location.path)
                || c.candidates.iter().any(|i| endpoint_indexes.contains(i))
            {
                return true;
            }
            c.method.as_deref() == Some(method)
                && qualified.is_none_or(|(receiver, _)| {
                    c.receiver.as_deref().is_some_and(|r| {
                        r == receiver
                            || source::receiver_path(r)
                                .is_some_and(|path| node_label(&path, receiver))
                    })
                })
        })
        .cloned()
        .collect();
    for call in &calls {
        endpoint_indexes.extend(
            call.candidates
                .iter()
                .copied()
                .filter(|&i| i < report.endpoints.len()),
        );
    }
    let endpoint_map: BTreeMap<_, _> = endpoint_indexes
        .iter()
        .enumerate()
        .map(|(new, &old)| (old, new))
        .collect();
    for call in &mut calls {
        call.candidates = call
            .candidates
            .iter()
            .filter_map(|i| endpoint_map.get(i).copied())
            .collect();
    }
    let endpoints: Vec<_> = endpoint_indexes
        .iter()
        .map(|&i| report.endpoints[i].clone())
        .collect();
    let scripts: BTreeSet<_> = endpoints
        .iter()
        .map(|e| &e.script)
        .chain(calls.iter().map(|c| &c.location.path))
        .collect();
    anchor_indexes.extend(
        report
            .anchors
            .iter()
            .enumerate()
            .filter(|(_, a)| scripts.contains(&a.script))
            .map(|(i, _)| i),
    );
    let mut anchors: Vec<_> = anchor_indexes
        .iter()
        .map(|&i| report.anchors[i].clone())
        .collect();
    let scene_nodes: BTreeSet<_> = anchors
        .iter()
        .filter_map(|a| a.scene.as_ref().map(|s| (s.clone(), a.node.clone())))
        .collect();
    let spawners: Vec<_> = report
        .spawners
        .iter()
        .filter(|s| {
            s.scene.as_str() == query
                || s.node.0 == query
                || scene_nodes.contains(&(s.scene.clone(), s.node.clone()))
                || s.spawn_path.as_ref().is_some_and(|root| {
                    related_root(&s.node, root)
                        .is_some_and(|node| scene_nodes.contains(&(s.scene.clone(), node)))
                })
                || s.auto_spawn_list
                    .iter()
                    .any(|spawned| scene_nodes.iter().any(|(scene, _)| scene == spawned))
        })
        .cloned()
        .collect();
    let synchronizers: Vec<_> = report
        .synchronizers
        .iter()
        .filter(|s| {
            s.scene.as_str() == query
                || s.node.0 == query
                || scene_nodes.contains(&(s.scene.clone(), s.node.clone()))
                || s.root_path.as_ref().is_some_and(|root| {
                    related_root(&s.node, root)
                        .is_some_and(|node| scene_nodes.contains(&(s.scene.clone(), node)))
                })
        })
        .cloned()
        .collect();
    let context_indexes: BTreeSet<_> = anchors
        .iter()
        .flat_map(|a| a.context_candidates.iter().copied())
        .filter(|&i| i < report.contexts.len())
        .collect();
    let context_map: BTreeMap<_, _> = context_indexes
        .iter()
        .enumerate()
        .map(|(new, &old)| (old, new))
        .collect();
    for anchor in &mut anchors {
        anchor.context_candidates = anchor
            .context_candidates
            .iter()
            .filter_map(|i| context_map.get(i).copied())
            .collect();
    }
    let contexts = context_indexes
        .iter()
        .map(|&i| report.contexts[i].clone())
        .collect();
    let authority_uses: Vec<_> = report
        .authority_uses
        .iter()
        .filter(|a| {
            a.location.path.as_str() == query
                || (resource_query && selected_scripts.contains(&a.location.path))
        })
        .cloned()
        .collect();
    let locations: BTreeSet<_> = calls
        .iter()
        .map(|c| c.location.clone())
        .chain(endpoints.iter().map(|e| e.location.clone()))
        .chain(spawners.iter().map(|s| SourceLocation {
            path: s.scene.clone(),
            line: s.line,
        }))
        .chain(synchronizers.iter().map(|s| SourceLocation {
            path: s.scene.clone(),
            line: s.line,
        }))
        .collect();
    let unknowns: Vec<_> = report
        .unknowns
        .iter()
        .filter(|u| {
            u.location.as_ref().is_some_and(|l| {
                locations.contains(l)
                    || l.path.as_str() == query
                    || (resource_query && selected_scripts.contains(&l.path))
            })
        })
        .cloned()
        .collect();
    let matched = !(endpoints.is_empty()
        && calls.is_empty()
        && anchors.is_empty()
        && spawners.is_empty()
        && synchronizers.is_empty()
        && authority_uses.is_empty()
        && unknowns.is_empty());
    let mut notes = vec!["Static source candidates, not proof of runtime compatibility. Endpoint and context indexes are local to this explanation.".into()];
    notes.extend(report.coverage.limitations.clone());
    if !matched {
        notes.push(format!("no observations match {query:?}"));
    }
    Explanation {
        schema_version: NET_REPORT_SCHEMA_VERSION,
        query: query.into(),
        matched,
        contexts,
        endpoints,
        calls,
        anchors,
        spawners,
        synchronizers,
        authority_uses,
        unknowns,
        notes,
    }
}

fn node_label(path: &str, query: &str) -> bool {
    path == query || path.rsplit('/').next() == Some(query)
}

fn related_root(node: &NodePath, root: &NodePath) -> Option<NodePath> {
    if root.is_absolute() {
        return None;
    }
    let mut parts: Vec<_> = node.segments().filter(|s| *s != ".").collect();
    for part in root.segments() {
        match part {
            "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    Some(NodePath(if parts.is_empty() {
        ".".into()
    } else {
        parts.join("/")
    }))
}
