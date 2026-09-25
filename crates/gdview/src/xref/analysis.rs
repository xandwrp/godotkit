//! The checks behind [`super::analyze`]. See the module docs for what is
//! reported and why each check errs toward silence.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::{Finding, FindingKind, Location, ProjectGraph};
use crate::declarations::{IndexedScript, MemberDeclaration, MemberKind, ResourceUseKind};
use crate::respath::{NodePath, ResPath, Uid};
use crate::scene::{ExtResource, FileKind, Resolved, ResourceRef, SceneFile, Value};

pub(super) fn run(graph: &ProjectGraph<'_>) -> Vec<Finding> {
    let analysis = Analysis {
        graph,
        lookups: RefCell::new(HashMap::new()),
    };
    let mut findings = graph.unparseable.clone();
    for (path, scene) in &graph.scenes {
        analysis.scene_references(path, scene, &mut findings);
        if scene.kind == FileKind::Scene {
            analysis.connections(path, scene, &mut findings);
        }
    }
    for script in &graph.declarations.scripts {
        analysis.script_references(script, &mut findings);
    }
    analysis.onready_node_paths(&mut findings);
    analysis.duplicates(&mut findings);
    findings.sort_by(|a, b| {
        (&a.at, a.kind as u8, &a.target, &a.message).cmp(&(
            &b.at,
            b.kind as u8,
            &b.target,
            &b.message,
        ))
    });
    findings.dedup();
    findings
}

struct Analysis<'g, 'a> {
    graph: &'g ProjectGraph<'a>,
    lookups: RefCell<HashMap<(ResPath, Vec<String>), Lookup>>,
}

/// What the effective scene tree says about one node path.
#[derive(Clone, Debug, PartialEq)]
enum Lookup {
    Found(NodeFacts),
    NotFound,
    /// Depends on a scene gdview could not read, or a cycle.
    Unknown,
}

#[derive(Clone, Debug, PartialEq)]
struct NodeFacts {
    script: ScriptFact,
    /// The scene this node instances, if any.
    instance: Option<ResPath>,
}

#[derive(Clone, Debug, PartialEq)]
enum ScriptFact {
    None,
    Script(ResPath),
    Unknown,
}

/// A script and the project scripts it extends, nearest first.
struct Chain<'g> {
    scripts: Vec<&'g IndexedScript>,
    /// Every link resolved and parsed cleanly. When false, absence proves nothing.
    complete: bool,
}

impl<'g> Chain<'g> {
    fn member(&self, kind: MemberKind, name: &str) -> Option<&'g MemberDeclaration> {
        self.scripts
            .iter()
            .flat_map(|script| &script.declaration.members)
            .find(|member| member.kind == kind && member.name == name)
    }
    fn member_names(&self, kind: MemberKind) -> impl Iterator<Item = &'g str> {
        self.scripts
            .iter()
            .flat_map(|script| &script.declaration.members)
            .filter(move |member| member.kind == kind)
            .map(|member| member.name.as_str())
    }
}

const MAX_DEPTH: usize = 32;

