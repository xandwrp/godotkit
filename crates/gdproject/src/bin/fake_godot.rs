//! Offline, scenario-driven stand-in for Godot (build with `test-engine`).
//!
//! # Scenario schema
//! The scenario is the JSON object in `<current_executable>.scenario.json`; a
//! missing sidecar means `{}`. There are deliberately no environment overrides:
//! tests copy the executable per test, and an inherited variable must never
//! change what a test observes.
//! Keys are `help`, `probe`, `--import`, `import_scan`, `check`, and
//! `script_bootstrap:<res://path>`, with fallback to `script_bootstrap`.
//! Selection uses flags before `--`, the `--script` filename stem, and the first
//! user argument after `--` for the bootstrap target. Unknown commands fail.
//! Each value is an object with optional fields (unknown fields are errors):
//! - `mode`: `envelope` (default), `error_envelope`, `no_envelope`, `hang`, `crash`.
//! - `stdout`, `stderr`: strings emitted verbatim (no implicit newline).
//! - `exit`: integer 0..255; defaults to 0, or 1 for `error_envelope`/`crash`.
//! - `delay_ms`: unsigned milliseconds, after output is flushed, before exit/hang.
//! - `payload`: any JSON value, replacing the entire default success payload.
//!   In `error_envelope` mode it instead supplies a HarnessError object
//!   (`stage`, `message`, optional `field`); default is stage=selected harness,
//!   message="fake engine error". Ignored in modes without an envelope.
//! - `class_cache`: exact string written by a normal (`envelope`) `--import` or
//!   `import_scan` to `<--path>/.godot/global_script_class_cache.cfg`.
//!
//! `envelope` emits `GDKIT_RESULT:` protocol v1 for harnesses, except bootstrap:
//! bootstrap emits exactly `GDKIT_SCRIPT_STARTED\n`, BEFORE configured output,
//! and never a success envelope. Help prints default flags unless `stdout` is
//! supplied; raw `--import` emits no envelope. Other configured output precedes
//! envelopes; terminate stdout with a newline when an envelope should be parsed.
//! `error_envelope` emits an error and no startup marker. `no_envelope` emits
//! only configured output, with no marker. `hang` flushes configured output then
//! sleeps until killed (bootstrap also emits its startup marker). `crash` means
//! a portable nonzero process exit, NOT a signal/abort, with no envelope/marker;
//! an explicit exit=0 is rejected. A configured nonzero normal exit is preserved.
//!
//! Defaults: probe payload is {"version":"4.7.2.stable.fake","major":4,
//! "editor":true}. ImportScan takes exactly one argument: the full-file manifest
//! filename. Its payload is {"scanned":N,"recognized_extensions":[strings]},
//! counting only `.gd` entries and reporting the fake editor set below.
//! Check takes exactly three arguments: manifest filename, `strict-methods` or
//! `project-policy`, and an editor-extension JSON filename (NOT inline JSON).
//! Its payload is {"counts":{"scripts":N,"scenes":N,"resources":N},
//! "failures":[],"recognized_extensions":[strings]}. The returned set is the
//! sorted, unique, lowercase union of the fake runtime set and editor input.
//! Only manifest paths whose case-insensitive final extension is in that union
//! count: `.gd` = scripts, `.tscn`/`.scn` = scenes, all other eligible paths =
//! resources. Duplicates count separately; missing files remain eligible.
//!
//! Deterministic fake runtime set: gd, gdshader, gdshaderinc, json, res, scn,
//! tres, tscn. Fake editor set: the runtime set plus bmp, png, svg. These lists
//! are fixture capabilities, not a claim about any real Godot installation.
//! No Godot process or installed registry is consulted. Use a complete `payload`
//! override to simulate custom registries/counts (including ImportScan handoff).
//!
//! Input filenames may be absolute, relative to --path, or res://. Manifests are
//! JSON arrays of res:// strings with no empty, '.', '..', or '.godot' components,
//! backslashes or control characters. Editor input is a JSON array of nonempty
//! extension strings without dots, slashes, backslashes, colons, whitespace or
//! control characters; [] is valid. Bad arguments/manifests/extensions emit an
//! error envelope with stage arguments/manifest/extensions and default exit 2;
//! file errors include the input filename in error.field. `payload` overrides
//! bypass argument and input validation, as do modes without default computation,
//! but not the real-engine invocation contracts below.
//!
//! Without `class_cache`, normal imports recursively scan --path for `.gd`
//! fixtures, skipping hidden directories and symlinks. Simple top-level
//! `class_name Name [extends Base]`, `extends Base`, and `@tool` declarations
//! produce a deterministic Godot class cache; this is NOT a GDScript parser.
//! Quoted/path inheritance and complex declarations require `class_cache`.
//! No script is executed, no assets are imported, no validation is simulated.
//!
//! # Real-engine invocation contracts
//! These fail like the real harnesses even when `payload` is overridden, so a
//! production regression cannot hide behind a lenient fake. They apply in
//! `envelope` mode (and `hang` for bootstrap, whose marker implies a valid
//! start); other modes are explicit failure simulations and skip them.
//! - `probe` requires `<--path>/project.godot` and `<--path>/probe.tres` files:
//!   otherwise a `resource` error envelope, exit 1.
//! - `import_scan` requires `--editor` before `--`: otherwise an `editor` error
//!   envelope, exit 2 (after default argument/manifest validation, as in Godot).
//! - `script_bootstrap` requires exactly one user argument: otherwise an
//!   `arguments` error envelope, exit 2, with no startup marker or configured
//!   output.
//!
//! Every invocation appends one JSON array of argv (excluding argv[0]) to
//! `<current_executable>.log`.
//! Invalid configuration/unsupported requests print `fake-godot: ...` to stderr
//! and exit 2. `runtime_probe`, `runtime-probe`, and FAKE_GODOT_READY_FILE are
//! explicitly unsupported: this fake does not implement a runtime-probe server.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use gdproject::protocol::{Envelope, HarnessError, PROTOCOL_VERSION, RESULT_PREFIX};
use serde::Deserialize;
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Copy, Default, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Mode {
    #[default]
    Envelope,
    ErrorEnvelope,
    NoEnvelope,
    Hang,
    Crash,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Scenario {
    mode: Mode,
    stdout: Option<String>,
    stderr: String,
    exit: Option<u8>,
    delay_ms: u64,
    // Preserve explicit JSON null as an override, rather than treating it as absent.
    #[serde(deserialize_with = "payload_value")]
    payload: Option<Value>,
    class_cache: Option<String>,
}

fn payload_value<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

fn sidecar(executable: &Path, suffix: &str) -> PathBuf {
    let mut name = executable.as_os_str().to_owned();
    name.push(suffix);
    name.into()
}

fn option<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
}

