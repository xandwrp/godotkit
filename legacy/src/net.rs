use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

use gdkit::net_report::{
    AuthorityAssignment, LifecycleFinding, MultiplayerContext, NET_REPORT_SCHEMA_VERSION,
    NetAutoload, NetCoverage, NetEngine, NetReport, ReplicationNode, ReplicationProperty, RpcCall,
    RpcContract, RpcContractEndpoint, RpcEndpoint, SourceFinding, SourceLocation,
};
use gdview::syntax::{
    SyntaxKind as K,
    ast::{AstNode, Function},
    parse,
};
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::{
    cli::{NetArgs, NetCommand, NetExplainArgs, NetOutput},
    engine, project_files,
};

const RESULT_PREFIX: &str = "GDKIT_NET_RESULT:";

#[derive(Deserialize)]
struct EngineIndex {
    version: String,
    scripts: Vec<EngineScript>,
    errors: Vec<String>,
    resolved_paths: Map<String, Value>,
}

#[derive(Deserialize)]
struct EngineScript {
    path: String,
    rpc_config: Map<String, Value>,
}

struct TemporaryFile(PathBuf);

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[derive(Clone)]
struct SourceFunction {
    signature: String,
    line: usize,
    end_line: usize,
    rpc_line: Option<usize>,
}

#[derive(Default)]
struct SourceIndex {
    functions: HashMap<(String, String), SourceFunction>,
    rpc_calls: Vec<RpcCall>,
    peer_constructions: Vec<SourceFinding>,
    peer_assignments: Vec<SourceFinding>,
    lifecycle: Vec<LifecycleFinding>,
    authority: Vec<SourceFinding>,
    authority_assignments: Vec<AuthorityAssignment>,
    multiplayer_contexts: Vec<MultiplayerContext>,
    unknowns: Vec<String>,
}

