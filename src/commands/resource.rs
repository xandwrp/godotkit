//! `resource schema`: workspace → engine → gdproject::resource::schema → emit.
//! `resource create`: read spec JSON → CreateSpec::from_json → workspace → engine
//! → gdproject::resource::create → emit CreateReport. Verify/publish failures are
//! `Err` (exit 2): nothing was written, and that is a tool outcome, not a project verdict.

use std::io::Write;
use std::path::Path;

use gdproject::diagnostics::Severity;
use gdproject::resource::{self, DEFAULT_DEADLINE, FieldSchema, ResourceSchema};
use gdview::ResPath;
use gdview::property::TypeSchema;
use gdview::variant::{ResourceTarget, VariantType};

use crate::cli::*;
use crate::context::Context;
use crate::render::{self, Exit, Human};

pub fn run(ctx: &Context, command: ResourceCommand) -> gdproject::Result<Exit> {
    match command {
        ResourceCommand::Schema {
            project,
            class,
            script,
        } => schema(ctx, &project, class, script),
        ResourceCommand::Create { .. } => todo!(),
    }
}

fn schema(
    ctx: &Context,
    project: &ProjectArgs,
    class: Option<String>,
    script: Option<String>,
) -> gdproject::Result<Exit> {
    let workspace = ctx.workspace(project)?;
    let target = match (class, script) {
        (Some(class), _) => ResourceTarget::Class(class),
        (None, Some(script)) => ResourceTarget::Script(script_path(&workspace, &script)?),
        (None, None) => unreachable!("clap requires --class or --script"),
    };
    let engine = ctx.engine(&workspace, project)?;
    let schema = resource::schema(&workspace, &engine, &target, DEFAULT_DEADLINE)?;
    render::write(ctx.output, &SchemaReport(schema))?;
    Ok(Exit::Ok)
}

/// `res://…` as given; anything else is a path inside the project.
fn script_path(workspace: &gdproject::Workspace, script: &str) -> gdproject::Result<ResPath> {
    Ok(if script.starts_with("res://") {
        ResPath::parse(script)?
    } else {
        workspace.project().localize(Path::new(script))?
    })
}

#[derive(serde::Serialize)]
#[serde(transparent)]
struct SchemaReport(ResourceSchema);

impl Human for SchemaReport {
    fn human(&self, out: &mut dyn Write) -> std::io::Result<()> {
        let schema = &self.0;
        match (&schema.target, &schema.script_class) {
            (ResourceTarget::Class(class), _) => writeln!(out, "{class}")?,
            (ResourceTarget::Script(script), Some(name)) => {
                writeln!(out, "{name} ({script}, extends {})", schema.class)?
            }
            (ResourceTarget::Script(script), None) => {
                writeln!(out, "{script} (extends {})", schema.class)?
            }
        }
        let rows: Vec<(String, String, String)> = schema
            .fields
            .iter()
            .map(|field| (field.name.clone(), field_type(field), details(field)))
            .collect();
        let name_width = rows.iter().map(|row| row.0.len()).max().unwrap_or(0);
        let type_width = rows.iter().map(|row| row.1.len()).max().unwrap_or(0);
        for (name, ty, details) in rows {
            let line = format!("  {name:name_width$}  {ty:type_width$}  {details}");
            writeln!(out, "{}", line.trim_end())?;
        }
        let count = |severity| {
            schema
                .engine_diagnostics
                .iter()
                .filter(|d| d.severity == severity)
                .count()
        };
        let (errors, warnings) = (count(Severity::Error), count(Severity::Warning));
        if errors + warnings > 0 {
            writeln!(
                out,
                "engine: {errors} error(s), {warnings} warning(s) while loading (see --output json)"
            )?;
        }
        Ok(())
    }
}

fn type_name(variant_type: VariantType, class_name: Option<&str>) -> String {
    match (variant_type, class_name) {
        (VariantType::Object, Some(class)) => class.to_owned(),
        (VariantType::Nil, _) => "Variant".into(),
        (other, _) => other.name().into(),
    }
}

fn element_name(ty: &TypeSchema) -> String {
    type_name(ty.variant_type, ty.class_name.as_deref())
}

fn field_type(field: &FieldSchema) -> String {
    if let Some(enum_name) = &field.enum_name {
        return enum_name.clone();
    }
    match (&field.element, &field.key, &field.value) {
        (Some(element), _, _) => format!("Array[{}]", element_name(element)),
        (_, Some(key), Some(value)) => {
            format!("Dictionary[{}, {}]", element_name(key), element_name(value))
        }
        _ => type_name(field.variant_type, field.class_name.as_deref()),
    }
}

fn details(field: &FieldSchema) -> String {
    if let Some(why) = &field.unsupported {
        return format!("unsupported: {why}");
    }
    let mut parts = vec![format!("= {}", field.default)];
    if !field.enum_choices.is_empty() {
        let choices: Vec<String> = field
            .enum_choices
            .iter()
            .map(|choice| format!("{}={}", choice.name, choice.value))
            .collect();
        parts.push(format!("{{{}}}", choices.join(", ")));
    }
    match (&field.hint, &field.hint_string) {
        (Some(hint), _) if hint == "enum" || hint == "flags" || hint == "type_string" => {}
        (Some(hint), _)
            if matches!(
                hint.as_str(),
                "resource_type" | "array_type" | "dictionary_type"
            ) => {}
        (Some(hint), Some(text)) => parts.push(format!("{hint}({text})")),
        (Some(hint), None) => parts.push(hint.clone()),
        (None, _) => {}
    }
    if field.internal {
        parts.push("internal".into());
    }
    parts.join("  ")
}
