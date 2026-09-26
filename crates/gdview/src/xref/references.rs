//! The inbound references behind [`super::references_to`].

use super::analysis::resolve_script_path;
use super::{Location, ProjectGraph, Reference, ReferenceKind};
use crate::declarations::ResourceUseKind;
use crate::respath::{NodePath, ResPath, Uid};
use crate::scene::{ExtResource, Resolved, ResourceRef, SceneFile, Value};

pub(super) fn find(graph: &ProjectGraph<'_>, query: &ResPath) -> crate::Result<Vec<Reference>> {
    let mut search = Search {
        graph,
        query,
        found: Vec::new(),
    };
    for (path, scene) in &graph.scenes {
        search.scene(path, scene);
    }
    for script in &graph.declarations.scripts {
        for used in &script.resource_uses {
            let kind = match used.kind {
                ResourceUseKind::Preload => ReferenceKind::Preload,
                ResourceUseKind::Load => ReferenceKind::Load,
                ResourceUseKind::Extends => ReferenceKind::Extends,
                ResourceUseKind::String => ReferenceKind::String,
            };
            let at = Location {
                path: script.declaration.path.clone(),
                line: used.line,
            };
            if used.path.starts_with("uid://") {
                search.by_uid(&Uid(used.path.clone()), at, kind, None, None);
            } else if let Some(target) = resolve_script_path(&script.declaration.path, &used.path) {
                search.push(target, at, kind, None, None, false);
            }
        }
    }
    search.project_settings()?;
    let mut found = search.found;
    found.sort_by(|a, b| {
        (&a.at, a.kind, &a.target, &a.node, &a.key)
            .cmp(&(&b.at, b.kind, &b.target, &b.node, &b.key))
    });
    found.dedup();
    Ok(found)
}

struct Search<'g, 'a> {
    graph: &'g ProjectGraph<'a>,
    query: &'g ResPath,
    found: Vec<Reference>,
}

impl Search<'_, '_> {
    fn push(
        &mut self,
        target: ResPath,
        at: Location,
        kind: ReferenceKind,
        node: Option<NodePath>,
        key: Option<String>,
        by_uid: bool,
    ) {
        if target.starts_with(self.query) {
            self.found.push(Reference {
                at,
                kind,
                target,
                node,
                key,
                by_uid,
            });
        }
    }

    /// A `uid://` that resolves; one that does not points at nothing to report.
    fn by_uid(
        &mut self,
        uid: &Uid,
        at: Location,
        kind: ReferenceKind,
        node: Option<NodePath>,
        key: Option<String>,
    ) {
        if let Some(target) = self.graph.uids.resolve(uid) {
            self.push(target.clone(), at, kind, node, key, true);
        }
    }

    /// Where an ext_resource points, as the engine loads it: its uid when that
    /// resolves, else its path.
    fn ext_target(&self, ext: &ExtResource) -> (ResPath, bool) {
        match ext
            .uid
            .as_ref()
            .and_then(|uid| self.graph.uids.resolve(uid))
        {
            Some(target) => (target.clone(), true),
            None => (ext.path.clone(), false),
        }
    }

    /// A `res://` or `uid://` string, as written in a value.
    fn string(
        &mut self,
        text: &str,
        at: Location,
        kind: ReferenceKind,
        node: Option<NodePath>,
        key: Option<String>,
    ) {
        if text.starts_with("uid://") {
            self.by_uid(&Uid(text.to_owned()), at, kind, node, key);
        } else if text.starts_with("res://")
            && let Ok(target) = ResPath::parse(text)
        {
            self.push(target, at, kind, node, key, false);
        }
    }