fn temporary_file(
    stem: &str,
    extension: &str,
    contents: &[u8],
) -> Result<TemporaryFile, Box<dyn Error>> {
    for attempt in 0..100 {
        let path = std::env::temp_dir().join(format!(
            "gdkit-{stem}-{}-{attempt}.{extension}",
            std::process::id()
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                file.write_all(contents)?;
                return Ok(TemporaryFile(path));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(format!("could not create temporary {stem} file").into())
}

fn resource_path(project: &Path, path: &Path) -> Result<String, Box<dyn Error>> {
    Ok(format!(
        "res://{}",
        path.strip_prefix(project)?
            .to_string_lossy()
            .replace('\\', "/")
    ))
}

fn query_engine(
    engine_path: &Path,
    project: &Path,
    scripts: &[PathBuf],
) -> Result<EngineIndex, Box<dyn Error>> {
    let script = temporary_file("net-query", "gd", include_bytes!("net.gd"))?;
    let paths: Vec<_> = scripts
        .iter()
        .map(|path| resource_path(project, path))
        .collect::<Result<_, _>>()?;
    let manifest = temporary_file("net-scripts", "json", &serde_json::to_vec(&paths)?)?;
    let output = Command::new(engine_path)
        .args(["--headless", "--no-header", "--path"])
        .arg(project)
        .arg("--script")
        .arg(&script.0)
        .arg("--")
        .arg(&manifest.0)
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let index = stdout
        .lines()
        .find_map(|line| line.strip_prefix(RESULT_PREFIX))
        .map(serde_json::from_str::<EngineIndex>)
        .transpose()?;
    match index {
        Some(index) if output.status.success() => Ok(index),
        _ => Err(format!(
            "configured engine could not inspect project RPC metadata\n{}{}",
            stdout,
            String::from_utf8_lossy(&output.stderr)
        )
        .into()),
    }
}

fn source_line(source: &str, offset: usize) -> usize {
    source[..offset]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
}

fn node_line(source: &str, node: gdview::syntax::Node<'_>) -> usize {
    let offset = node
        .tokens()
        .find(|token| !token.kind.is_trivia() && !token.kind.is_synthetic_layout())
        .map_or(node.range().start, |token| token.range.start);
    source_line(source, offset)
}

fn compact(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn function_signature(function: Function<'_>) -> String {
    let syntax = function.syntax();
    let end = function
        .body()
        .map_or(syntax.range().end, |body| body.syntax().range().start);
    compact(
        syntax.text()[..end - syntax.range().start].trim_end_matches([':', ' ', '\t', '\r', '\n']),
    )
}

fn annotation_name(node: gdview::syntax::Node<'_>) -> Option<&str> {
    node.tokens()
        .find(|token| token.kind == K::Ident)
        .map(|token| {
            &node.text()
                [token.range.start - node.range().start..token.range.end - node.range().start]
        })
}

fn collect_functions(
    container: gdview::syntax::Node<'_>,
    source: &str,
    path: &str,
    functions: &mut HashMap<(String, String), SourceFunction>,
) {
    let mut rpc_line = None;
    for child in container.children() {
        match child.kind() {
            K::Annotation => {
                if annotation_name(child) == Some("rpc") {
                    rpc_line = Some(node_line(source, child));
                }
            }
            K::FuncDecl => {
                if let Some(function) = Function::cast(child)
                    && let Some(name) = function.name()
                {
                    functions.insert(
                        (path.to_owned(), name.to_owned()),
                        SourceFunction {
                            signature: function_signature(function),
                            line: node_line(source, child),
                            end_line: source_line(source, child.range().end),
                            rpc_line,
                        },
                    );
                }
                rpc_line = None;
            }
            _ => rpc_line = None,
        }
    }
}

fn call_parts(node: gdview::syntax::Node<'_>) -> Option<(String, Vec<String>)> {
    let arguments = node.children().find(|child| child.kind() == K::ArgList)?;
    let offset = arguments.range().start.checked_sub(node.range().start)?;
    let callee = node.text()[..offset].trim().to_owned();
    let values = arguments
        .children()
        .map(|argument| compact(argument.text()))
        .collect();
    Some((callee, values))
}

fn rpc_call(callee: &str, arguments: &[String], source: SourceLocation) -> Option<RpcCall> {
    if callee == "multiplayer.rpc" {
        return Some(RpcCall {
            method: arguments
                .get(2)
                .map(|method| method.trim_start_matches('&').trim_matches('"').to_owned())
                .unwrap_or_else(|| "dynamic".into()),
            kind: "MultiplayerAPI.rpc".into(),
            expression: format!("{callee}({})", arguments.join(", ")),
            receiver: arguments.get(1).cloned(),
            target: arguments.first().cloned(),
            source,
        });
    }
    for (suffix, kind) in [(".rpc_id", "rpc_id"), (".rpc", "rpc")] {
        if let Some(receiver) = callee.strip_suffix(suffix) {
            let (receiver, method) = receiver
                .rsplit_once('.')
                .map_or((None, receiver), |(receiver, method)| {
                    (Some(receiver.to_owned()), method)
                });
            return Some(RpcCall {
                method: method.to_owned(),
                kind: kind.into(),
                expression: format!("{callee}({})", arguments.join(", ")),
                receiver,
                target: (kind == "rpc_id")
                    .then(|| arguments.first().cloned())
                    .flatten(),
                source,
            });
        }
    }
    None
}

fn scan_script(path: &str, source: &str, index: &mut SourceIndex) {
    let parsed = parse(source);
    if !parsed.is_valid() {
        index.unknowns.push(format!(
            "{path}: gdview reported {} syntax error(s); findings may be incomplete",
            parsed.errors().len()
        ));
    }
    collect_functions(parsed.root(), source, path, &mut index.functions);
    for node in parsed.root().descendants() {
        let location = SourceLocation {
            path: path.to_owned(),
            line: node_line(source, node),
        };
        if node.kind() == K::AssignExpr {
            let value = compact(node.text());
            let left = value
                .split_once('=')
                .map_or(value.as_str(), |(left, _)| left);
            if left.trim().ends_with("multiplayer_peer") {
                index.peer_assignments.push(SourceFinding {
                    value,
                    source: location,
                });
            }
            continue;
        }
        if node.kind() != K::CallExpr {
            continue;
        }
        let Some((callee, arguments)) = call_parts(node) else {
            continue;
        };
        if let Some(call) = rpc_call(&callee, &arguments, location.clone()) {
            index.rpc_calls.push(call);
        }
        if callee.ends_with(".set_multiplayer") || callee == "set_multiplayer" {
            index.multiplayer_contexts.push(MultiplayerContext {
                multiplayer_api: arguments
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "dynamic".into()),
                subtree_root: arguments
                    .get(1)
                    .map(|value| node_path_value(value))
                    .unwrap_or_else(|| "/root".into()),
                source: Some(location.clone()),
            });
        }
        if let Some(class) = callee.strip_suffix(".new")
            && class
                .rsplit('.')
                .next()
                .is_some_and(|name| name.ends_with("MultiplayerPeer"))
        {
            index.peer_constructions.push(SourceFinding {
                value: class.to_owned(),
                source: location.clone(),
            });
        }
        for signal in [
            "peer_connected",
            "peer_disconnected",
            "connected_to_server",
            "connection_failed",
            "server_disconnected",
        ] {
            let prefix = format!("multiplayer.{signal}.");
            if let Some(operation) = callee.strip_prefix(&prefix)
                && matches!(operation, "connect" | "disconnect" | "emit")
            {
                index.lifecycle.push(LifecycleFinding {
                    signal: signal.into(),
                    operation: operation.into(),
                    source: location.clone(),
                });
            }
        }
        if [
            "is_server",
            "get_remote_sender_id",
            "get_unique_id",
            "get_multiplayer_authority",
            "is_multiplayer_authority",
            "set_multiplayer_authority",
        ]
        .iter()
        .any(|method| callee == *method || callee.ends_with(&format!(".{method}")))
        {
            index.authority.push(SourceFinding {
                value: format!("{callee}({})", arguments.join(", ")),
                source: location.clone(),
            });
        }
        if callee == "set_multiplayer_authority" || callee.ends_with(".set_multiplayer_authority") {
            let node = callee
                .strip_suffix(".set_multiplayer_authority")
                .unwrap_or("self");
            index.authority_assignments.push(AuthorityAssignment {
                node: node.to_owned(),
                authority: arguments
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "dynamic".into()),
                recursive: arguments.get(1).is_none_or(|value| value != "false"),
                source: location,
            });
        }
    }
}

fn node_path_value(value: &str) -> String {
    let value = value.trim();
    let inner = value
        .strip_prefix("NodePath(")
        .and_then(|value| value.strip_suffix(')'))
        .unwrap_or(value)
        .trim();
    let path = inner.strip_prefix('&').unwrap_or(inner).trim_matches('"');
    if path.is_empty() || path == "." {
        "/root".into()
    } else {
        path.to_owned()
    }
}

fn config_integer(config: &Map<String, Value>, name: &str, default: i64) -> i64 {
    config.get(name).and_then(Value::as_i64).unwrap_or(default)
}

fn config_boolean(config: &Map<String, Value>, name: &str, default: bool) -> bool {
    config.get(name).and_then(Value::as_bool).unwrap_or(default)
}

fn rpc_mode(value: i64) -> String {
    match value {
        0 => "disabled",
        1 => "any_peer",
        2 => "authority",
        _ => "unknown",
    }
    .into()
}

fn transfer_mode(value: i64) -> String {
    match value {
        0 => "unreliable",
        1 => "unreliable_ordered",
        2 => "reliable",
        _ => "unknown",
    }
    .into()
}

fn rpc_endpoints(engine_index: &EngineIndex, source: &SourceIndex) -> Vec<RpcEndpoint> {
    let mut endpoints = Vec::new();
    for script in &engine_index.scripts {
        for (method, value) in &script.rpc_config {
            let Some(config) = value.as_object() else {
                continue;
            };
            let source_function = source.functions.get(&(script.path.clone(), method.clone()));
            endpoints.push(RpcEndpoint {
                method: method.clone(),
                signature: source_function.map(|function| function.signature.clone()),
                source: SourceLocation {
                    path: script.path.clone(),
                    line: source_function
                        .map_or(0, |function| function.rpc_line.unwrap_or(function.line)),
                },
                rpc_mode: rpc_mode(config_integer(config, "rpc_mode", 2)),
                call: if config_boolean(config, "call_local", false) {
                    "call_local"
                } else {
                    "call_remote"
                }
                .into(),
                transfer_mode: transfer_mode(config_integer(config, "transfer_mode", 2)),
                channel: config_integer(config, "channel", 0),
                inherited: source_function.is_none_or(|function| function.rpc_line.is_none()),
            });
        }
    }
    endpoints.sort_by(|left, right| {
        left.source
            .path
            .cmp(&right.source.path)
            .then_with(|| left.method.cmp(&right.method))
    });
    endpoints
}

fn replication_nodes(
    project: &Path,
    scenes: &[PathBuf],
    unknowns: &mut Vec<String>,
) -> Vec<ReplicationNode> {
    let mut nodes = Vec::new();
    for path in scenes {
        let display = match resource_path(project, path) {
            Ok(path) => path,
            Err(error) => {
                unknowns.push(format!("{}: {error}", path.display()));
                continue;
            }
        };
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(error) => {
                unknowns.push(format!("{display}: {error}"));
                continue;
            }
        };
        let scene = match gdview::scene::parse(&source) {
            Ok(scene) => scene,
            Err(error) => {
                unknowns.push(format!("{display}: {error}"));
                continue;
            }
        };
        let mut scene_nodes = Vec::new();
        for node in scene.nodes {
            let Some(kind) = node.kind else {
                continue;
            };
            if !matches!(
                kind.as_str(),
                "MultiplayerSpawner" | "MultiplayerSynchronizer"
            ) {
                continue;
            }
            let node_path = match node.parent.as_deref() {
                None | Some(".") => node.name,
                Some(parent) => format!("{parent}/{}", node.name),
            };
            scene_nodes.push(ReplicationNode {
                kind,
                node_path,
                scene: display.clone(),
                root_path: None,
                spawn_path: None,
                spawn_limit: None,
                spawnable_scenes: Vec::new(),
                replication_properties: Vec::new(),
                related_nodes: Vec::new(),
                authority_assignments: Vec::new(),
            });
        }
        enrich_replication_nodes(&source, &mut scene_nodes);
        nodes.extend(scene_nodes);
    }
    nodes.sort_by(|left, right| {
        left.scene
            .cmp(&right.scene)
            .then_with(|| left.node_path.cmp(&right.node_path))
    });
    nodes
}

#[derive(Default)]
struct ReplicationPropertyBuilder {
    path: Option<String>,
    spawn: Option<bool>,
    mode: Option<i64>,
}

fn quoted_values(value: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut chars = value.char_indices().peekable();
    while let Some((_, character)) = chars.next() {
        if character != '"' {
            continue;
        }
        let mut output = String::new();
        let mut escaped = false;
        for (_, character) in chars.by_ref() {
            if escaped {
                output.push(character);
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                break;
            } else {
                output.push(character);
            }
        }
        values.push(output);
    }
    values
}

fn header_attribute(header: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=\"");
    let rest = header.split_once(&needle)?.1;
    Some(rest.split_once('"')?.0.to_owned())
}

