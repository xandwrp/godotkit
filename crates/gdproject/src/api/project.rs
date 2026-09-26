//! The project's script classes, as the engine documents them
//! (`--doctool <dir> --gdscript-docs res://`), cached per engine and script
//! content in `.godot/gdkit/api-scripts.json`.
//!
//! Where the engine runs:
//! - Imported project (it has `.godot/global_script_class_cache.cfg`): on the
//!   project itself, under the workspace lock, headless and never `--editor`.
//!   Godot 4.7.2 writes nothing into an imported project for this run
//!   (`real_engine_script_docs_leave_the_project_untouched` pins it), and only
//!   there do cross-script types and `preload`s resolve.
//! - Otherwise, or when another gdkit holds the lock: on a scratch copy of the
//!   scripts and `project.godot`. Scripts that need other classes or assets
//!   may fail there.
//!
//! Every run names a placeholder scene: that skips resolving `run/main_scene`,
//! which aborts (and raises an OS alert) when a `uid://` main scene cannot be
//! resolved. The docs run exits before any scene loads.
//!
//! Scripts answer to their `class_name`, else their autoload name (as the
//! engine names them), else their `res://` path.
//!
//! A script the engine could not document comes from gdkit's syntactic index
//! instead (`from_engine: false`, no descriptions, declared types only), with
//! the engine's reason in [`ScriptDocs::fallbacks`]. If the engine run itself
//! fails, every script does, and nothing is cached.
//!
//! Loading a script runs its static initializers, as it does in `check`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use gdview::api::{
    self, API_INDEX_SCHEMA_VERSION, ApiArgument, ApiClass, ApiConstant, ApiEnum, ApiMethod,
    ApiProperty, ApiSignal, ApiType, ScriptOrigin, doc_xml,
};
use gdview::autoload::AutoloadTarget;
use gdview::declarations::{
    self, IndexedScript, InnerClassDeclaration, MemberDeclaration, MemberKind, Named,
};
use gdview::uid::UidMap;
use serde::{Deserialize, Serialize};

use crate::diagnostics::Diagnostic;
use crate::engine::Engine;
use crate::runner::{self, Invocation};
use crate::workspace::{API_SCRIPTS_CACHE_FILE, IsolatedCopy, Workspace};

const NO_MAIN_SCENE: &str = "res://.gdkit-no-main-scene.tscn";
const CLASS_CACHE: &str = ".godot/global_script_class_cache.cfg";

/// Where the project's script documentation came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptDocsSource {
    /// The engine documented the imported project in place.
    Project,
    /// The engine documented a copy of the scripts (not imported, or locked).
    ScriptCopy,
    /// The engine run failed; every class comes from source.
    Source,
    /// The project has no scripts.
    NoScripts,
}

/// A script the engine could not document, and why.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScriptFallback {
    /// `res://player.gd`
    pub path: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScriptDocs {
    pub source: ScriptDocsSource,
    pub fallbacks: Vec<ScriptFallback>,
    /// Answered from `.godot/gdkit/api-scripts.json`.
    pub cached: bool,
}

#[derive(Serialize, Deserialize)]
struct Cached {
    key: String,
    source: ScriptDocsSource,
    classes: Vec<ApiClass>,
    fallbacks: Vec<ScriptFallback>,
}

/// One engine run's result.
struct Documented {
    /// Engine-documented classes by [`api::script_key`].
    classes: BTreeMap<String, ApiClass>,
    /// Engine messages per `res://` script.
    reasons: BTreeMap<String, Vec<String>>,
    source: ScriptDocsSource,
    /// Why every script falls back, when the run itself failed.
    failure: Option<String>,
}