impl<'g> Analysis<'g, '_> {
    fn exists(&self, path: &ResPath) -> bool {
        self.graph.project.exists(path)
    }

    /// Where an ext_resource actually points: its uid when that resolves, else its path.
    fn ext_target(&self, ext: &ExtResource) -> ResPath {
        ext.uid
            .as_ref()
            .and_then(|uid| self.graph.uids.resolve(uid))
            .cloned()
            .unwrap_or_else(|| ext.path.clone())
    }

    fn ext_ref_target(&self, scene: &SceneFile, reference: &ResourceRef) -> Option<ResPath> {
        match scene.resolve(reference)? {
            Resolved::Ext(ext) => Some(self.ext_target(ext)),
            Resolved::Sub(_) => None,
        }
    }

    // ---- scene files -------------------------------------------------------

    fn scene_references(&self, path: &ResPath, scene: &SceneFile, findings: &mut Vec<Finding>) {
        for ext in &scene.ext_resources {
            let uid_resolves = ext
                .uid
                .as_ref()
                .is_some_and(|uid| self.graph.uids.resolve(uid).is_some());
            if uid_resolves || self.exists(&ext.path) {
                continue;
            }
            let message = match &ext.uid {
                Some(uid) => format!(
                    "ext_resource {} points at {}, which does not exist, and its {} resolves to no file",
                    ext.id, ext.path, uid.0
                ),
                None => format!(
                    "ext_resource {} points at {}, which does not exist",
                    ext.id, ext.path
                ),
            };
            findings.push(Finding {
                kind: FindingKind::MissingResource,
                at: Location {
                    path: path.clone(),
                    line: ext.line,
                },
                message,
                target: ext.path.to_string(),
                suggestions: self.suggest_files(&ext.path),
            });
        }
        for node in &scene.nodes {
            for (what, reference) in [("instance", &node.instance), ("script", &node.script)] {
                if let Some(reference) = reference
                    && scene.resolve(reference).is_none()
                {
                    let (kind, id) = match reference {
                        ResourceRef::Ext(id) => ("ExtResource", id),
                        ResourceRef::Sub(id) => ("SubResource", id),
                    };
                    findings.push(Finding {
                        kind: FindingKind::MissingResource,
                        at: Location { path: path.clone(), line: node.line },
                        message: format!("node {} {what} refers to {kind}(\"{id}\"), which this file does not declare", node.path().0),
                        target: id.clone(),
                        suggestions: Vec::new(),
                    });
                }
            }
            if let Some(placeholder) = &node.instance_placeholder
                && !self.exists(placeholder)
            {
                findings.push(Finding {
                    kind: FindingKind::MissingResource,
                    at: Location {
                        path: path.clone(),
                        line: node.line,
                    },
                    message: format!(
                        "node {} is a placeholder for {placeholder}, which does not exist",
                        node.path().0
                    ),
                    target: placeholder.to_string(),
                    suggestions: self.suggest_files(placeholder),
                });
            }
        }
    }

    fn connections(&self, path: &ResPath, scene: &SceneFile, findings: &mut Vec<Finding>) {
        for connection in &scene.connections {
            let at = Location {
                path: path.clone(),
                line: connection.line,
            };
            let describe = format!(
                "signal {} from {} to {}",
                connection.signal, connection.from.0, connection.to.0
            );
            let mut missing_node = |which: &str, node: &NodePath| {
                findings.push(Finding {
                    kind: FindingKind::MissingNode,
                    at: at.clone(),
                    message: format!("{describe}: {which} node {} is not in the scene", node.0),
                    target: node.0.clone(),
                    suggestions: Vec::new(),
                });
            };
            let source = self.lookup(path, &connection.from);
            if source == Lookup::NotFound {
                missing_node("source", &connection.from);
            }
            let target = match self.lookup(path, &connection.to) {
                Lookup::Found(facts) => facts,
                Lookup::NotFound => {
                    missing_node("target", &connection.to);
                    continue;
                }
                Lookup::Unknown => continue,
            };
            let ScriptFact::Script(script) = &target.script else {
                continue;
            };
            let chain = self.chain(script);
            let Some(handler) = chain.member(MemberKind::Func, &connection.method) else {
                if chain.complete && connection.method.starts_with('_') {
                    findings.push(Finding {
                        kind: FindingKind::MissingMethod,
                        at: at.clone(),
                        message: format!("{describe}: method {} is not declared in {script} or the scripts it extends", connection.method),
                        target: connection.method.clone(),
                        suggestions: similar(&connection.method, chain.member_names(MemberKind::Func)),
                    });
                }
                continue;
            };
            // Arity: only for project-declared signals and plain connection flags
            // (deferred, persist, one-shot, reference-counted).
            let Lookup::Found(source) = source else {
                continue;
            };
            let ScriptFact::Script(source_script) = &source.script else {
                continue;
            };
            if connection.flags.unwrap_or(0) & !0xF != 0 {
                continue;
            }
            let Some(signal) = self
                .chain(source_script)
                .member(MemberKind::Signal, &connection.signal)
            else {
                continue;
            };
            let emitted = signal.parameters.len() as i64 - connection.unbinds as i64
                + connection.binds.len() as i64;
            let (required, max) = handler.arity();
            let fits = emitted >= required as i64 && max.is_none_or(|max| emitted <= max as i64);
            if emitted >= 0 && !fits {
                let accepts = match max {
                    Some(max) if max == required => format!("{required}"),
                    Some(max) => format!("{required} to {max}"),
                    None => format!("at least {required}"),
                };
                findings.push(Finding {
                    kind: FindingKind::MethodArity,
                    at,
                    message: format!(
                        "{describe}: the call passes {emitted} argument(s) ({} from the signal, {} bound, {} unbound) but {} takes {accepts}",
                        signal.parameters.len(),
                        connection.binds.len(),
                        connection.unbinds,
                        connection.method
                    ),
                    target: connection.method.clone(),
                    suggestions: Vec::new(),
                });
            }
        }
    }

    // ---- scripts -----------------------------------------------------------

    fn script_references(&self, script: &IndexedScript, findings: &mut Vec<Finding>) {
        let path = &script.declaration.path;
        for used in &script.resource_uses {
            let at = Location {
                path: path.clone(),
                line: used.line,
            };
            let kind = match used.kind {
                ResourceUseKind::Extends => FindingKind::MissingBaseScript,
                _ => FindingKind::MissingResource,
            };
            let verb = match used.kind {
                ResourceUseKind::Extends => "extends",
                ResourceUseKind::Preload => "preload",
                ResourceUseKind::Load => "load",
            };
            if used.path.starts_with("uid://") {
                if self.graph.uids.resolve(&Uid(used.path.clone())).is_none() {
                    findings.push(Finding {
                        kind: FindingKind::UnresolvedUid,
                        at,
                        message: format!(
                            "{verb} of {}, which no file in the project claims",
                            used.path
                        ),
                        target: used.path.clone(),
                        suggestions: Vec::new(),
                    });
                }
                continue;
            }
            let Some(target) = resolve_script_path(path, &used.path) else {
                continue;
            };
            if !self.exists(&target) {
                findings.push(Finding {
                    kind,
                    at,
                    message: format!("{verb} of {target}, which does not exist"),
                    target: target.to_string(),
                    suggestions: self.suggest_files(&target),
                });
            }
        }
    }

    /// `@onready` node paths that resolve in none of the scenes the script is attached to.
    fn onready_node_paths(&self, findings: &mut Vec<Finding>) {
        let owners = self.owners();
        for script in &self.graph.declarations.scripts {
            let path = &script.declaration.path;
            let Some(owners) = owners.get(path) else {
                continue;
            };
            for used in script.node_path_uses.iter().filter(|used| used.onready) {
                let mut missing_everywhere = true;
                let mut suggestions = BTreeSet::new();
                for (scene, node) in owners {
                    match self.resolve_from(scene, node, &used.path) {
                        Resolution::Found | Resolution::Unknown => {
                            missing_everywhere = false;
                            break;
                        }
                        Resolution::Missing { parent, name } => {
                            let parent: Vec<String> = parent
                                .segments()
                                .filter(|s| *s != ".")
                                .map(str::to_owned)
                                .collect();
                            let siblings = self.child_names(scene, &parent, 0);
                            suggestions.extend(similar(&name, siblings.iter().map(String::as_str)));
                        }
                        Resolution::MissingUnique { name } => {
                            let names = self.unique_names(scene);
                            let close = similar(&name, names.iter().map(String::as_str));
                            suggestions.extend(close.into_iter().map(|name| format!("%{name}")));
                        }
                    }
                }
                if missing_everywhere {
                    let scenes: BTreeSet<_> =
                        owners.iter().map(|(scene, _)| scene.as_str()).collect();
                    let scenes: Vec<_> = scenes.into_iter().collect();
                    findings.push(Finding {
                        kind: FindingKind::MissingNode,
                        at: Location { path: path.clone(), line: used.line },
                        message: format!(
                            "@onready node path \"{}\" does not exist in any scene that attaches this script ({})",
                            used.path,
                            scenes.join(", ")
                        ),
                        target: used.path.clone(),
                        suggestions: suggestions.into_iter().take(3).collect(),
                    });
                }
            }
        }
    }

    fn duplicates(&self, findings: &mut Vec<Finding>) {
        for (uid, paths) in &self.graph.uids.duplicates {
            for path in paths.iter().skip(1) {
                findings.push(Finding {
                    kind: FindingKind::DuplicateUid,
                    at: Location {
                        path: path.clone(),
                        line: 1,
                    },
                    message: format!(
                        "{} is also claimed by {}; references to it may load the wrong file",
                        uid.0, paths[0]
                    ),
                    target: uid.0.clone(),
                    suggestions: Vec::new(),
                });
            }
        }
        for (name, declarations) in self.graph.declarations.by_class_name() {
            for declaration in declarations.iter().skip(1) {
                let line = declaration
                    .class_name
                    .as_ref()
                    .map_or(1, |named| named.line);
                findings.push(Finding {
                    kind: FindingKind::DuplicateClassName,
                    at: Location {
                        path: declaration.path.clone(),
                        line,
                    },
                    message: format!(
                        "class_name {name} is also declared by {}",
                        declarations[0].path
                    ),
                    target: name.to_owned(),
                    suggestions: Vec::new(),
                });
            }
        }
    }

    // ---- script chains -----------------------------------------------------

    fn chain(&self, script: &ResPath) -> Chain<'g> {
        let declarations = self.graph.declarations;
        let classes = declarations.by_class_name();
        let mut chain = Chain {
            scripts: Vec::new(),
            complete: true,
        };
        let mut next = Some(script.clone());
        while let Some(path) = next.take() {
            let Some(indexed) = declarations.indexed(&path) else {
                chain.complete = false;
                break;
            };
            if chain
                .scripts
                .iter()
                .any(|seen| seen.declaration.path == path)
                || chain.scripts.len() > MAX_DEPTH
            {
                chain.complete = false;
                break;
            }
            chain.complete &= indexed.parse_error.is_none();
            chain.scripts.push(indexed);
            let Some(extends) = &indexed.declaration.extends else {
                break;
            };
            if let Some(base) = extends.quoted_path() {
                match resolve_script_path(&path, base) {
                    Some(base) => next = Some(base),
                    None => chain.complete = false,
                }
            } else if extends.name.contains(['"', '\'']) {
                // `extends "res://a.gd".Inner`: an inner class of another script.
                chain.complete = false;
            } else if let Some(bases) = classes.get(extends.name.as_str()) {
                next = Some(bases[0].path.clone());
            } else if classes.contains_key(extends.name.split('.').next().unwrap_or_default()) {
                chain.complete = false;
            }
            // Anything else is a native class: the chain ends complete.
        }
        chain
    }

    /// Script path -> every (scene, node path) whose node runs that script,
    /// directly or as a base of the node's own script. Inherited scenes own
    /// what their bases own unless they override the node's script.
    fn owners(&self) -> BTreeMap<ResPath, Vec<(ResPath, NodePath)>> {
        let mut owners: BTreeMap<ResPath, Vec<(ResPath, NodePath)>> = BTreeMap::new();
        for (scene_path, scene) in &self.graph.scenes {
            if scene.kind != FileKind::Scene {
                continue;
            }
            for (node, script) in self.effective_scripts(scene_path, 0) {
                for base in self.chain(&script).scripts {
                    owners
                        .entry(base.declaration.path.clone())
                        .or_default()
                        .push((scene_path.clone(), node.clone()));
                }
            }
        }
        owners
    }

    /// Node path -> script for nodes this scene or its inherited bases attach a script to.
    fn effective_scripts(&self, scene_path: &ResPath, depth: usize) -> BTreeMap<NodePath, ResPath> {
        let Some(scene) = self.graph.scene(scene_path) else {
            return BTreeMap::new();
        };
        let mut scripts = match scene.inherited_base() {
            Some(base) if depth < MAX_DEPTH => {
                self.effective_scripts(&self.ext_target(base), depth + 1)
            }
            _ => BTreeMap::new(),
        };
        for node in &scene.nodes {
            let path = normalized(node.path());
            match node.properties.get("script") {
                Some(Value::Null) => {
                    scripts.remove(&path);
                }
                Some(_) => {
                    if let Some(script) = node
                        .script
                        .as_ref()
                        .and_then(|r| self.ext_ref_target(scene, r))
                    {
                        scripts.insert(path, script);
                    }
                }
                None => {}
            }
        }
        scripts
    }

    // ---- node lookup in the effective tree -----------------------------------

    fn lookup(&self, scene: &ResPath, path: &NodePath) -> Lookup {
        let segments: Vec<String> = normalized(path.clone())
            .segments()
            .filter(|s| *s != ".")
            .map(str::to_owned)
            .collect();
        self.lookup_segments(scene, &segments, 0)
    }

    fn lookup_segments(&self, scene_path: &ResPath, segments: &[String], depth: usize) -> Lookup {
        if depth > MAX_DEPTH {
            return Lookup::Unknown;
        }
        let key = (scene_path.clone(), segments.to_vec());
        if let Some(cached) = self.lookups.borrow().get(&key) {
            return cached.clone();
        }
        let result = self.lookup_uncached(scene_path, segments, depth);
        self.lookups.borrow_mut().insert(key, result.clone());
        result
    }

    fn lookup_uncached(&self, scene_path: &ResPath, segments: &[String], depth: usize) -> Lookup {
        let Some(scene) = self.graph.scene(scene_path) else {
            return Lookup::Unknown;
        };
        let node_path = NodePath(if segments.is_empty() {
            ".".into()
        } else {
            segments.join("/")
        });
        let direct = scene.node(&node_path);
        // An inherited scene's tree starts as its base's tree, path for path.
        let from_base = match scene.inherited_base() {
            Some(base) => self.lookup_segments(&self.ext_target(base), segments, depth + 1),
            None => Lookup::NotFound,
        };
        // Below an instanced node, the tree is the instanced scene's.
        let mut from_instance = Lookup::NotFound;
        for split in (1..segments.len()).rev() {
            match self.lookup_segments(scene_path, &segments[..split], depth + 1) {
                Lookup::Found(NodeFacts {
                    instance: Some(instanced),
                    ..
                }) => {
                    from_instance = self.lookup_segments(&instanced, &segments[split..], depth + 1);
                    break;
                }
                Lookup::Found(_) => {}
                Lookup::NotFound => {}
                Lookup::Unknown => {
                    from_instance = Lookup::Unknown;
                    break;
                }
            }
        }
        let below = match (&from_base, &from_instance) {
            (Lookup::Found(facts), _) | (_, Lookup::Found(facts)) => Some(facts.clone()),
            _ => None,
        };
        let Some(direct) = direct else {
            return match below {
                Some(facts) => Lookup::Found(facts),
                None if from_base == Lookup::Unknown || from_instance == Lookup::Unknown => {
                    Lookup::Unknown
                }
                None => Lookup::NotFound,
            };
        };
        let instance = direct
            .instance
            .as_ref()
            .and_then(|r| self.ext_ref_target(scene, r));
        let script = match direct.properties.get("script") {
            Some(Value::Null) => ScriptFact::None,
            Some(_) => match direct
                .script
                .as_ref()
                .and_then(|r| self.ext_ref_target(scene, r))
            {
                Some(script) => ScriptFact::Script(script),
                None => ScriptFact::Unknown,
            },
            None => match (&instance, &below) {
                (Some(instanced), _) => match self.lookup_segments(instanced, &[], depth + 1) {
                    Lookup::Found(root) => root.script,
                    _ => ScriptFact::Unknown,
                },
                (None, Some(facts)) => facts.script.clone(),
                (None, None) if direct.type_name.is_none() => ScriptFact::Unknown,
                (None, None) => ScriptFact::None,
            },
        };
        let instance = instance.or_else(|| below.and_then(|facts| facts.instance));
        Lookup::Found(NodeFacts { script, instance })
    }

    /// Resolves a `get_node` path from `node` inside `scene`.
    fn resolve_from(&self, scene: &ResPath, node: &NodePath, path: &str) -> Resolution {
        if path.starts_with('/') {
            return Resolution::Unknown;
        }
        let mut current: Vec<String> = node
            .segments()
            .filter(|s| *s != ".")
            .map(str::to_owned)
            .collect();
        for (index, segment) in path.split('/').filter(|s| !s.is_empty()).enumerate() {
            if let Some(unique) = segment.strip_prefix('%') {
                // `%Name` resolves among the nodes the scene root owns; later `%` segments
                // would need the owner of an intermediate node.
                if index > 0 {
                    return Resolution::Unknown;
                }
                match self.unique_node(scene, unique) {
                    Some(found) => current = found,
                    None => {
                        return Resolution::MissingUnique {
                            name: unique.to_owned(),
                        };
                    }
                }
                continue;
            }
            match segment {
                "." => {}
                ".." => {
                    // Above the scene root depends on where the scene is instanced.
                    if current.pop().is_none() {
                        return Resolution::Unknown;
                    }
                }
                name => {
                    let parent = current.clone();
                    current.push(name.to_owned());
                    match self.lookup_segments(scene, &current, 0) {
                        Lookup::Found(_) => {}
                        Lookup::Unknown => return Resolution::Unknown,
                        Lookup::NotFound => {
                            let parent = NodePath(if parent.is_empty() {
                                ".".into()
                            } else {
                                parent.join("/")
                            });
                            return Resolution::Missing {
                                parent,
                                name: name.to_owned(),
                            };
                        }
                    }
                }
            }
        }
        Resolution::Found
    }

    /// Path of the node marked `unique_name_in_owner` with this name, declared in
    /// the scene or its inherited bases (nodes inside instances belong to their own scene).
    fn unique_node(&self, scene_path: &ResPath, name: &str) -> Option<Vec<String>> {
        let mut scene_path = scene_path.clone();
        for _ in 0..MAX_DEPTH {
            let scene = self.graph.scene(&scene_path)?;
            let found = scene
                .nodes
                .iter()
                .find(|node| node.name == name && node.unique_name_in_owner);
            if let Some(node) = found {
                return Some(
                    normalized(node.path())
                        .segments()
                        .filter(|s| *s != ".")
                        .map(str::to_owned)
                        .collect(),
                );
            }
            scene_path = self.ext_target(scene.inherited_base()?);
        }
        None
    }

    /// Every `%` name the scene root owns, including those of inherited bases.
    fn unique_names(&self, scene_path: &ResPath) -> Vec<String> {
        let mut names = Vec::new();
        let mut scene_path = scene_path.clone();
        for _ in 0..MAX_DEPTH {
            let Some(scene) = self.graph.scene(&scene_path) else {
                break;
            };
            names.extend(
                scene
                    .nodes
                    .iter()
                    .filter(|node| node.unique_name_in_owner)
                    .map(|node| node.name.clone()),
            );
            let Some(base) = scene.inherited_base() else {
                break;
            };
            scene_path = self.ext_target(base);
        }
        names
    }

    /// Names of the children of `parent` in the effective tree, including
    /// children contributed by inherited bases and instanced scenes.
    fn child_names(&self, scene_path: &ResPath, parent: &[String], depth: usize) -> Vec<String> {
        let Some(scene) = self.graph.scene(scene_path) else {
            return Vec::new();
        };
        if depth > MAX_DEPTH {
            return Vec::new();
        }
        let parent_path = NodePath(if parent.is_empty() {
            ".".into()
        } else {
            parent.join("/")
        });
        let mut names: Vec<String> = scene
            .children_of(&parent_path)
            .map(|node| node.name.clone())
            .collect();
        if let Some(base) = scene.inherited_base() {
            names.extend(self.child_names(&self.ext_target(base), parent, depth + 1));
        }
        for split in (1..=parent.len()).rev() {
            if let Lookup::Found(NodeFacts {
                instance: Some(instanced),
                ..
            }) = self.lookup_segments(scene_path, &parent[..split], depth + 1)
            {
                names.extend(self.child_names(&instanced, &parent[split..], depth + 1));
                break;
            }
        }
        names
    }

    // ---- suggestions ---------------------------------------------------------

    /// Files with the same name elsewhere (a move), then close names in the same directory (a rename).
    fn suggest_files(&self, missing: &ResPath) -> Vec<String> {
        let name = missing.file_name();
        let mut suggestions: Vec<String> = self
            .graph
            .files
            .iter()
            .filter(|file| file.file_name() == name)
            .map(ToString::to_string)
            .collect();
        let parent = missing.parent();
        let siblings = self
            .graph
            .files
            .iter()
            .filter(|file| file.parent() == parent);
        let close = similar(name, siblings.map(|file| file.file_name()));
        suggestions.extend(
            close
                .into_iter()
                .filter_map(|close| Some(parent.as_ref()?.join(&close).ok()?.to_string())),
        );
        suggestions.dedup();
        suggestions.truncate(3);
        suggestions
    }
}