fn constructor_string(value: &str) -> Option<String> {
    quoted_values(value).into_iter().next()
}

fn enrich_replication_nodes(source: &str, nodes: &mut [ReplicationNode]) {
    let mut current_subresource = None;
    let mut current_node = None;
    let mut configurations: HashMap<String, HashMap<usize, ReplicationPropertyBuilder>> =
        HashMap::new();
    let mut node_configurations = HashMap::new();
    for line in source.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            current_subresource = (line.starts_with("[sub_resource")
                && header_attribute(line, "type").as_deref() == Some("SceneReplicationConfig"))
            .then(|| header_attribute(line, "id"))
            .flatten();
            current_node = if line.starts_with("[node") {
                let name = header_attribute(line, "name");
                let parent = header_attribute(line, "parent");
                name.map(|name| match parent.as_deref() {
                    None | Some(".") => name,
                    Some(parent) => format!("{parent}/{name}"),
                })
            } else {
                None
            };
            continue;
        }
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        let name = name.trim();
        let value = value.trim();
        if let Some(id) = &current_subresource
            && let Some(rest) = name.strip_prefix("properties/")
            && let Some((index, field)) = rest.split_once('/')
            && let Ok(index) = index.parse::<usize>()
        {
            let property = configurations
                .entry(id.clone())
                .or_default()
                .entry(index)
                .or_default();
            match field {
                "path" => property.path = constructor_string(value),
                "spawn" => property.spawn = Some(value == "true"),
                "replication_mode" => property.mode = value.parse().ok(),
                _ => {}
            }
        }
        let Some(node_path) = &current_node else {
            continue;
        };
        let Some(node) = nodes.iter_mut().find(|node| &node.node_path == node_path) else {
            continue;
        };
        match name {
            "root_path" => node.root_path = constructor_string(value),
            "spawn_path" => node.spawn_path = constructor_string(value),
            "spawn_limit" => node.spawn_limit = value.parse().ok(),
            "_spawnable_scenes" => node.spawnable_scenes = quoted_values(value),
            "replication_config" => {
                if let Some(id) = constructor_string(value) {
                    node_configurations.insert(node_path.clone(), id);
                }
            }
            _ => {}
        }
    }
    for node in nodes {
        let Some(id) = node_configurations.get(&node.node_path) else {
            continue;
        };
        let Some(properties) = configurations.get(id) else {
            continue;
        };
        let mut properties = properties.iter().collect::<Vec<_>>();
        properties.sort_by_key(|(index, _)| **index);
        node.replication_properties = properties
            .into_iter()
            .filter_map(|(_, property)| {
                let mode = property.mode.unwrap_or(1);
                Some(ReplicationProperty {
                    path: property.path.clone()?,
                    spawn: property.spawn.unwrap_or(true),
                    sync: mode != 0,
                    mode: match mode {
                        0 => "never",
                        1 => "always",
                        2 => "on_change",
                        _ => "unknown",
                    }
                    .into(),
                })
            })
            .collect();
    }
}

