use std::{
    collections::{BTreeSet, HashMap},
    error::Error,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

use serde::Serialize;
use serde_json::Value;

use crate::{
    check,
    cli::{AnimationArgs, AnimationCommand, AnimationInspectArgs, AnimationListArgs, NetOutput},
    engine,
};

const GLB_MAGIC: &[u8; 4] = b"glTF";
const GLB_VERSION: u32 = 2;
const JSON_CHUNK: u32 = 0x4e4f_534a;
const INSPECT_RESULT_PREFIX: &str = "GDKIT_ANIMATION_RESULT:";

#[derive(Debug, PartialEq, Serialize)]
struct AnimationReport {
    schema_version: u32,
    file: PathBuf,
    animations: Vec<PackedAnimation>,
}

#[derive(Debug, PartialEq, Serialize)]
struct PackedAnimation {
    index: usize,
    name: Option<String>,
    duration_seconds: Option<f64>,
    channels: usize,
    samplers: usize,
    target_nodes: usize,
    target_properties: Vec<String>,
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, Box<dyn Error>> {
    let value = bytes
        .get(offset..offset + 4)
        .ok_or("truncated GLB header or chunk")?;
    Ok(u32::from_le_bytes(value.try_into()?))
}

fn json_chunk(bytes: &[u8]) -> Result<&[u8], Box<dyn Error>> {
    if bytes.len() < 20 {
        return Err("file is too short to be a GLB".into());
    }
    if bytes.get(..4) != Some(GLB_MAGIC) {
        return Err("invalid GLB magic; expected binary glTF".into());
    }
    let version = read_u32(bytes, 4)?;
    if version != GLB_VERSION {
        return Err(format!("unsupported GLB version {version}; expected version 2").into());
    }
    let declared_length = usize::try_from(read_u32(bytes, 8)?)?;
    if declared_length != bytes.len() {
        return Err(format!(
            "GLB length mismatch: header declares {declared_length} bytes but file contains {}",
            bytes.len()
        )
        .into());
    }
    let chunk_length = usize::try_from(read_u32(bytes, 12)?)?;
    let chunk_type = read_u32(bytes, 16)?;
    if chunk_type != JSON_CHUNK {
        return Err("the first GLB chunk is not JSON".into());
    }
    let end = 20usize
        .checked_add(chunk_length)
        .ok_or("GLB JSON chunk length overflow")?;
    bytes
        .get(20..end)
        .ok_or_else(|| "truncated GLB JSON chunk".into())
}

fn number_at(value: &Value, field: &str) -> Option<f64> {
    value.get(field)?.as_array()?.first()?.as_f64()
}

fn animation_duration(
    animation: &Value,
    accessors: &[Value],
) -> Result<Option<f64>, Box<dyn Error>> {
    let Some(samplers) = animation.get("samplers").and_then(Value::as_array) else {
        return Ok(None);
    };
    let mut start: Option<f64> = None;
    let mut end: Option<f64> = None;
    for sampler in samplers {
        let input = sampler
            .get("input")
            .and_then(Value::as_u64)
            .ok_or("animation sampler is missing an input accessor")?;
        let input = usize::try_from(input)?;
        let accessor = accessors
            .get(input)
            .ok_or_else(|| format!("animation sampler references missing accessor {input}"))?;
        let (Some(minimum), Some(maximum)) =
            (number_at(accessor, "min"), number_at(accessor, "max"))
        else {
            continue;
        };
        start = Some(start.map_or(minimum, |current| current.min(minimum)));
        end = Some(end.map_or(maximum, |current| current.max(maximum)));
    }
    Ok(start.zip(end).map(|(start, end)| {
        let duration = (end - start).max(0.0);
        (duration * 1_000_000_000.0).round() / 1_000_000_000.0
    }))
}

fn parse(path: &Path, bytes: &[u8]) -> Result<AnimationReport, Box<dyn Error>> {
    let json = json_chunk(bytes)?;
    let document: Value = serde_json::from_slice(json)?;
    let accessors = document
        .get("accessors")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let animations = document
        .get("animations")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut packed = Vec::with_capacity(animations.len());
    for (index, animation) in animations.iter().enumerate() {
        let channels = animation
            .get("channels")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let samplers = animation
            .get("samplers")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        let mut target_nodes = BTreeSet::new();
        let mut target_properties = BTreeSet::new();
        for channel in channels {
            let Some(target) = channel.get("target") else {
                continue;
            };
            if let Some(node) = target.get("node").and_then(Value::as_u64) {
                target_nodes.insert(node);
            }
            if let Some(property) = target.get("path").and_then(Value::as_str) {
                target_properties.insert(property.to_owned());
            }
        }
        packed.push(PackedAnimation {
            index,
            name: animation
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_owned),
            duration_seconds: animation_duration(animation, accessors)?,
            channels: channels.len(),
            samplers,
            target_nodes: target_nodes.len(),
            target_properties: target_properties.into_iter().collect(),
        });
    }
    Ok(AnimationReport {
        schema_version: 1,
        file: path.to_owned(),
        animations: packed,
    })
}

fn display_name(animation: &PackedAnimation) -> String {
    animation
        .name
        .clone()
        .unwrap_or_else(|| format!("<unnamed #{}>", animation.index + 1))
}

fn print_human(report: &AnimationReport, names_only: bool) {
    if names_only {
        for animation in &report.animations {
            println!("{}", display_name(animation));
        }
        return;
    }
    let label = report
        .file
        .file_name()
        .unwrap_or(report.file.as_os_str())
        .to_string_lossy();
    println!(
        "{label}\n{} animation{}",
        report.animations.len(),
        if report.animations.len() == 1 {
            ""
        } else {
            "s"
        }
    );
    if report.animations.is_empty() {
        return;
    }
    let width = report
        .animations
        .iter()
        .map(|animation| display_name(animation).chars().count())
        .chain([4])
        .max()
        .unwrap_or(4);
    println!(
        "\n{:<width$}  {:>8}  {:>8}  {:>7}",
        "NAME", "DURATION", "CHANNELS", "TARGETS"
    );
    for animation in &report.animations {
        let duration = animation
            .duration_seconds
            .map_or_else(|| "-".to_owned(), |value| format!("{value:.3}s"));
        println!(
            "{:<width$}  {:>8}  {:>8}  {:>7}",
            display_name(animation),
            duration,
            animation.channels,
            animation.target_nodes
        );
    }
}

fn list(args: AnimationListArgs) -> Result<ExitCode, Box<dyn Error>> {
    if !args
        .path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("glb"))
    {
        return Err(format!("{} is not a .glb file", args.path.display()).into());
    }
    let bytes = fs::read(&args.path)
        .map_err(|error| format!("could not read {}: {error}", args.path.display()))?;
    let mut report = parse(&args.path, &bytes)
        .map_err(|error| format!("could not inspect {}: {error}", args.path.display()))?;
    if let Some(filter) = args.filter {
        let filter = filter.to_lowercase();
        report
            .animations
            .retain(|animation| display_name(animation).to_lowercase().contains(&filter));
    }
    match args.output {
        NetOutput::Human => print_human(&report, args.names),
        NetOutput::Json => println!("{}", serde_json::to_string_pretty(&report)?),
    }
    Ok(ExitCode::SUCCESS)
}