fn resource_file(project: &Path, name: &str) -> PathBuf {
    project.join(name.strip_prefix("res://").unwrap_or(name))
}

const RUNTIME_EXTENSIONS: &[&str] = &[
    "gd",
    "gdshader",
    "gdshaderinc",
    "json",
    "res",
    "scn",
    "tres",
    "tscn",
];
const EDITOR_EXTENSIONS: &[&str] = &[
    "bmp",
    "gd",
    "gdshader",
    "gdshaderinc",
    "json",
    "png",
    "res",
    "scn",
    "svg",
    "tres",
    "tscn",
];

fn input_error(stage: &str, field: Option<&str>, message: impl ToString) -> HarnessError {
    HarnessError {
        stage: stage.into(),
        message: message.to_string(),
        field: field.map(str::to_owned),
    }
}

fn read_strings(
    project: &Path,
    file: &str,
    stage: &str,
) -> std::result::Result<Vec<String>, HarnessError> {
    let bytes = fs::read(resource_file(project, file))
        .map_err(|error| input_error(stage, Some(file), error))?;
    serde_json::from_slice(&bytes).map_err(|error| input_error(stage, Some(file), error))
}

fn extension(path: &str) -> String {
    path.rsplit('/')
        .next()
        .unwrap_or("")
        .rsplit_once('.')
        .map_or_else(String::new, |(_, ext)| ext.to_lowercase())
}