fn location_text(location: &SourceLocation) -> String {
    if location.line == 0 {
        location.path.clone()
    } else {
        format!("{}:{}", location.path, location.line)
    }
}

#[derive(Clone)]
struct ScriptAnchor {
    script: String,
    receiver_path: String,
    label: String,
    stable_path: String,
    scene: Option<String>,
}

fn script_anchors(
    project: &Path,
    scenes: &[PathBuf],
    autoloads: &[NetAutoload],
    unknowns: &mut Vec<String>,
) -> Vec<ScriptAnchor> {
    let mut anchors = autoloads
        .iter()
        .map(|autoload| ScriptAnchor {
            script: autoload
                .resolved_path
                .clone()
                .unwrap_or_else(|| autoload.path.clone()),
            receiver_path: format!("/root/{}", autoload.name),
            label: autoload.name.clone(),
            stable_path: "autoload on every participant".into(),
            scene: None,
        })
        .collect::<Vec<_>>();
    for path in scenes {
        let display = match resource_path(project, path) {
            Ok(display) => display,
            Err(error) => {
                unknowns.push(format!("{}: {error}", path.display()));
                continue;
            }
        };
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) => {
                unknowns.push(format!("{display}: {error}"));
                continue;
            }
        };
        let scene = match gdview::scene::parse(&text) {
            Ok(scene) => scene,
            Err(_) => continue,
        };
        let resources = scene
            .external_resources
            .iter()
            .map(|resource| (resource.id.as_str(), resource.path.as_str()))
            .collect::<HashMap<_, _>>();
        let Some(root) = scene.nodes.iter().find(|node| node.parent.is_none()) else {
            continue;
        };
        for node in &scene.nodes {
            let Some(script) = node
                .script
                .as_deref()
                .and_then(|id| resources.get(id).copied())
            else {
                continue;
            };
            let relative = match node.parent.as_deref() {
                None => root.name.clone(),
                Some(".") => format!("{}/{}", root.name, node.name),
                Some(parent) => format!("{}/{parent}/{}", root.name, node.name),
            };
            anchors.push(ScriptAnchor {
                script: script.to_owned(),
                receiver_path: relative.clone(),
                label: node.name.clone(),
                stable_path: format!("scene node {relative} in {display} must match across peers"),
                scene: Some(display.clone()),
            });
        }
    }
    anchors
}

fn link_replication_contracts(
    nodes: &mut [ReplicationNode],
    assignments: &[AuthorityAssignment],
    anchors: &[ScriptAnchor],
) {
    let identities = nodes
        .iter()
        .map(|node| {
            (
                node.scene.clone(),
                node.kind.clone(),
                node.node_path.clone(),
            )
        })
        .collect::<Vec<_>>();
    for node in nodes {
        node.related_nodes = identities
            .iter()
            .filter(|(scene, kind, path)| {
                scene == &node.scene && kind != &node.kind && path != &node.node_path
            })
            .map(|(_, kind, path)| format!("{kind} {path}"))
            .collect();
        node.authority_assignments = assignments
            .iter()
            .filter(|assignment| {
                anchors.iter().any(|anchor| {
                    anchor.script == assignment.source.path
                        && anchor.scene.as_deref() == Some(node.scene.as_str())
                })
            })
            .cloned()
            .collect();
    }
}

fn matching_context(path: &str, contexts: &[MultiplayerContext]) -> String {
    contexts
        .iter()
        .filter(|context| path.starts_with(&context.subtree_root))
        .max_by_key(|context| context.subtree_root.len())
        .map_or_else(|| "/root".into(), |context| context.subtree_root.clone())
}

