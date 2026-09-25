use crate::{
    cli::{NetOutput, ResourceArgs, ResourceCommand, ResourceCreateArgs, ResourceSchemaArgs},
    engine, process,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    error::Error,
    fs,
    path::{Component, Path, PathBuf},
    process::{Command, ExitCode},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    class: Option<String>,
    script: Option<String>,
    properties: serde_json::Map<String, Value>,
}

struct Workspace(PathBuf);
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn local_path(project: &Path, value: &str) -> Result<PathBuf, Box<dyn Error>> {
    let relative = value
        .strip_prefix("res://")
        .ok_or("path must start with res://")?;
    if relative.is_empty()
        || relative.contains(['\\', ':'])
        || relative
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || Path::new(relative)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("path must be a project-local resource path without traversal".into());
    }
    let path = project.join(relative);
    let mut ancestor = path.as_path();
    while !ancestor.exists() {
        ancestor = ancestor.parent().ok_or("invalid resource path")?;
    }
    if !fs::canonicalize(ancestor)?.starts_with(project) {
        return Err("resource path resolves outside project".into());
    }
    Ok(path)
}

fn validate_type(
    project: &Path,
    class: &Option<String>,
    script: &Option<String>,
) -> Result<(), Box<dyn Error>> {
    match (class, script) {
        (Some(name), None) if !name.is_empty() => {}
        (None, Some(path)) => {
            local_path(project, path)?;
            if !path.ends_with(".gd") {
                return Err("script must be a .gd resource path".into());
            }
        }
        _ => return Err("spec requires exactly one nonempty class or script".into()),
    }
    Ok(())
}

fn create(args: ResourceCreateArgs) -> Result<Value, Box<dyn Error>> {
    let project = engine::project_root(&args.project)?;
    let destination = local_path(&project, &args.out)?;
    if destination.extension().and_then(|s| s.to_str()) != Some("tres") {
        return Err("destination must end in .tres".into());
    }
    if fs::symlink_metadata(&destination).is_ok() {
        return Err("destination already exists".into());
    }
    let parent = destination.parent().ok_or("invalid destination")?;
    if !parent.is_dir() {
        return Err("destination parent directory must exist".into());
    }
    let text = fs::read_to_string(&args.spec)?;
    let spec: Spec = serde_json::from_str(&text)?;
    if let Err((field, message)) = validate_spec(&project, &spec, "", 0) {
        return Ok(json!({"status":"error", "stage":"validate", "field":field, "message":message}));
    }
    let (workspace, mut result) = worker(&project, args.godot.as_deref(), parent, &text, "create")?;
    if result["status"] != "created" {
        return Ok(result);
    }
    if let Err(error) = fs::hard_link(workspace.0.join("resource.tres"), &destination) {
        return Ok(json!({"status": "error", "stage": "publish", "field": "",
            "message": error.to_string()}));
    }
    result["path"] = json!(args.out);
    Ok(result)
}

fn validate_spec(
    project: &Path,
    spec: &Spec,
    prefix: &str,
    depth: usize,
) -> Result<(), (String, String)> {
    if depth > 16 {
        return Err((
            prefix.to_owned(),
            "Resource nesting exceeds limit of 16".to_owned(),
        ));
    }
    validate_type(project, &spec.class, &spec.script)
        .map_err(|error| (prefix.to_owned(), error.to_string()))?;
    for (name, value) in &spec.properties {
        let field = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}.properties.{name}")
        };
        validate_value(project, value, &field, depth)?;
    }
    Ok(())
}