fn default_payload(
    harness: &str,
    project: &Path,
    user_args: &[String],
) -> std::result::Result<Value, HarnessError> {
    if harness == "probe" {
        return Ok(json!({"version": "4.7.2.stable.fake", "major": 4, "editor": true}));
    }
    if (harness == "import_scan" && user_args.len() != 1)
        || (harness == "check"
            && (user_args.len() != 3
                || !matches!(user_args[1].as_str(), "strict-methods" | "project-policy")))
    {
        return Err(input_error(
            "arguments",
            None,
            if harness == "import_scan" {
                "Expected one manifest path"
            } else {
                "Expected manifest path, strict-methods | project-policy, and editor extensions file"
            },
        ));
    }
    let manifest = &user_args[0];
    let paths = read_strings(project, manifest, "manifest")?;
    for path in &paths {
        if path.contains('\\')
            || path.chars().any(char::is_control)
            || !path.strip_prefix("res://").is_some_and(|relative| {
                relative
                    .split('/')
                    .all(|part| !matches!(part, "" | "." | ".." | ".godot"))
            })
        {
            return Err(input_error(
                "manifest",
                Some(manifest),
                "Invalid resource path",
            ));
        }
    }
    if harness == "import_scan" {
        return Ok(json!({
            "scanned": paths.iter().filter(|path| extension(path) == "gd").count(),
            "recognized_extensions": EDITOR_EXTENSIONS
        }));
    }
    let extensions_file = &user_args[2];
    let mut recognized: BTreeSet<String> =
        RUNTIME_EXTENSIONS.iter().map(|ext| (*ext).into()).collect();
    for ext in read_strings(project, extensions_file, "extensions")? {
        if ext.is_empty()
            || ext
                .chars()
                .any(|c| matches!(c, '.' | '/' | '\\' | ':') || c.is_whitespace() || c.is_control())
        {
            return Err(input_error(
                "extensions",
                Some(extensions_file),
                "Invalid extension",
            ));
        }
        recognized.insert(ext.to_lowercase());
    }
    let (mut scripts, mut scenes, mut resources) = (0usize, 0usize, 0usize);
    for path in paths {
        let ext = extension(&path);
        if recognized.contains(&ext) {
            match ext.as_str() {
                "gd" => scripts += 1,
                "tscn" | "scn" => scenes += 1,
                _ => resources += 1,
            }
        }
    }
    Ok(json!({
        "counts": {"scripts": scripts, "scenes": scenes, "resources": resources},
        "failures": [], "recognized_extensions": recognized
    }))
}

/// Preconditions the real harnesses enforce regardless of their inputs; the
/// error carries the real exit code. See "Real-engine invocation contracts".
fn invocation_contract(
    harness: &str,
    flags: &[String],
    project: &Path,
) -> std::result::Result<(), (HarnessError, u8)> {
    match harness {
        "probe"
            if !["project.godot", "probe.tres"]
                .iter()
                .all(|name| project.join(name).is_file()) =>
        {
            Err((
                input_error(
                    "resource",
                    None,
                    "Cannot read probe project directory or load res://probe.tres",
                ),
                1,
            ))
        }
        "import_scan" if !flags.iter().any(|flag| flag == "--editor") => Err((
            input_error("editor", None, "Import scan requires --editor"),
            2,
        )),
        _ => Ok(()),
    }
}

fn emit_envelope(
    stdout: &mut impl Write,
    harness: &str,
    payload: Option<Value>,
    error: Option<HarnessError>,
) -> Result<()> {
    let envelope = Envelope {
        protocol: PROTOCOL_VERSION,
        harness: harness.into(),
        ok: error.is_none(),
        payload,
        error,
    };
    writeln!(
        stdout,
        "{RESULT_PREFIX}{}",
        serde_json::to_string(&envelope)?
    )?;
    Ok(())
}