fn build_rpc_contracts(
    calls: &[RpcCall],
    endpoints: &[RpcEndpoint],
    functions: &HashMap<(String, String), SourceFunction>,
    authority: &[SourceFinding],
    contexts: &[MultiplayerContext],
    anchors: &[ScriptAnchor],
) -> Vec<RpcContract> {
    calls
        .iter()
        .map(|call| {
            let receiver_anchor = match call.receiver.as_deref() {
                None | Some("self") => anchors.iter().find(|anchor| anchor.script == call.source.path),
                Some(receiver) => anchors.iter().find(|anchor| anchor.label == receiver),
            };
            let candidate_script = receiver_anchor
                .map(|anchor| anchor.script.as_str())
                .or_else(|| call.receiver.is_none().then_some(call.source.path.as_str()));
            let mut compatible_endpoints = Vec::new();
            for endpoint in endpoints.iter().filter(|endpoint| {
                endpoint.method == call.method
                    && candidate_script.is_none_or(|script| endpoint.source.path == script)
            }) {
                let endpoint_anchors = anchors
                    .iter()
                    .filter(|anchor| anchor.script == endpoint.source.path)
                    .collect::<Vec<_>>();
                let endpoint_anchors: Vec<Option<&ScriptAnchor>> = if endpoint_anchors.is_empty() {
                    vec![None]
                } else {
                    endpoint_anchors.into_iter().map(Some).collect()
                };
                for anchor in endpoint_anchors {
                    let sender_identity = functions
                        .get(&(endpoint.source.path.clone(), endpoint.method.clone()))
                        .map(|function| {
                            authority
                                .iter()
                                .filter(|finding| {
                                    finding.source.path == endpoint.source.path
                                        && finding.source.line >= function.line
                                        && finding.source.line <= function.end_line
                                        && finding.value.contains("get_remote_sender_id")
                                })
                                .cloned()
                                .collect()
                        })
                        .unwrap_or_default();
                    let receiver_path = anchor.map(|anchor| anchor.receiver_path.clone());
                    compatible_endpoints.push(RpcContractEndpoint {
                        endpoint: endpoint.clone(),
                        multiplayer_root: receiver_path
                            .as_deref()
                            .map_or_else(|| "/root".into(), |path| matching_context(path, contexts)),
                        receiver_path,
                        recipient: match (call.kind.as_str(), call.target.as_deref()) {
                            ("rpc", _) => "all peers".into(),
                            (_, Some("1")) => "server peer 1".into(),
                            (_, Some(target)) => format!("peer {target}"),
                            _ => "dynamic recipient".into(),
                        },
                        stable_path: anchor.map_or_else(
                            || "the same node path must exist in the multiplayer subtree on every participant".into(),
                            |anchor| anchor.stable_path.clone(),
                        ),
                        sender_identity,
                    });
                }
            }
            let unresolved_reason = compatible_endpoints.is_empty().then(|| {
                if candidate_script.is_some() {
                    "no compatible endpoint was found on the resolved receiver".into()
                } else {
                    "the receiver is dynamic, so method-name candidates cannot prove compatibility".into()
                }
            });
            RpcContract {
                call: call.clone(),
                compatible_endpoints,
                unresolved_reason,
            }
        })
        .collect()
}

fn qualified_call(call: &RpcCall, anchors: &[ScriptAnchor]) -> String {
    if call.receiver.is_some() || call.kind == "MultiplayerAPI.rpc" {
        return call.expression.clone();
    }
    let Some(anchor) = anchors
        .iter()
        .find(|anchor| anchor.script == call.source.path)
    else {
        return call.expression.clone();
    };
    format!("{}.{}", anchor.label, call.expression)
}

fn rpc_contract_matches(contract: &RpcContract, query: &str, anchors: &[ScriptAnchor]) -> bool {
    let qualified = qualified_call(&contract.call, anchors);
    contract.call.method == query || qualified.starts_with(&format!("{query}."))
}

fn print_explanation(report: &NetReport, query: &str, anchors: &[ScriptAnchor]) -> bool {
    let mut matched = false;
    for contract in &report.rpc_contracts {
        let qualified = qualified_call(&contract.call, anchors);
        if !rpc_contract_matches(contract, query, anchors) {
            continue;
        }
        matched = true;
        println!("RPC contract {}", contract.call.method);
        println!(
            "  {}: {} [{}]",
            if contract.call.target.as_deref() == Some("1") {
                "Client call"
            } else {
                "Call"
            },
            qualified,
            location_text(&contract.call.source)
        );
        for endpoint in &contract.compatible_endpoints {
            println!(
                "  Receiver: {} [{}]",
                endpoint
                    .receiver_path
                    .as_deref()
                    .unwrap_or("dynamic node path"),
                location_text(&endpoint.endpoint.source)
            );
            println!("  Permission: {}", endpoint.endpoint.rpc_mode);
            println!(
                "  Delivery: {}, {}, channel {}",
                endpoint.endpoint.transfer_mode,
                if endpoint.endpoint.call == "call_remote" {
                    "remote-only"
                } else {
                    "local-and-remote"
                },
                endpoint.endpoint.channel
            );
            println!("  Recipient: {}", endpoint.recipient);
            if endpoint.sender_identity.is_empty() {
                println!("  Sender identity: not read in the endpoint body");
            } else {
                for sender in &endpoint.sender_identity {
                    println!(
                        "  Sender identity: read with {} [{}]",
                        sender.value,
                        location_text(&sender.source)
                    );
                }
            }
            println!(
                "  Multiplayer context: subtree rooted at {}",
                endpoint.multiplayer_root
            );
            println!("  Stable path: {}", endpoint.stable_path);
        }
        if let Some(reason) = &contract.unresolved_reason {
            println!("  Unresolved: {reason}");
        }
    }
    for node in &report.replication_nodes {
        if query != node.node_path && query != node.scene && !node.node_path.ends_with(query) {
            continue;
        }
        matched = true;
        println!("Replication contract {} {}", node.kind, node.node_path);
        println!("  Scene: {}", node.scene);
        if let Some(path) = &node.spawn_path {
            println!("  Spawn path: {path}");
        }
        if let Some(path) = &node.root_path {
            println!("  Synchronization root: {path}");
        }
        if let Some(limit) = node.spawn_limit {
            println!("  Spawn limit: {limit}");
        }
        for scene in &node.spawnable_scenes {
            println!("  Spawnable scene: {scene}");
        }
        for property in &node.replication_properties {
            println!(
                "  Property: {} [spawn: {}; sync: {}; mode: {}]",
                property.path, property.spawn, property.sync, property.mode
            );
        }
        for related in &node.related_nodes {
            println!("  Related node: {related}");
        }
        for assignment in &node.authority_assignments {
            println!(
                "  Authority: {} assigned to peer {}{} [{}]",
                assignment.node,
                assignment.authority,
                if assignment.recursive {
                    " recursively"
                } else {
                    ""
                },
                location_text(&assignment.source)
            );
        }
        println!(
            "  Stable path: scene node paths and authority assignments must match across peers"
        );
    }
    matched
}