/// Script classes (not yet added to an index) and how they were obtained.
pub fn load_scripts(
    workspace: &Workspace,
    engine: &Engine,
    deadline: Duration,
) -> crate::Result<(Vec<ApiClass>, ScriptDocs)> {
    let project = workspace.project();
    let declarations = declarations::index_project(project)?;
    if declarations.scripts.is_empty() {
        let docs = ScriptDocs {
            source: ScriptDocsSource::NoScripts,
            fallbacks: Vec::new(),
            cached: false,
        };
        return Ok((Vec::new(), docs));
    }
    let imported = workspace.root().join(CLASS_CACHE).is_file();
    let key = cache_key(workspace, engine, &declarations.scripts, imported)?;
    let cache_path = workspace.state_dir().join(API_SCRIPTS_CACHE_FILE);
    if let Some(cached) = std::fs::read(&cache_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Cached>(&bytes).ok())
        .filter(|cached| cached.key == key)
    {
        let docs = ScriptDocs {
            source: cached.source,
            fallbacks: cached.fallbacks,
            cached: true,
        };
        return Ok((cached.classes, docs));
    }

    let documented = document(workspace, engine, &declarations.scripts, imported, deadline)?;
    let (source, failure) = (documented.source, documented.failure.clone());
    let autoloads = autoload_names(project);
    let (classes, fallbacks) = assemble(&declarations.scripts, &autoloads, documented);
    if failure.is_none() {
        let record = Cached {
            key,
            source,
            classes,
            fallbacks,
        };
        workspace.replace_state_file(API_SCRIPTS_CACHE_FILE, &serde_json::to_vec(&record)?)?;
        let docs = ScriptDocs {
            source,
            fallbacks: record.fallbacks,
            cached: false,
        };
        return Ok((record.classes, docs));
    }
    let docs = ScriptDocs {
        source: ScriptDocsSource::Source,
        fallbacks,
        cached: false,
    };
    Ok((classes, docs))
}

/// Engine, index schema, whether the project is imported (and its class cache),
/// `project.godot`, and every script's bytes.
fn cache_key(
    workspace: &Workspace,
    engine: &Engine,
    scripts: &[IndexedScript],
    imported: bool,
) -> crate::Result<String> {
    let mut hash = blake3::Hasher::new();
    let mut part = |bytes: &[u8]| {
        hash.update(&(bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    };
    part(engine.fingerprint.as_bytes());
    part(&API_INDEX_SCHEMA_VERSION.to_le_bytes());
    part(&[u8::from(imported)]);
    let read =
        |path: PathBuf| std::fs::read(&path).map_err(|source| crate::Error::Io { path, source });
    if imported {
        part(&read(workspace.root().join(CLASS_CACHE))?);
    }
    part(&read(workspace.root().join("project.godot"))?);
    for script in scripts {
        let path = &script.declaration.path;
        part(path.as_str().as_bytes());
        part(&read(workspace.root().join(path.relative()))?);
    }
    Ok(hash.finalize().to_hex().to_string())
}

/// Runs the engine where [the module docs](self) say.
fn document(
    workspace: &Workspace,
    engine: &Engine,
    scripts: &[IndexedScript],
    imported: bool,
    deadline: Duration,
) -> crate::Result<Documented> {
    let out = IsolatedCopy::bare()?;
    let docs = out.path().join("docs");
    std::fs::create_dir(&docs).map_err(|source| crate::Error::Io {
        path: docs.clone(),
        source,
    })?;
    let lock = if imported {
        match workspace.lock() {
            Ok(lock) => Some(lock),
            Err(crate::Error::Locked(_)) => None,
            Err(error) => return Err(error),
        }
    } else {
        None
    };
    let copy;
    let (project_dir, source) = match &lock {
        Some(_) => (workspace.root(), ScriptDocsSource::Project),
        None => {
            let mut selections: Vec<PathBuf> = scripts
                .iter()
                .map(|script| PathBuf::from(script.declaration.path.relative()))
                .collect();
            selections.push("project.godot".into());
            copy = IsolatedCopy::slice(workspace.project(), &selections)?;
            (copy.path(), ScriptDocsSource::ScriptCopy)
        }
    };
    let mut invocation = Invocation::new(engine, project_dir, deadline);
    invocation.engine_args = vec![
        "--doctool".into(),
        docs.clone().into_os_string(),
        "--gdscript-docs".into(),
        "res://".into(),
        NO_MAIN_SCENE.into(),
    ];
    let (captured, diagnostics) = runner::run_engine(&invocation)?;
    drop(lock);
    if !captured.success() {
        let status = if captured.timed_out {
            format!("the engine did not finish within {deadline:?}")
        } else {
            let output = String::from_utf8_lossy(&captured.stderr()).into_owned();
            let last = output.lines().rev().find(|line| !line.trim().is_empty());
            format!(
                "the engine's --gdscript-docs run failed ({}){}",
                captured
                    .status
                    .map_or("no exit status".into(), |status| status.to_string()),
                last.map(|line| format!(": {}", line.trim()))
                    .unwrap_or_default()
            )
        };
        return Ok(Documented {
            classes: BTreeMap::new(),
            reasons: BTreeMap::new(),
            source,
            failure: Some(status),
        });
    }
    let mut classes = BTreeMap::new();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&docs)
        .map_err(|source| crate::Error::Io {
            path: docs.clone(),
            source,
        })?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "xml"))
        .collect();
    files.sort();
    for file in files {
        let text = std::fs::read_to_string(&file).map_err(|source| crate::Error::Io {
            path: file.clone(),
            source,
        })?;
        let mut class = doc_xml::parse_class(&text)
            .map_err(|error| error.with_path(file.clone()))?
            .class;
        class.name = api::script_key(&class.name);
        class.parent = class.parent.as_deref().map(api::script_key);
        class.api_type = "script".into();
        classes.insert(class.name.clone(), class);
    }
    Ok(Documented {
        classes,
        reasons: reasons(&diagnostics),
        source,
        failure: None,
    })
}