enum Resolution {
    Found,
    Missing { parent: NodePath, name: String },
    MissingUnique { name: String },
    Unknown,
}

fn normalized(path: NodePath) -> NodePath {
    match path.0.trim_start_matches("./") {
        "" | "." => NodePath(".".into()),
        rest => NodePath(rest.to_owned()),
    }
}

/// A path as a script spells it: `res://…`, or relative to the script's directory.
/// `None` for anything that is not a project path (`user://`, absolute OS paths,
/// relative paths that climb out of the project).
fn resolve_script_path(script: &ResPath, written: &str) -> Option<ResPath> {
    if written.starts_with("res://") {
        return normalize_res(written.strip_prefix("res://")?);
    }
    if written.contains("://") || written.starts_with('/') || written.contains('\\') {
        return None;
    }
    let directory = script.parent()?;
    let joined = match directory.relative() {
        "" => written.to_owned(),
        dir => format!("{dir}/{written}"),
    };
    normalize_res(&joined)
}

fn normalize_res(relative: &str) -> Option<ResPath> {
    let mut segments: Vec<&str> = Vec::new();
    for segment in relative.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            name => segments.push(name),
        }
    }
    ResPath::from_relative(&segments.join("/")).ok()
}

/// Up to three names within a small edit distance of `name` (a third of its
/// length, between 1 and 3 edits), closest first.
fn similar<'n>(name: &str, candidates: impl Iterator<Item = &'n str>) -> Vec<String> {
    let limit = (name.chars().count() / 3).clamp(1, 3);
    let mut scored: Vec<(usize, &str)> = candidates
        .filter(|candidate| *candidate != name)
        .map(|candidate| {
            (
                edit_distance(&name.to_lowercase(), &candidate.to_lowercase()),
                candidate,
            )
        })
        .filter(|(distance, _)| *distance <= limit)
        .collect();
    scored.sort();
    scored.dedup();
    scored
        .into_iter()
        .take(3)
        .map(|(_, candidate)| candidate.to_owned())
        .collect()
}

/// Optimal string alignment distance: Levenshtein plus adjacent transpositions,
/// so the commonest typo (`Lable`) is one edit away.
fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut rows = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in rows.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in rows[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (rows[i - 1][j] + 1)
                .min(rows[i][j - 1] + 1)
                .min(rows[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(rows[i - 2][j - 2] + 1);
            }
            rows[i][j] = best;
        }
    }
    rows[a.len()][b.len()]
}