fn print_findings(label: &str, findings: &[SourceFinding]) {
    println!("{label} ({}):", findings.len());
    for finding in findings {
        println!("  {} [{}]", finding.value, location_text(&finding.source));
    }
}

fn print_human(report: &NetReport) {
    println!(
        "Engine {} ({})",
        report.engine.version, report.engine.executable
    );
    println!("Project {}", report.project);
    println!(
        "Coverage: {} GDScript files, {} text scenes",
        report.coverage.scripts_scanned, report.coverage.scenes_scanned
    );
    let networked: Vec<_> = report
        .autoloads
        .iter()
        .filter(|autoload| autoload.networked)
        .collect();
    println!("\nNetworked autoloads ({}):", networked.len());
    for autoload in networked {
        let resolved = autoload
            .resolved_path
            .as_deref()
            .map_or(String::new(), |path| format!(" (resolved: {path})"));
        println!(
            "  {}. {} -> {}{}{}",
            autoload.index,
            autoload.name,
            autoload.path,
            resolved,
            if autoload.singleton {
                " [singleton]"
            } else {
                ""
            }
        );
    }
    println!(
        "\nMultiplayer contexts ({}):",
        report.multiplayer_contexts.len()
    );
    for context in &report.multiplayer_contexts {
        println!(
            "  {} -> {}{}",
            context.multiplayer_api,
            context.subtree_root,
            context
                .source
                .as_ref()
                .map_or(String::new(), |source| format!(
                    " [{}]",
                    location_text(source)
                ))
        );
    }
    println!("\nRPC endpoints ({}):", report.rpc_endpoints.len());
    for endpoint in &report.rpc_endpoints {
        println!(
            "  {} [{}; {}, {}, {}, channel {}{}]",
            endpoint
                .signature
                .as_deref()
                .unwrap_or(endpoint.method.as_str()),
            location_text(&endpoint.source),
            endpoint.rpc_mode,
            endpoint.call,
            endpoint.transfer_mode,
            endpoint.channel,
            if endpoint.inherited {
                "; inherited"
            } else {
                ""
            }
        );
    }
    println!("\nRPC calls ({}):", report.rpc_calls.len());
    for call in &report.rpc_calls {
        let target = call
            .target
            .as_deref()
            .map_or(String::new(), |target| format!(" to {target}"));
        println!(
            "  {} via {}{} [{}]",
            call.method,
            call.kind,
            target,
            location_text(&call.source)
        );
    }
    println!();
    print_findings("Peer constructions", &report.peer_constructions);
    println!();
    print_findings("Multiplayer peer assignments", &report.peer_assignments);
    println!("\nConnection lifecycle uses ({}):", report.lifecycle.len());
    for finding in &report.lifecycle {
        println!(
            "  {}.{} [{}]",
            finding.signal,
            finding.operation,
            location_text(&finding.source)
        );
    }
    println!();
    print_findings("Authority and peer identity uses", &report.authority);
    println!(
        "\nAuthority assignments ({}):",
        report.authority_assignments.len()
    );
    for assignment in &report.authority_assignments {
        println!(
            "  {} -> peer {}{} [{}]",
            assignment.node,
            assignment.authority,
            if assignment.recursive {
                " recursively"
            } else {
                ""
            },
            location_text(&assignment.source)
        );
    }
    println!(
        "\nScene replication nodes ({}):",
        report.replication_nodes.len()
    );
    for node in &report.replication_nodes {
        println!("  {} {} [{}]", node.kind, node.node_path, node.scene);
        if let Some(path) = &node.spawn_path {
            println!("    spawn path: {path}");
        }
        if let Some(path) = &node.root_path {
            println!("    synchronization root: {path}");
        }
        for property in &node.replication_properties {
            println!(
                "    {} [spawn: {}; sync: {}; mode: {}]",
                property.path, property.spawn, property.sync, property.mode
            );
        }
        for related in &node.related_nodes {
            println!("    related: {related}");
        }
        for assignment in &node.authority_assignments {
            println!(
                "    authority: {} -> peer {}{} [{}]",
                assignment.node,
                assignment.authority,
                if assignment.recursive {
                    " recursively"
                } else {
                    ""
                },
                location_text(&assignment.source)
            );
        }
    }
    if !report.unknowns.is_empty() {
        println!("\nUnknown or incomplete ({}):", report.unknowns.len());
        for unknown in &report.unknowns {
            println!("  {unknown}");
        }
    }
}

fn sort_source_findings(findings: &mut [SourceFinding]) {
    findings.sort_by(|left, right| {
        left.source
            .path
            .cmp(&right.source.path)
            .then_with(|| left.source.line.cmp(&right.source.line))
            .then_with(|| left.value.cmp(&right.value))
    });
}