fn validate_value(
    project: &Path,
    value: &Value,
    field: &str,
    depth: usize,
) -> Result<(), (String, String)> {
    let field = field.to_owned();
    match value {
        Value::Array(entries) => {
            for (index, entry) in entries.iter().enumerate() {
                let location = format!("{field}[{index}]");
                if !entry.is_null()
                    && !entry.as_object().is_some_and(|object| {
                        object.len() == 1
                            && (object.contains_key("$ref") || object.contains_key("$resource"))
                    })
                {
                    return Err((
                        location,
                        "Expected null, $ref, or $resource array entry".to_owned(),
                    ));
                }
                validate_value(project, entry, &location, depth)?;
            }
        }
        Value::Null | Value::Bool(_) | Value::String(_) => {}
        Value::Number(number) => {
            if (number.is_i64() || number.is_u64())
                && number
                    .as_f64()
                    .is_none_or(|v| v.abs() > 9_007_199_254_740_991.0)
            {
                return Err((
                    field,
                    "Integer exceeds exact JSON transport range".to_owned(),
                ));
            }
        }
        Value::Object(object) if object.len() == 1 && object.contains_key("$variant") => {
            validate_payload(
                project,
                &object["$variant"],
                &format!("{field}.$variant"),
                depth,
            )?;
        }
        Value::Object(object) if object.len() == 1 && object.contains_key("$ref") => {
            let path = object["$ref"].as_str().ok_or_else(|| {
                (
                    field.clone(),
                    "$ref must be a res:// path string".to_owned(),
                )
            })?;
            local_path(project, path).map_err(|error| (field, error.to_string()))?;
        }
        Value::Object(object) if object.len() == 1 && object.contains_key("$resource") => {
            let nested: Spec = serde_json::from_value(object["$resource"].clone())
                .map_err(|error| (field.clone(), error.to_string()))?;
            let nested_prefix = if field.starts_with("properties.") {
                field
            } else {
                format!("properties.{field}")
            };
            validate_spec(project, &nested, &nested_prefix, depth + 1)?;
        }
        _ => {
            return Err((
                field,
                "Expected a scalar, null, $ref, or $resource; arbitrary objects are unsupported"
                    .to_owned(),
            ));
        }
    }
    Ok(())
}