    fn scene(&mut self, path: &ResPath, scene: &SceneFile) {
        let at = |line| Location {
            path: path.clone(),
            line,
        };
        for ext in &scene.ext_resources {
            let (target, by_uid) = self.ext_target(ext);
            self.push(
                target,
                at(ext.line),
                ReferenceKind::ExtResource,
                None,
                None,
                by_uid,
            );
        }
        let through_ext = |reference: &Option<ResourceRef>| match reference
            .as_ref()
            .and_then(|reference| scene.resolve(reference))
        {
            Some(Resolved::Ext(ext)) => Some(ext),
            _ => None,
        };
        for (index, node) in scene.nodes.iter().enumerate() {
            let node_path = Some(node.path());
            if let Some(ext) = through_ext(&node.script) {
                let (target, by_uid) = self.ext_target(ext);
                let kind = ReferenceKind::Script;
                self.push(target, at(node.line), kind, node_path.clone(), None, by_uid);
            }
            if let Some(ext) = through_ext(&node.instance) {
                let (target, by_uid) = self.ext_target(ext);
                let kind = match index {
                    0 => ReferenceKind::Inherits,
                    _ => ReferenceKind::Instance,
                };
                self.push(target, at(node.line), kind, node_path.clone(), None, by_uid);
            }
            if let Some(placeholder) = &node.instance_placeholder {
                let kind = ReferenceKind::Placeholder;
                self.push(
                    placeholder.clone(),
                    at(node.line),
                    kind,
                    node_path.clone(),
                    None,
                    false,
                );
            }
            for (name, value) in &node.properties {
                if name != "script" {
                    self.strings(name, value, at(node.line), node_path.clone());
                }
            }
        }
        if let Some(resource) = &scene.resource {
            let line = scene.resource_line.unwrap_or(1);
            let script = resource.get("script").and_then(Value::as_resource_ref);
            if let Some(ext) = through_ext(&script) {
                let (target, by_uid) = self.ext_target(ext);
                self.push(target, at(line), ReferenceKind::Script, None, None, by_uid);
            }
            for (name, value) in resource {
                if name != "script" {
                    self.strings(name, value, at(line), None);
                }
            }
        }
        for sub in &scene.sub_resources {
            for (name, value) in &sub.properties {
                self.strings(name, value, at(sub.line), None);
            }
        }
    }

    /// Every `res://`/`uid://` string inside property `name`'s value. The
    /// location is the node's or sub-resource's header line.
    fn strings(&mut self, name: &str, value: &Value, at: Location, node: Option<NodePath>) {
        let mut strings = Vec::new();
        collect_strings(value, &mut strings);
        for text in strings {
            let kind = ReferenceKind::Property;
            self.string(text, at.clone(), kind, node.clone(), Some(name.to_owned()));
        }
    }

    fn project_settings(&mut self) -> crate::Result<()> {
        let settings = self.graph.project.settings()?;
        let at = |line| Location {
            path: ResPath::from_relative("project.godot").expect("a valid path"),
            line,
        };
        for (section, entries) in &settings.sections {
            for (index, entry) in entries.iter().enumerate() {
                // A repeated key is read with its last value.
                if entries[index + 1..]
                    .iter()
                    .any(|later| later.key == entry.key)
                {
                    continue;
                }
                let setting = match section.as_str() {
                    "" => entry.key.clone(),
                    section => format!("{section}/{}", entry.key),
                };
                let kind = match setting.as_str() {
                    "application/run/main_scene" => ReferenceKind::MainScene,
                    _ if section == "autoload" => ReferenceKind::Autoload,
                    _ => ReferenceKind::ProjectSetting,
                };
                let Ok(value) = crate::scene::parse_value(&entry.value) else {
                    continue;
                };
                let mut strings = Vec::new();
                collect_strings(&value, &mut strings);
                for text in strings {
                    // Autoloads mark singletons with a leading `*`.
                    let text = match kind {
                        ReferenceKind::Autoload => text.strip_prefix('*').unwrap_or(text),
                        _ => text,
                    };
                    self.string(text, at(entry.line), kind, None, Some(setting.clone()));
                }
            }
        }
        Ok(())
    }
}

fn collect_strings<'v>(value: &'v Value, out: &mut Vec<&'v str>) {
    match value {
        Value::Str(text) | Value::StringName(text) => out.push(text),
        Value::Array(items) | Value::Call { args: items, .. } => {
            for item in items {
                collect_strings(item, out);
            }
        }
        Value::Dict(entries) => {
            for (key, value) in entries {
                collect_strings(key, out);
                collect_strings(value, out);
            }
        }
        Value::Null | Value::Bool(_) | Value::Int(_) | Value::Float(_) => {}
    }
}