/// Script autoloads by `res://` path. The engine documents an autoload script
/// without `class_name` under its autoload name. An unreadable `project.godot`
/// only costs those names.
fn autoload_names(project: &gdview::Project) -> BTreeMap<String, String> {
    let Ok(autoloads) = project.settings().and_then(|settings| settings.autoloads()) else {
        return BTreeMap::new();
    };
    let uses_uids = autoloads
        .iter()
        .any(|autoload| matches!(autoload.target, AutoloadTarget::Uid(_)));
    let uids = match uses_uids {
        true => UidMap::build(project).unwrap_or_default(),
        false => UidMap::default(),
    };
    autoloads
        .iter()
        .filter_map(|autoload| {
            let path = autoload.path(&uids)?;
            (path.extension() == Some("gd"))
                .then(|| (path.as_str().to_owned(), autoload.name.clone()))
        })
        .collect()
}

/// Pairs each declared class (script and inner) with its engine documentation,
/// falling back to source for the ones the engine did not document. A script
/// answers to its `class_name`, else its autoload name, else its path.
fn assemble(
    scripts: &[IndexedScript],
    autoloads: &BTreeMap<String, String>,
    documented: Documented,
) -> (Vec<ApiClass>, Vec<ScriptFallback>) {
    let Documented {
        classes: mut documented,
        reasons,
        failure,
        ..
    } = documented;
    let mut classes = Vec::new();
    let mut fallbacks = Vec::new();
    for script in scripts {
        let declaration = &script.declaration;
        let path = declaration.path.as_str().to_owned();
        let key = match (&declaration.class_name, autoloads.get(&path)) {
            (Some(named), _) => named.name.clone(),
            (None, Some(autoload)) => autoload.clone(),
            (None, None) => path.clone(),
        };
        let mut missing = false;
        let origin = |line: Option<usize>, from_engine: bool| ScriptOrigin {
            path: path.clone(),
            line,
            from_engine,
        };
        let class_line = declaration.class_name.as_ref().map(|named| named.line);
        match documented.remove(&key) {
            Some(mut class) => {
                annotate(&mut class, &declaration.members);
                class.script = Some(origin(class_line, true));
                classes.push(class);
            }
            None => {
                missing = true;
                let mut class =
                    from_source(&key, declaration.extends.as_ref(), &declaration.members);
                class.script = Some(origin(class_line, false));
                classes.push(class);
            }
        }
        let mut inner: Vec<&InnerClassDeclaration> = Vec::new();
        collect_inner(&declaration.inner_classes, &mut inner);
        for class_declaration in inner {
            // Under a `class_name` the index already qualifies inner classes.
            let inner_key = match &declaration.class_name {
                Some(_) => class_declaration.qualified_name.clone(),
                None => format!("{key}.{}", class_declaration.qualified_name),
            };
            let line = Some(class_declaration.line);
            match documented.remove(&inner_key) {
                Some(mut class) => {
                    annotate(&mut class, &class_declaration.members);
                    class.script = Some(origin(line, true));
                    classes.push(class);
                }
                None => {
                    missing = true;
                    let mut class = from_source(
                        &inner_key,
                        class_declaration.extends.as_ref(),
                        &class_declaration.members,
                    );
                    class.script = Some(origin(line, false));
                    classes.push(class);
                }
            }
        }
        if missing {
            let reason = match &failure {
                Some(failure) => failure.clone(),
                None => match reasons.get(&path) {
                    Some(messages) => messages.join("; "),
                    None => match &script.parse_error {
                        Some(error) => format!("the script does not parse: {error}"),
                        None => "the engine did not document it".into(),
                    },
                },
            };
            fallbacks.push(ScriptFallback { path, reason });
        }
    }
    (classes, fallbacks)
}