struct TemporaryFile(PathBuf);

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn temporary_script() -> Result<TemporaryFile, Box<dyn Error>> {
    for attempt in 0..100 {
        let path = std::env::temp_dir().join(format!(
            "gdkit-animation-inspect-{}-{attempt}.gd",
            std::process::id()
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                file.write_all(include_bytes!("animation_inspect.gd"))?;
                return Ok(TemporaryFile(path));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err("could not create temporary animation inspector".into())
}

fn scene_resource_path(project: &Path, scene: &str) -> Result<(String, PathBuf), Box<dyn Error>> {
    let path = if let Some(relative) = scene.strip_prefix("res://") {
        if relative.is_empty()
            || relative.contains(['\\', ':'])
            || relative
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err("scene must be a project-local path without traversal".into());
        }
        project.join(relative)
    } else {
        let path = Path::new(scene);
        if path.is_absolute() {
            path.to_owned()
        } else {
            project.join(path)
        }
    };
    let path = fs::canonicalize(&path)
        .map_err(|error| format!("could not resolve scene {}: {error}", path.display()))?;
    if !path.starts_with(project) {
        return Err("scene resolves outside the selected Godot project".into());
    }
    let relative = path
        .strip_prefix(project)?
        .to_string_lossy()
        .replace('\\', "/");
    Ok((format!("res://{relative}"), path))
}

fn quoted_attribute(line: &str, name: &str) -> Option<String> {
    let marker = format!(r#"{name}="#);
    let start = line.find(&marker)? + marker.len();
    let rest = line.get(start..)?;
    if let Some(rest) = rest.strip_prefix('"') {
        return Some(rest.split('"').next()?.to_owned());
    }
    Some(
        rest.split([' ', ']'])
            .next()
            .filter(|value| !value.is_empty())?
            .to_owned(),
    )
}

#[derive(Default)]
struct SourceLocations {
    tree_lines: HashMap<String, usize>,
    resource_lines: HashMap<String, usize>,
}

fn source_locations(source: &str) -> SourceLocations {
    let mut locations = SourceLocations::default();
    for (index, line) in source.lines().enumerate() {
        let line_number = index + 1;
        if line.starts_with("[sub_resource ") {
            if let Some(id) = quoted_attribute(line, "id") {
                locations.resource_lines.insert(id, line_number);
            }
        } else if line.starts_with("[node ")
            && quoted_attribute(line, "type").as_deref() == Some("AnimationTree")
            && let Some(name) = quoted_attribute(line, "name")
        {
            locations.tree_lines.insert(name, line_number);
        }
    }
    locations
}

fn annotate_graph(value: &mut Value, locations: &SourceLocations) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    if let Some(resource_path) = object.get("resource_path").and_then(Value::as_str)
        && let Some(id) = resource_path.rsplit("::").next()
        && let Some(line) = locations.resource_lines.get(id)
    {
        object.insert("source_line".into(), (*line).into());
    }
    if let Some(children) = object.get_mut("children").and_then(Value::as_array_mut) {
        for child in children {
            annotate_graph(child, locations);
        }
    }
}

fn annotate_source(report: &mut Value, source_path: &Path, source: Option<&str>) {
    let mut source_report = serde_json::json!({"path": engine::display_path(source_path)});
    let Some(source) = source else {
        source_report["text_scene"] = false.into();
        report["source"] = source_report;
        return;
    };
    source_report["text_scene"] = true.into();
    let locations = source_locations(source);
    if let Some(trees) = report.get_mut("trees").and_then(Value::as_array_mut) {
        for tree in trees {
            let name = tree
                .get("path")
                .and_then(Value::as_str)
                .and_then(|path| path.rsplit('/').next())
                .unwrap_or_default();
            if let Some(line) = locations.tree_lines.get(name) {
                tree["source_line"] = (*line).into();
            }
            if let Some(graph) = tree.get_mut("graph") {
                annotate_graph(graph, &locations);
            }
        }
    }
    report["source"] = source_report;
}

fn graph_label(node: &Value) -> String {
    let path = node.get("path").and_then(Value::as_str).unwrap_or("root");
    let kind = node
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("AnimationNode");
    let animation = node.get("animation").and_then(Value::as_str);
    let found = node
        .get("animation_found")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    match animation {
        Some(animation) => format!(
            "{path}: {kind} -> {animation} [{}]",
            if found { "found" } else { "missing" }
        ),
        None => format!("{path}: {kind}"),
    }
}

fn print_graph(node: &Value, indent: usize) {
    println!("{}{}", " ".repeat(indent), graph_label(node));
    if let Some(transitions) = node.get("transitions").and_then(Value::as_array) {
        for transition in transitions {
            let mode = match transition.get("advance_mode").and_then(Value::as_i64) {
                Some(0) => "disabled",
                Some(1) => "enabled",
                Some(2) => "auto",
                _ => "unknown",
            };
            let condition = transition
                .get("advance_condition")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let suffix = if condition.is_empty() {
                mode.to_owned()
            } else {
                format!("{mode}, condition {condition}")
            };
            println!(
                "{}{} -> {} [{suffix}]",
                " ".repeat(indent + 2),
                transition
                    .get("from")
                    .and_then(Value::as_str)
                    .unwrap_or("?"),
                transition.get("to").and_then(Value::as_str).unwrap_or("?")
            );
        }
    }
    if let Some(children) = node.get("children").and_then(Value::as_array) {
        for child in children {
            print_graph(child, indent + 2);
        }
    }
}

fn report_findings(report: &Value) -> Vec<&Value> {
    let mut findings = Vec::new();
    if let Some(entries) = report.get("findings").and_then(Value::as_array) {
        findings.extend(entries);
    }
    if let Some(trees) = report.get("trees").and_then(Value::as_array) {
        for tree in trees {
            if let Some(entries) = tree.get("findings").and_then(Value::as_array) {
                findings.extend(entries);
            }
        }
    }
    findings
}

fn print_inspection(report: &Value) {
    println!(
        "{}\nengine: {} ({})",
        report
            .get("scene")
            .and_then(Value::as_str)
            .unwrap_or("scene"),
        report
            .pointer("/engine/executable")
            .and_then(Value::as_str)
            .unwrap_or("unknown"),
        report
            .pointer("/engine/version")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
    );
    if let Some(trees) = report.get("trees").and_then(Value::as_array) {
        for tree in trees {
            let path = tree.get("path").and_then(Value::as_str).unwrap_or(".");
            let line = tree
                .get("source_line")
                .and_then(Value::as_u64)
                .map_or_else(String::new, |line| format!(" (source line {line})"));
            println!("\nAnimationTree {path}{line}");
            println!(
                "  active: {}",
                tree.get("active").and_then(Value::as_bool).unwrap_or(false)
            );
            println!(
                "  root node: {} [{}]",
                tree.pointer("/root_node/path")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
                if tree
                    .pointer("/root_node/resolved")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    "resolved"
                } else {
                    "missing"
                }
            );
            println!(
                "  animation player: {} [{}]",
                tree.pointer("/animation_player/path")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
                if tree
                    .pointer("/animation_player/resolved")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    "resolved"
                } else {
                    "missing"
                }
            );
            let animations = tree
                .get("animations")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default();
            println!("  animations: {}", animations.len());
            for animation in animations {
                println!(
                    "    {}  {:.3}s  {} tracks  {} invalid targets  {} invalid bones",
                    animation.get("name").and_then(Value::as_str).unwrap_or(""),
                    animation
                        .get("length")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0),
                    animation
                        .get("track_count")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                    animation
                        .get("invalid_track_targets")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                    animation
                        .get("invalid_bones")
                        .and_then(Value::as_u64)
                        .unwrap_or(0)
                );
            }
            let parameters = tree
                .get("parameters")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default();
            println!("  parameters: {}", parameters.len());
            for parameter in parameters {
                println!(
                    "    {} = {}",
                    parameter.get("name").and_then(Value::as_str).unwrap_or(""),
                    parameter
                        .get("value")
                        .map(Value::to_string)
                        .unwrap_or_else(|| "null".into())
                );
            }
            if let Some(graph) = tree.get("graph")
                && !graph.is_null()
            {
                println!("  graph:");
                print_graph(graph, 4);
            }
        }
    }
    let findings = report_findings(report);
    if !findings.is_empty() {
        println!("\nFindings:");
        for finding in findings {
            println!(
                "  {} [{}] {}{}",
                finding
                    .get("severity")
                    .and_then(Value::as_str)
                    .unwrap_or("info"),
                finding
                    .get("code")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown"),
                finding.get("message").and_then(Value::as_str).unwrap_or(""),
                finding
                    .get("path")
                    .and_then(Value::as_str)
                    .filter(|path| !path.is_empty())
                    .map_or_else(String::new, |path| format!(" ({path})"))
            );
        }
    }
}