pub fn run(args: NetArgs) -> Result<ExitCode, Box<dyn Error>> {
    let (project_argument, godot_argument, output, explanation) = match args.command {
        Some(NetCommand::Explain(NetExplainArgs {
            query,
            project,
            godot,
            output,
        })) => (project, godot, output, Some(query)),
        None => (args.project, args.godot, args.output, None),
    };
    let project = engine::project_root(&project_argument)?;
    let engine_path = engine::resolve(&project, godot_argument.as_deref())?;
    let scripts = project_files::collect(&project, &["gd"])?;
    let scenes = project_files::collect(&project, &["tscn"])?;
    let csharp_scripts = project_files::collect(&project, &["cs"])?;
    let binary_scenes = project_files::collect(&project, &["scn"])?;
    let engine_index = query_engine(&engine_path, &project, &scripts)?;
    let mut source = SourceIndex::default();
    for path in &scripts {
        let display = resource_path(&project, path)?;
        let text =
            fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
        scan_script(&display, &text, &mut source);
    }
    if !csharp_scripts.is_empty() {
        source.unknowns.push(format!(
            "{} C# script(s) were not inspected for multiplayer declarations or calls",
            csharp_scripts.len()
        ));
    }
    if !binary_scenes.is_empty() {
        source.unknowns.push(format!(
            "{} binary scene(s) were not inspected for multiplayer replication nodes",
            binary_scenes.len()
        ));
    }
    source.unknowns.extend(engine_index.errors.iter().cloned());
    sort_source_findings(&mut source.peer_constructions);
    sort_source_findings(&mut source.peer_assignments);
    sort_source_findings(&mut source.authority);
    source.authority_assignments.sort_by(|left, right| {
        left.source
            .path
            .cmp(&right.source.path)
            .then_with(|| left.source.line.cmp(&right.source.line))
    });
    source.multiplayer_contexts.sort_by(|left, right| {
        left.subtree_root.cmp(&right.subtree_root).then_with(|| {
            left.source
                .as_ref()
                .map(|source| (&source.path, source.line))
                .cmp(
                    &right
                        .source
                        .as_ref()
                        .map(|source| (&source.path, source.line)),
                )
        })
    });
    source.rpc_calls.sort_by(|left, right| {
        left.source
            .path
            .cmp(&right.source.path)
            .then_with(|| left.source.line.cmp(&right.source.line))
    });
    source.lifecycle.sort_by(|left, right| {
        left.source
            .path
            .cmp(&right.source.path)
            .then_with(|| left.source.line.cmp(&right.source.line))
    });
    let endpoints = rpc_endpoints(&engine_index, &source);
    let reflected_endpoints: HashSet<_> = endpoints
        .iter()
        .map(|endpoint| (endpoint.source.path.as_str(), endpoint.method.as_str()))
        .collect();
    for ((path, method), function) in &source.functions {
        if function.rpc_line.is_some()
            && !reflected_endpoints.contains(&(path.as_str(), method.as_str()))
        {
            source.unknowns.push(format!(
                "{path}:{}: @rpc method {method} was not returned by the configured engine",
                function.rpc_line.unwrap_or(function.line)
            ));
        }
    }
    let network_paths: HashSet<_> = endpoints
        .iter()
        .map(|endpoint| endpoint.source.path.as_str())
        .chain(
            source
                .rpc_calls
                .iter()
                .map(|call| call.source.path.as_str()),
        )
        .chain(
            source
                .peer_constructions
                .iter()
                .map(|finding| finding.source.path.as_str()),
        )
        .chain(
            source
                .peer_assignments
                .iter()
                .map(|finding| finding.source.path.as_str()),
        )
        .chain(
            source
                .lifecycle
                .iter()
                .map(|finding| finding.source.path.as_str()),
        )
        .chain(
            source
                .authority
                .iter()
                .map(|finding| finding.source.path.as_str()),
        )
        .collect();
    let autoloads: Vec<NetAutoload> = gdview::Project::open(&project)?
        .autoloads()?
        .entries()
        .iter()
        .map(|autoload| {
            let resolved = engine_index
                .resolved_paths
                .get(&autoload.path)
                .and_then(Value::as_str);
            NetAutoload {
                index: autoload.index,
                name: autoload.name.clone(),
                path: autoload.path.clone(),
                resolved_path: resolved.map(str::to_owned),
                singleton: autoload.singleton,
                networked: network_paths.contains(autoload.path.as_str())
                    || resolved.is_some_and(|path| network_paths.contains(path)),
            }
        })
        .collect();
    let mut replication_nodes = replication_nodes(&project, &scenes, &mut source.unknowns);
    let mut multiplayer_contexts = vec![MultiplayerContext {
        subtree_root: "/root".into(),
        multiplayer_api: "default MultiplayerAPI".into(),
        source: None,
    }];
    multiplayer_contexts.extend(source.multiplayer_contexts.iter().cloned());
    let anchors = script_anchors(&project, &scenes, &autoloads, &mut source.unknowns);
    link_replication_contracts(
        &mut replication_nodes,
        &source.authority_assignments,
        &anchors,
    );
    let rpc_contracts = build_rpc_contracts(
        &source.rpc_calls,
        &endpoints,
        &source.functions,
        &source.authority,
        &multiplayer_contexts,
        &anchors,
    );
    let report = NetReport {
        schema_version: NET_REPORT_SCHEMA_VERSION,
        project: engine::display_path(&project),
        engine: NetEngine {
            executable: engine::display_path(&engine_path),
            version: engine_index.version,
        },
        coverage: NetCoverage {
            scripts_scanned: scripts.len(),
            scenes_scanned: scenes.len(),
            languages: vec!["GDScript".into()],
        },
        autoloads,
        multiplayer_contexts,
        rpc_endpoints: endpoints,
        rpc_calls: source.rpc_calls,
        rpc_contracts,
        peer_constructions: source.peer_constructions,
        peer_assignments: source.peer_assignments,
        lifecycle: source.lifecycle,
        authority: source.authority,
        authority_assignments: source.authority_assignments,
        replication_nodes,
        unknowns: source.unknowns,
    };
    match (explanation, output) {
        (Some(query), NetOutput::Human) => {
            if !print_explanation(&report, &query, &anchors) {
                eprintln!("no multiplayer contract matched {query}");
                return Ok(ExitCode::from(1));
            }
        }
        (Some(query), NetOutput::Json) => {
            let contracts = report
                .rpc_contracts
                .iter()
                .filter(|contract| rpc_contract_matches(contract, &query, &anchors))
                .collect::<Vec<_>>();
            let replication = report
                .replication_nodes
                .iter()
                .filter(|node| {
                    node.node_path == query
                        || node.scene == query
                        || node.node_path.ends_with(&query)
                })
                .collect::<Vec<_>>();
            if contracts.is_empty() && replication.is_empty() {
                eprintln!("no multiplayer contract matched {query}");
                return Ok(ExitCode::from(1));
            }
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "schema_version": NET_REPORT_SCHEMA_VERSION,
                    "rpc_contracts": contracts,
                    "replication_nodes": replication,
                }))?
            );
        }
        (None, NetOutput::Human) => print_human(&report),
        (None, NetOutput::Json) => println!("{}", serde_json::to_string_pretty(&report)?),
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_rpc_calls_peer_lifecycle_and_authority_without_matching_strings() {
        let source = r#"extends Node

@rpc("any_peer", "call_remote", "reliable")
func request(value: int) -> void:
	print("fake.rpc_id(8)")
	if multiplayer.is_server():
		request.rpc_id(7, value)

func setup(peer: MultiplayerPeer) -> void:
	multiplayer.peer_connected.connect(_on_peer)
	multiplayer.multiplayer_peer = peer
	var offline := OfflineMultiplayerPeer.new()
"#;
        let mut index = SourceIndex::default();
        scan_script("res://network.gd", source, &mut index);
        let function = index
            .functions
            .get(&("res://network.gd".into(), "request".into()))
            .unwrap();
        assert_eq!(function.rpc_line, Some(3));
        assert_eq!(index.rpc_calls.len(), 1);
        assert_eq!(index.rpc_calls[0].method, "request");
        assert_eq!(index.rpc_calls[0].target.as_deref(), Some("7"));
        assert_eq!(index.lifecycle.len(), 1);
        assert_eq!(index.peer_assignments.len(), 1);
        assert_eq!(index.peer_constructions.len(), 1);
        assert_eq!(index.authority.len(), 1);
    }

    #[test]
    fn converts_engine_rpc_defaults_and_source_provenance() {
        let mut source = SourceIndex::default();
        source.functions.insert(
            ("res://network.gd".into(), "request".into()),
            SourceFunction {
                signature: "func request(value: int) -> void".into(),
                line: 4,
                end_line: 5,
                rpc_line: Some(3),
            },
        );
        let engine = EngineIndex {
            version: "test".into(),
            scripts: vec![EngineScript {
                path: "res://network.gd".into(),
                rpc_config: Map::from_iter([(
                    "request".into(),
                    serde_json::json!({"rpc_mode": 1, "call_local": false, "transfer_mode": 2}),
                )]),
            }],
            errors: Vec::new(),
            resolved_paths: Map::new(),
        };
        let endpoints = rpc_endpoints(&engine, &source);
        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].rpc_mode, "any_peer");
        assert_eq!(endpoints[0].call, "call_remote");
        assert_eq!(endpoints[0].transfer_mode, "reliable");
        assert_eq!(endpoints[0].source.line, 3);
        assert!(!endpoints[0].inherited);
    }

    #[test]
    fn scans_multiplayer_contexts_and_structured_authority_assignments() {
        let source = r#"extends Node

func configure(api: MultiplayerAPI, peer_id: int) -> void:
	get_tree().set_multiplayer(api, NodePath("/root/Match"))
	$Player.set_multiplayer_authority(peer_id, false)
"#;
        let mut index = SourceIndex::default();
        scan_script("res://match.gd", source, &mut index);
        assert_eq!(index.multiplayer_contexts.len(), 1);
        assert_eq!(index.multiplayer_contexts[0].subtree_root, "/root/Match");
        assert_eq!(index.authority_assignments.len(), 1);
        assert_eq!(index.authority_assignments[0].node, "$Player");
        assert_eq!(index.authority_assignments[0].authority, "peer_id");
        assert!(!index.authority_assignments[0].recursive);
    }

    #[test]
    fn reads_spawner_synchronizer_and_replication_properties() {
        let source = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="SceneReplicationConfig" id="SceneReplicationConfig_sync"]