fn identifier(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .enumerate()
            .all(|(i, c)| c == '_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit()))
}

fn collect_classes(root: &Path, dir: &Path, entries: &mut Vec<(String, String)>) -> Result<()> {
    let mut files = fs::read_dir(dir)?.collect::<io::Result<Vec<_>>>()?;
    files.sort_by_key(|entry| entry.file_name());
    for file in files {
        if file.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let kind = file.file_type()?;
        let path = file.path();
        if kind.is_dir() {
            collect_classes(root, &path, entries)?;
        } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "gd") {
            let source = fs::read_to_string(&path)?;
            let (mut name, mut base, mut tool) = (None, "RefCounted", false);
            for line in source.lines() {
                if line.starts_with(char::is_whitespace) {
                    continue;
                }
                let words: Vec<_> = line
                    .split('#')
                    .next()
                    .unwrap_or("")
                    .split_whitespace()
                    .collect();
                match words.as_slice() {
                    ["@tool"] => tool = true,
                    ["class_name", class] => name = Some(*class),
                    ["class_name", class, "extends", parent] => {
                        name = Some(*class);
                        base = parent;
                    }
                    ["extends", parent] => base = parent,
                    _ => {}
                }
            }
            if let Some(name) = name {
                if !identifier(name) || !identifier(base) {
                    return Err(
                        "complex fixture class declaration requires explicit class_cache".into(),
                    );
                }
                let res = format!(
                    "res://{}",
                    path.strip_prefix(root)?
                        .to_string_lossy()
                        .replace('\\', "/")
                );
                let entry = format!(
                    "{{\n\"base\": &{},\n\"class\": &{},\n\"icon\": \"\",\n\"is_abstract\": false,\n\"is_tool\": {tool},\n\"language\": &\"GDScript\",\n\"path\": {}\n}}",
                    serde_json::to_string(base)?,
                    serde_json::to_string(name)?,
                    serde_json::to_string(&res)?
                );
                entries.push((res, entry));
            }
        }
    }
    Ok(())
}