fn inspect(args: AnimationInspectArgs) -> Result<ExitCode, Box<dyn Error>> {
    let project = engine::project_root(&args.project)?;
    let (scene, source_path) = scene_resource_path(&project, &args.scene)?;
    let engine_path = engine::resolve(&project, args.godot.as_deref())?;
    let (version, _) = engine::validated_version(&engine_path, &project)?;
    let script = temporary_script()?;
    let mut command = Command::new(&engine_path);
    command
        .args(["--headless", "--no-header", "--path"])
        .arg(&project)
        .arg("--script")
        .arg(&script.0)
        .arg("--")
        .arg(&scene);
    if let Some(tree) = &args.tree {
        command.arg(tree);
    }
    let output = command.output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut report = stdout
        .lines()
        .find_map(|line| line.strip_prefix(INSPECT_RESULT_PREFIX))
        .map(serde_json::from_str::<Value>)
        .transpose()?
        .ok_or_else(|| {
            format!(
                "configured engine did not return an animation inspection\n{}{}",
                stdout,
                String::from_utf8_lossy(&output.stderr)
            )
        })?;
    if !output.status.success() || check::has_errors(&output) || report.get("error").is_some() {
        return Err(format!(
            "configured engine could not inspect the animation scene\n{}{}",
            stdout,
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    report["engine"] = serde_json::json!({
        "version": version,
        "executable": engine::display_path(&engine_path),
    });
    let source = fs::read_to_string(&source_path).ok();
    annotate_source(&mut report, &source_path, source.as_deref());
    let failed = report_findings(&report)
        .iter()
        .any(|finding| finding.get("severity").and_then(Value::as_str) == Some("error"));
    match args.output {
        NetOutput::Human => print_inspection(&report),
        NetOutput::Json => println!("{}", serde_json::to_string_pretty(&report)?),
    }
    Ok(if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

pub(crate) fn run(args: AnimationArgs) -> Result<ExitCode, Box<dyn Error>> {
    match args.command {
        AnimationCommand::List(args) => list(args),
        AnimationCommand::Inspect(args) => inspect(args),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glb(document: &str) -> Vec<u8> {
        let mut json = document.as_bytes().to_vec();
        while !json.len().is_multiple_of(4) {
            json.push(b' ');
        }
        let length = 20 + json.len();
        let mut bytes = Vec::with_capacity(length);
        bytes.extend_from_slice(GLB_MAGIC);
        bytes.extend_from_slice(&GLB_VERSION.to_le_bytes());
        bytes.extend_from_slice(&u32::try_from(length).unwrap().to_le_bytes());
        bytes.extend_from_slice(&u32::try_from(json.len()).unwrap().to_le_bytes());
        bytes.extend_from_slice(&JSON_CHUNK.to_le_bytes());
        bytes.extend_from_slice(&json);
        bytes
    }

    #[test]
    fn reads_names_durations_and_target_counts() {
        let bytes = glb(
            r#"{"asset":{"version":"2.0"},"accessors":[{"min":[0.25],"max":[1.75]},{"min":[0.0],"max":[2.5]}],"animations":[{"name":"Run","samplers":[{"input":0},{"input":1}],"channels":[{"target":{"node":3,"path":"rotation"}},{"target":{"node":3,"path":"translation"}},{"target":{"node":4,"path":"rotation"}}]},{"samplers":[],"channels":[]}]}"#,
        );
        let report = parse(Path::new("model.glb"), &bytes).unwrap();
        assert_eq!(report.animations.len(), 2);
        assert_eq!(report.animations[0].name.as_deref(), Some("Run"));
        assert_eq!(report.animations[0].duration_seconds, Some(2.5));
        assert_eq!(report.animations[0].channels, 3);
        assert_eq!(report.animations[0].target_nodes, 2);
        assert_eq!(
            report.animations[0].target_properties,
            ["rotation", "translation"]
        );
        assert_eq!(display_name(&report.animations[1]), "<unnamed #2>");
    }

    #[test]
    fn rejects_invalid_containers_and_accessor_references() {
        assert!(json_chunk(b"not a glb").is_err());
        let bytes = glb(r#"{"asset":{"version":"2.0"},"animations":[{"samplers":[{"input":4}]}]}"#);
        assert!(parse(Path::new("broken.glb"), &bytes).is_err());
    }

    #[test]
    fn locates_authored_animation_tree_resources() {
        let source = r#"[gd_scene format=3]

[sub_resource type="AnimationNodeAnimation" id="AnimationNodeAnimation_run"]
animation = &"Run"

[node name="AnimationTree" type="AnimationTree" parent="."]
"#;
        let locations = source_locations(source);
        assert_eq!(locations.resource_lines["AnimationNodeAnimation_run"], 3);
        assert_eq!(locations.tree_lines["AnimationTree"], 6);
    }
}