properties/0/path = NodePath(".:position")
properties/0/spawn = true
properties/0/replication_mode = 2

[node name="Player" type="Node3D"]

[node name="Spawner" type="MultiplayerSpawner" parent="."]
spawn_path = NodePath("../Players")
spawn_limit = 10
_spawnable_scenes = PackedStringArray("res://player.tscn")

[node name="Sync" type="MultiplayerSynchronizer" parent="."]
root_path = NodePath("..")
replication_config = SubResource("SceneReplicationConfig_sync")
"#;
        let mut nodes = vec![
            ReplicationNode {
                kind: "MultiplayerSpawner".into(),
                node_path: "Spawner".into(),
                scene: "res://match.tscn".into(),
                root_path: None,
                spawn_path: None,
                spawn_limit: None,
                spawnable_scenes: Vec::new(),
                replication_properties: Vec::new(),
                related_nodes: Vec::new(),
                authority_assignments: Vec::new(),
            },
            ReplicationNode {
                kind: "MultiplayerSynchronizer".into(),
                node_path: "Sync".into(),
                scene: "res://match.tscn".into(),
                root_path: None,
                spawn_path: None,
                spawn_limit: None,
                spawnable_scenes: Vec::new(),
                replication_properties: Vec::new(),
                related_nodes: Vec::new(),
                authority_assignments: Vec::new(),
            },
        ];
        enrich_replication_nodes(source, &mut nodes);
        assert_eq!(nodes[0].spawn_path.as_deref(), Some("../Players"));
        assert_eq!(nodes[0].spawn_limit, Some(10));
        assert_eq!(nodes[0].spawnable_scenes, ["res://player.tscn"]);
        assert_eq!(nodes[1].root_path.as_deref(), Some(".."));
        assert_eq!(nodes[1].replication_properties.len(), 1);
        assert_eq!(nodes[1].replication_properties[0].path, ".:position");
        assert_eq!(nodes[1].replication_properties[0].mode, "on_change");
    }
}