/// Up to three distinct engine messages per script, in order.
fn reasons(diagnostics: &[Diagnostic]) -> BTreeMap<String, Vec<String>> {
    let mut reasons: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for diagnostic in diagnostics {
        let Some(resource) = &diagnostic.resource else {
            continue;
        };
        let messages = reasons.entry(resource.clone()).or_default();
        if messages.len() < 3 && !messages.contains(&diagnostic.message) {
            messages.push(diagnostic.message.clone());
        }
    }
    reasons
}

fn collect_inner<'a>(inner: &'a [InnerClassDeclaration], out: &mut Vec<&'a InnerClassDeclaration>) {
    for class in inner {
        out.push(class);
        collect_inner(&class.inner_classes, out);
    }
}

/// Declaration lines for engine-documented members, matched by name.
fn annotate(class: &mut ApiClass, members: &[MemberDeclaration]) {
    let line = |kind: MemberKind, name: &str| {
        members
            .iter()
            .find(|member| member.kind == kind && member.name == name)
            .map(|member| member.line)
    };
    for method in &mut class.methods {
        method.line = line(MemberKind::Func, &method.name);
    }
    for property in &mut class.properties {
        property.line = line(MemberKind::Var, &property.name);
    }
    for signal in &mut class.signals {
        signal.line = line(MemberKind::Signal, &signal.name);
    }
    for constant in &mut class.constants {
        constant.line = line(MemberKind::Const, &constant.name);
    }
    for enum_ in &mut class.enums {
        enum_.line = line(MemberKind::Enum, &enum_.name);
    }
}

/// A class from gdkit's syntactic index: declared names and types only.
fn from_source(name: &str, extends: Option<&Named>, members: &[MemberDeclaration]) -> ApiClass {
    let type_of = |text: Option<&String>| {
        text.map_or(ApiType::Variant, |text| {
            ApiType::parse_doc(text, None, false)
        })
    };
    let arguments = |member: &MemberDeclaration| -> Vec<ApiArgument> {
        member
            .parameters
            .iter()
            .filter(|parameter| !parameter.is_variadic)
            .map(|parameter| ApiArgument {
                name: parameter.name.clone(),
                type_: type_of(parameter.type_text.as_ref()),
                default: parameter.default.clone(),
            })
            .collect()
    };
    let mut class = ApiClass {
        name: name.to_owned(),
        parent: extends.map(|named| match named.quoted_path() {
            Some(path) => api::script_key(&format!("\"{}\"", path.trim_start_matches("res://"))),
            None => named.name.clone(),
        }),
        api_type: "script".into(),
        instantiable: true,
        ..ApiClass::default()
    };
    for member in members {
        let line = Some(member.line);
        match member.kind {
            MemberKind::Func => class.methods.push(ApiMethod {
                name: member.name.clone(),
                is_static: member.is_static,
                is_vararg: member.parameters.iter().any(|p| p.is_variadic),
                return_type: type_of(member.type_text.as_ref()),
                arguments: arguments(member),
                line,
                ..ApiMethod::default()
            }),
            MemberKind::Var => class.properties.push(ApiProperty {
                name: member.name.clone(),
                type_: type_of(member.type_text.as_ref()),
                line,
                ..ApiProperty::default()
            }),
            MemberKind::Signal => class.signals.push(ApiSignal {
                name: member.name.clone(),
                arguments: arguments(member),
                line,
                ..ApiSignal::default()
            }),
            MemberKind::Const => class.constants.push(ApiConstant {
                name: member.name.clone(),
                type_: member
                    .type_text
                    .as_ref()
                    .map(|text| ApiType::parse_doc(text, None, false)),
                line,
                ..ApiConstant::default()
            }),
            MemberKind::Enum => class.enums.push(ApiEnum {
                name: member.name.clone(),
                line,
                ..ApiEnum::default()
            }),
        }
    }
    class
}