fn validate_payload(
    project: &Path,
    value: &Value,
    field: &str,
    depth: usize,
) -> Result<(), (String, String)> {
    if depth > 64 {
        return Err((
            field.to_owned(),
            "Variant payload nesting exceeds limit of 64".to_owned(),
        ));
    }
    match value {
        Value::Number(_) => validate_value(project, value, field, depth)?,
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                validate_payload(project, item, &format!("{field}[{index}]"), depth + 1)?;
            }
        }
        Value::Object(fields) => {
            for (name, item) in fields {
                validate_payload(project, item, &format!("{field}.{name}"), depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn schema(args: ResourceSchemaArgs) -> Result<Value, Box<dyn Error>> {
    let project = engine::project_root(&args.project)?;
    validate_type(&project, &args.class, &args.script)?;
    let mut spec = json!({});
    if let Some(class) = args.class {
        spec["class"] = json!(class);
    }
    if let Some(script) = args.script {
        spec["script"] = json!(script);
    }
    let (_, result) = worker(
        &project,
        args.godot.as_deref(),
        &std::env::temp_dir(),
        &serde_json::to_string(&spec)?,
        "schema",
    )?;
    Ok(result)
}

fn worker(
    project: &Path,
    godot: Option<&Path>,
    parent: &Path,
    text: &str,
    operation: &str,
) -> Result<(Workspace, Value), Box<dyn Error>> {
    let executable = engine::resolve(project, godot)?;
    engine::validated_version(&executable, project)?;
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let workspace =
        Workspace(parent.join(format!(".gdkit-resource-{}-{nonce}", std::process::id())));
    fs::create_dir(&workspace.0)?;
    let script = workspace.0.join("worker.gd");
    let request = workspace.0.join("request.json");
    let staged = workspace.0.join("resource.tres");
    fs::write(&script, include_bytes!("resource.gd"))?;
    fs::write(&request, text)?;
    let captured = process::run(
        Command::new(executable)
            .args(["--headless", "--no-header", "--path"])
            .arg(project)
            .arg("--script")
            .arg(&script)
            .arg("--")
            .arg(&request)
            .arg(operation)
            .arg(&staged),
        Some(Duration::from_secs(60)),
    )?;
    let stdout = String::from_utf8_lossy(&captured.output.stdout);
    let stderr = String::from_utf8_lossy(&captured.output.stderr);
    let mut result: Value = stdout
        .lines()
        .find_map(|line| line.strip_prefix("GDKIT_RESOURCE_RESULT:"))
        .map(serde_json::from_str)
        .transpose()?
        .unwrap_or_else(|| {
            json!({"status": "error", "stage": "construct", "field": "",
            "message": format!("Resource worker did not complete\n{stdout}{stderr}")})
        });
    if captured.timed_out
        || !captured.output.status.success()
        || result["status"]
            != if operation == "schema" {
                "schema"
            } else {
                "created"
            }
        || stderr.contains("SCRIPT ERROR:")
        || stderr.contains("ERROR:")
        || stderr.contains("WARNING:")
        || stdout.lines().any(|line| {
            let line = line.trim_start();
            line.starts_with("ERROR:")
                || line.starts_with("SCRIPT ERROR:")
                || line.starts_with("WARNING:")
        })
    {
        if result["status"] == "error" {
            if !stderr.trim().is_empty() {
                result["diagnostics"] = json!(stderr);
            }
            return Ok((workspace, result));
        }
        return Ok((
            workspace,
            json!({"status": "error", "stage": if operation == "schema" {"schema"} else {"verify"}, "field": "",
            "message": format!("Resource worker failed\n{stdout}{stderr}")}),
        ));
    }
    if !stderr.trim().is_empty() {
        eprint!("{stderr}");
    }
    Ok((workspace, result))
}

pub(crate) fn run(args: ResourceArgs) -> Result<ExitCode, Box<dyn Error>> {
    let (output, result) = match args.command {
        ResourceCommand::Create(args) => (args.output, create(args)),
        ResourceCommand::Schema(args) => (args.output, schema(args)),
    };
    let result = result.unwrap_or_else(|error| json!({"status": "error", "stage": "prepare", "field": "", "message": error.to_string()}));
    match output {
        NetOutput::Json => println!("{}", serde_json::to_string(&result)?),
        NetOutput::Human => {
            if result["status"] == "created" {
                println!(
                    "Created {} ({})\nVerified properties: {}",
                    result["path"].as_str().unwrap_or_default(),
                    result["type"].as_str().unwrap_or_default(),
                    result["properties"]
                );
                if let Some(script) = result["script"].as_str() {
                    println!("Script: {script}");
                }
            } else if result["status"] == "schema" {
                println!(
                    "Resource schema: {}",
                    result["type"].as_str().unwrap_or_default()
                );
                if let Some(script) = result["script"].as_str() {
                    println!("Script: {script}");
                }
                println!(
                    "Discovery executes constructors and getters, including project scripts when selected."
                );
                for field in result["fields"].as_array().into_iter().flatten() {
                    println!(
                        "{}: {} = {} [{}]",
                        field["name"].as_str().unwrap_or_default(),
                        field["type"].as_str().unwrap_or_default(),
                        field["default"]["value"],
                        if field["create_supported"] == true {
                            "create supported"
                        } else {
                            field["unsupported_reason"].as_str().unwrap_or_default()
                        }
                    );
                    if !field["hint_string"].as_str().unwrap_or_default().is_empty() {
                        println!(
                            "  Hint {}: {}; choices: {}",
                            field["hint"], field["hint_string"], field["enum_choices"]
                        );
                    }
                }
            } else {
                eprintln!(
                    "Resource operation failed [{}] {}: {}",
                    result["stage"].as_str().unwrap_or_default(),
                    result["field"].as_str().unwrap_or_default(),
                    result["message"].as_str().unwrap_or_default()
                );
                if let Some(diagnostics) = result["diagnostics"].as_str() {
                    eprint!("{diagnostics}");
                }
            }
        }
    }
    Ok(
        if result["status"] == "created" || result["status"] == "schema" {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        },
    )
}