fn write_cache(project: &Path, configured: Option<&str>) -> Result<()> {
    let text = match configured {
        Some(text) => text.to_owned(),
        None => {
            let mut entries = Vec::new();
            collect_classes(project, project, &mut entries)?;
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            format!(
                "list=Array[Dictionary]([{}])\n",
                entries
                    .into_iter()
                    .map(|(_, entry)| entry)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    };
    fs::create_dir_all(project.join(".godot"))?;
    fs::write(project.join(".godot/global_script_class_cache.cfg"), text)?;
    Ok(())
}

fn run() -> Result<u8> {
    let executable = env::current_exe()?;
    let args: Vec<String> = env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(sidecar(&executable, ".log"))?;
    let mut line = serde_json::to_vec(&args)?;
    line.push(b'\n');
    log.write_all(&line)?;

    let split = args.iter().position(|a| a == "--").unwrap_or(args.len());
    let flags = &args[..split];
    let user_args = args.get(split + 1..).unwrap_or_default();
    let script = option(flags, "--script")
        .map(Path::new)
        .and_then(Path::file_stem)
        .and_then(|s| s.to_str());
    if env::var_os("FAKE_GODOT_READY_FILE").is_some()
        || flags.iter().any(|a| {
            matches!(
                a.as_str(),
                "runtime-probe" | "runtime_probe" | "--runtime-probe"
            )
        })
        || matches!(script, Some("runtime_probe" | "runtime-probe"))
    {
        return Err("runtime-probe server is not supported by fake-godot".into());
    }
    let harness = if flags.iter().any(|a| a == "--help") {
        "help"
    } else if flags.iter().any(|a| a == "--import") {
        "--import"
    } else {
        match script {
            Some(name @ ("probe" | "import_scan" | "check" | "script_bootstrap")) => name,
            _ => return Err("unsupported invocation (expected --help, --import, or a supported --script harness)".into()),
        }
    };
    let text = match fs::read_to_string(sidecar(&executable, ".scenario.json")) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => "{}".into(),
        Err(e) => return Err(e.into()),
    };
    let mut scenarios: BTreeMap<String, Scenario> = serde_json::from_str(&text)?;
    for key in scenarios.keys() {
        if !matches!(
            key.as_str(),
            "help" | "probe" | "--import" | "import_scan" | "check" | "script_bootstrap"
        ) && !key.starts_with("script_bootstrap:res://")
        {
            return Err(format!("unsupported scenario key: {key}").into());
        }
    }
    let specific = user_args
        .first()
        .map(|target| format!("script_bootstrap:{target}"));
    let scenario = if harness == "script_bootstrap" {
        specific
            .and_then(|key| scenarios.remove(&key))
            .or_else(|| scenarios.remove(harness))
    } else {
        scenarios.remove(harness)
    }
    .unwrap_or_default();
    let mode = scenario.mode;
    if mode == Mode::Crash && scenario.exit == Some(0) {
        return Err("crash requires a nonzero exit".into());
    }
    let project = Path::new(option(flags, "--path").unwrap_or("."));
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    if harness == "script_bootstrap" && matches!(mode, Mode::Envelope | Mode::Hang) {
        if user_args.len() != 1 {
            let error = input_error("arguments", None, "Expected one SceneTree script path");
            emit_envelope(&mut stdout, harness, None, Some(error))?;
            stdout.flush()?;
            return Ok(2);
        }
        writeln!(stdout, "GDKIT_SCRIPT_STARTED")?;
    }
    let default_help = "--headless --no-header --editor --path --script --import --quit\n";
    write!(
        stdout,
        "{}",
        scenario
            .stdout
            .as_deref()
            .unwrap_or(if harness == "help" && mode == Mode::Envelope {
                default_help
            } else {
                ""
            })
    )?;
    write!(stderr, "{}", scenario.stderr)?;
    let mut default_exit = if matches!(mode, Mode::ErrorEnvelope | Mode::Crash) {
        1
    } else {
        0
    };
    match mode {
        Mode::Envelope => {
            let contract = invocation_contract(harness, flags, project);
            // Without --editor there is no editor filesystem scan to write a cache.
            if harness == "--import" || (harness == "import_scan" && contract.is_ok()) {
                if option(flags, "--path").is_none() {
                    return Err("import requires --path".into());
                }
                write_cache(project, scenario.class_cache.as_deref())?;
            }
            if matches!(harness, "probe" | "import_scan" | "check") {
                let payload = match scenario.payload {
                    Some(value) => Ok(value),
                    None => default_payload(harness, project, user_args),
                };
                let (payload, error) = match (payload, contract) {
                    (Ok(payload), Ok(())) => (Some(payload), None),
                    (Ok(_), Err((error, exit))) => {
                        emit_envelope(&mut stdout, harness, None, Some(error))?;
                        stdout.flush()?;
                        stderr.flush()?;
                        return Ok(exit);
                    }
                    (Err(error), _) => {
                        default_exit = 2;
                        (None, Some(error))
                    }
                };
                emit_envelope(&mut stdout, harness, payload, error)?;
            }
        }
        Mode::ErrorEnvelope => {
            let error = match scenario.payload {
                Some(value) => serde_json::from_value(value)?,
                None => HarnessError {
                    stage: harness.into(),
                    message: "fake engine error".into(),
                    field: None,
                },
            };
            emit_envelope(&mut stdout, harness, None, Some(error))?;
        }
        _ => {}
    }
    stdout.flush()?;
    stderr.flush()?;
    std::thread::sleep(Duration::from_millis(scenario.delay_ms));
    if mode == Mode::Hang {
        loop {
            std::thread::sleep(Duration::from_secs(60));
        }
    }
    Ok(scenario.exit.unwrap_or(default_exit))
}

fn main() {
    let exit = match run() {
        Ok(exit) => exit,
        Err(error) => {
            eprintln!("fake-godot: {error}");
            2
        }
    };
    std::process::exit(i32::from(exit));
}
