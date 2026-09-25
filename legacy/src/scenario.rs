use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    cli::{
        ScenarioArgs, ScenarioCommand, ScenarioNameArgs, ScenarioParticipantArgs, ScenarioStartArgs,
    },
    session::{self, LaunchSpec, SessionRecord},
};

const SCENARIO_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ScenarioConfig {
    pub(crate) transport: ScenarioTransport,
    #[serde(default)]
    ports: BTreeMap<String, ScenarioPort>,
    #[serde(default = "default_timeout")]
    timeout_seconds: u64,
    participants: Vec<ParticipantConfig>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
enum ScenarioPort {
    Fixed(u16),
    Dynamic { checkpoint: String },
}

impl ScenarioPort {
    fn launch_value(&self) -> u16 {
        match self {
            Self::Fixed(port) => *port,
            Self::Dynamic { .. } => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ScenarioTransport {
    DedicatedEnet,
    SteamP2p,
}

impl ScenarioTransport {
    fn as_str(self) -> &'static str {
        match self {
            Self::DedicatedEnet => "dedicated_enet",
            Self::SteamP2p => "steam_p2p",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ParticipantConfig {
    name: String,
    role: ParticipantRole,
    #[serde(default)]
    scene: Option<String>,
    #[serde(default = "default_headless")]
    headless: bool,
    #[serde(default)]
    arguments: Vec<String>,
    readiness: Readiness,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ParticipantRole {
    Server,
    Client,
    LateClient,
}

impl ParticipantRole {
    fn as_str(self) -> &'static str {
        match self {
            Self::Server => "server",
            Self::Client => "client",
            Self::LateClient => "late_client",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Readiness {
    path: String,
    equals: Value,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ScenarioRun {
    schema_version: u32,
    name: String,
    generation: String,
    transport: ScenarioTransport,
    started_at_unix_ms: u64,
    ports: BTreeMap<String, u16>,
    participants: Vec<ParticipantRun>,
    #[serde(default = "ready_run_status")]
    status: ScenarioRunStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    failure: Option<ScenarioFailure>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ParticipantRun {
    name: String,
    role: ParticipantRole,
    session: String,
    log: PathBuf,
    user_data_dir: PathBuf,
    #[serde(default = "ready_participant_status")]
    readiness: ParticipantReadiness,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum ScenarioRunStatus {
    Starting,
    Ready,
    Failed,
}

impl ScenarioRunStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum ParticipantReadiness {
    Launched,
    Ready,
    Failed,
}

impl ParticipantReadiness {
    fn as_str(self) -> &'static str {
        match self {
            Self::Launched => "launched",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ScenarioFailure {
    participant: String,
    phase: ScenarioFailurePhase,
    message: String,
    occurred_at_unix_ms: u64,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum ScenarioFailurePhase {
    ArgumentExpansion,
    Launch,
    Readiness,
    EndpointResolution,
}

impl ScenarioFailurePhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::ArgumentExpansion => "argument_expansion",
            Self::Launch => "launch",
            Self::Readiness => "readiness",
            Self::EndpointResolution => "endpoint_resolution",
        }
    }
}

fn ready_run_status() -> ScenarioRunStatus {
    ScenarioRunStatus::Ready
}

fn ready_participant_status() -> ParticipantReadiness {
    ParticipantReadiness::Ready
}

fn default_timeout() -> u64 {
    30
}

fn default_headless() -> bool {
    true
}

fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn validate_component(kind: &str, value: &str) -> Result<(), Box<dyn Error>> {
    if value.is_empty()
        || value.len() > 40
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(
            format!("{kind} names must contain 1-40 ASCII letters, numbers, '-' or '_'").into(),
        );
    }
    Ok(())
}

fn validate_config(name: &str, config: &ScenarioConfig) -> Result<(), Box<dyn Error>> {
    validate_component("scenario", name)?;
    if config.timeout_seconds == 0 || config.timeout_seconds > 3600 {
        return Err("scenario timeout_seconds must be between 1 and 3600".into());
    }
    if config.participants.is_empty() {
        return Err("scenario must declare at least one participant".into());
    }
    let mut names = BTreeSet::new();
    for participant in &config.participants {
        validate_component("participant", &participant.name)?;
        if !names.insert(&participant.name) {
            return Err(format!("duplicate scenario participant '{}'", participant.name).into());
        }
        if !participant.readiness.path.starts_with('/') {
            return Err(format!(
                "participant '{}': readiness.path must be a JSON Pointer beginning with '/'",
                participant.name
            )
            .into());
        }
        let session_name = format!("scenario-{name}-{}", participant.name);
        if session_name.len() > 64 {
            return Err(format!(
                "participant '{}': generated session name exceeds 64 characters",
                participant.name
            )
            .into());
        }
    }
    for (name, port) in &config.ports {
        validate_component("port", name)?;
        match port {
            ScenarioPort::Fixed(0) => {
                return Err(format!(
                    "dynamic scenario port '{name}' must declare a checkpoint path"
                )
                .into());
            }
            ScenarioPort::Dynamic { checkpoint } if !checkpoint.starts_with('/') => {
                return Err(format!(
                    "dynamic scenario port '{name}' checkpoint must be a JSON Pointer beginning with '/'"
                )
                .into());
            }
            ScenarioPort::Fixed(_) | ScenarioPort::Dynamic { .. } => {}
        }
    }
    let servers = config
        .participants
        .iter()
        .filter(|participant| participant.role == ParticipantRole::Server)
        .count();
    let late_clients = config
        .participants
        .iter()
        .filter(|participant| participant.role == ParticipantRole::LateClient)
        .count();
    match config.transport {
        ScenarioTransport::DedicatedEnet => {
            if servers != 1 {
                return Err("dedicated_enet scenarios must declare exactly one server".into());
            }
            if config.ports.is_empty() {
                return Err("dedicated_enet scenarios must declare at least one named port".into());
            }
        }
        ScenarioTransport::SteamP2p if servers != 0 => {
            return Err("steam_p2p scenarios cannot declare a dedicated server participant".into());
        }
        ScenarioTransport::SteamP2p
            if config
                .ports
                .values()
                .any(|port| matches!(port, ScenarioPort::Dynamic { .. })) =>
        {
            return Err("steam_p2p scenarios cannot declare server-bound dynamic ports".into());
        }
        ScenarioTransport::SteamP2p => {}
    }
    if late_clients == 0 {
        return Err("scenario must declare at least one late_client participant".into());
    }
    Ok(())
}

fn launch_ports(config: &BTreeMap<String, ScenarioPort>) -> BTreeMap<String, u16> {
    config
        .iter()
        .map(|(name, port)| (name.clone(), port.launch_value()))
        .collect()
}

fn resolve_dynamic_ports(
    config: &BTreeMap<String, ScenarioPort>,
    checkpoints: &Value,
    ports: &mut BTreeMap<String, u16>,
) -> Result<(), Box<dyn Error>> {
    for (name, config) in config {
        let ScenarioPort::Dynamic { checkpoint } = config else {
            continue;
        };
        let port = checkpoints
            .pointer(checkpoint)
            .and_then(Value::as_u64)
            .filter(|port| (1..=u16::MAX.into()).contains(port))
            .ok_or_else(|| {
                format!("server checkpoint '{checkpoint}' did not report a valid port for '{name}'")
            })?;
        ports.insert(name.clone(), port as u16);
    }
    Ok(())
}

fn expand_argument(
    argument: &str,
    participant: &str,
    user_data_dir: &Path,
    ports: &BTreeMap<String, u16>,
) -> Result<String, Box<dyn Error>> {
    let mut expanded = argument
        .replace("{participant}", participant)
        .replace("{user_data_dir}", &user_data_dir.to_string_lossy());
    for (name, port) in ports {
        expanded = expanded.replace(&format!("{{port.{name}}}"), &port.to_string());
    }
    if expanded.contains('{') || expanded.contains('}') {
        return Err(format!("unknown scenario argument placeholder in '{argument}'").into());
    }
    Ok(expanded)
}

fn root(project: &Path) -> PathBuf {
    project.join(".godot/gdkit/scenarios")
}

fn records_root(project: &Path) -> PathBuf {
    root(project).join("records")
}

fn run_path(project: &Path, run: &ScenarioRun) -> PathBuf {
    records_root(project).join(format!("{}-{}.json", run.name, run.generation))
}

fn create_run(project: &Path, run: &ScenarioRun) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(records_root(project))?;
    let bytes = serde_json::to_vec_pretty(run)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(run_path(project, run))?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(())
}

#[cfg(not(windows))]
fn replace_record(temporary: &Path, path: &Path) -> io::Result<()> {
    fs::rename(temporary, path)
}

#[cfg(windows)]
fn replace_record(temporary: &Path, path: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{REPLACEFILE_WRITE_THROUGH, ReplaceFileW};

    let path: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let temporary: Vec<_> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe {
        ReplaceFileW(
            path.as_ptr(),
            temporary.as_ptr(),
            std::ptr::null(),
            REPLACEFILE_WRITE_THROUGH,
            std::ptr::null(),
            std::ptr::null(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn update_run(project: &Path, run: &ScenarioRun) -> Result<(), Box<dyn Error>> {
    let bytes = serde_json::to_vec_pretty(run)?;
    let path = run_path(project, run);
    let mut attempt = 0;
    let (temporary, mut file) = loop {
        let temporary = path.with_extension(format!("json.{}-{attempt}.tmp", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists && attempt < 100 => {
                attempt += 1;
            }
            Err(error) => return Err(error.into()),
        }
    };
    let result = (|| {
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        replace_record(&temporary, &path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(Into::into)
}

fn read_runs(project: &Path) -> Result<Vec<ScenarioRun>, Box<dyn Error>> {
    let entries = match fs::read_dir(records_root(project)) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut runs = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            let run: ScenarioRun = serde_json::from_slice(&fs::read(&path)?)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            if run.schema_version != SCENARIO_SCHEMA_VERSION {
                return Err(format!(
                    "{}: unsupported scenario schema version {}",
                    path.display(),
                    run.schema_version
                )
                .into());
            }
            runs.push(run);
        }
    }
    runs.sort_by(|left, right| {
        left.started_at_unix_ms
            .cmp(&right.started_at_unix_ms)
            .then_with(|| left.generation.cmp(&right.generation))
    });
    Ok(runs)
}

fn select_run(project: &Path, name: &str) -> Result<Option<ScenarioRun>, Box<dyn Error>> {
    validate_component("scenario", name)?;
    Ok(read_runs(project)?
        .into_iter()
        .rev()
        .find(|run| run.name == name))
}

fn participant_record(
    project: &Path,
    participant: &ParticipantRun,
) -> Result<SessionRecord, Box<dyn Error>> {
    session::select_record(project, &participant.session)?.ok_or_else(|| {
        format!(
            "scenario participant '{}' has no matching session record",
            participant.name
        )
        .into()
    })
}

fn wait_ready(
    record: &SessionRecord,
    participant: &ParticipantConfig,
    adapter: &str,
    timeout: Duration,
) -> Result<Value, Box<dyn Error>> {
    let deadline = Instant::now() + timeout;
    loop {
        if !session::is_running(record) {
            return Err(format!(
                "participant '{}' exited before readiness {} == {}",
                participant.name, participant.readiness.path, participant.readiness.equals
            )
            .into());
        }
        let values = crate::runtime_probe::checkpoint_values(record, adapter)?;
        if values.pointer(&participant.readiness.path) == Some(&participant.readiness.equals) {
            return Ok(values);
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "participant '{}' did not reach readiness {} == {} within {} seconds",
                participant.name,
                participant.readiness.path,
                participant.readiness.equals,
                timeout.as_secs()
            )
            .into());
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn cleanup(records: &[SessionRecord]) {
    for record in records.iter().rev() {
        let _ = session::crash_record(record);
    }
}

fn fail_start(
    project: &Path,
    run: &mut ScenarioRun,
    launched: &[SessionRecord],
    participant: &str,
    phase: ScenarioFailurePhase,
    error: Box<dyn Error>,
) -> Result<ExitCode, Box<dyn Error>> {
    run.status = ScenarioRunStatus::Failed;
    run.failure.get_or_insert_with(|| ScenarioFailure {
        participant: participant.into(),
        phase,
        message: error.to_string(),
        occurred_at_unix_ms: timestamp(),
    });
    if matches!(phase, ScenarioFailurePhase::Readiness)
        && let Some(participant) = run
            .participants
            .iter_mut()
            .find(|record| record.name == participant)
    {
        participant.readiness = ParticipantReadiness::Failed;
    }
    let persisted = update_run(project, run);
    cleanup(launched);
    match persisted {
        Ok(()) => Err(error),
        Err(persist_error) => Err(format!(
            "{error}; additionally failed to persist scenario failure: {persist_error}"
        )
        .into()),
    }
}

fn start(args: ScenarioStartArgs) -> Result<ExitCode, Box<dyn Error>> {
    let project = crate::engine::project_root(&args.project)?;
    let config =
        crate::engine::read_config(&project)?.ok_or("named scenarios require gdkit.toml")?;
    let scenario = config.scenarios.get(&args.name).ok_or_else(|| {
        format!(
            "no scenario named '{}' is declared in gdkit.toml",
            args.name
        )
    })?;
    validate_config(&args.name, scenario)?;
    let adapter = config
        .inspect
        .checkpoint_adapter
        .as_deref()
        .ok_or("named scenarios require inspect.checkpoint_adapter in gdkit.toml")?;
    let engine = crate::engine::resolve(&project, args.godot.as_deref())?;
    if let Some(previous) = select_run(&project, &args.name)? {
        for participant in &previous.participants {
            let record = participant_record(&project, participant)?;
            if session::is_running(&record) {
                return Err(format!(
                    "scenario '{}' already has a running participant '{}'; stop it first",
                    args.name, participant.name
                )
                .into());
            }
        }
    }
    let mut ports = launch_ports(&scenario.ports);
    let generation = format!("{:x}-{:x}", timestamp(), std::process::id());
    let run_root = root(&project)
        .join("runs")
        .join(format!("{}-{generation}", args.name));
    fs::create_dir_all(&run_root)?;
    let mut run = ScenarioRun {
        schema_version: SCENARIO_SCHEMA_VERSION,
        name: args.name,
        generation,
        transport: scenario.transport,
        started_at_unix_ms: timestamp(),
        ports: ports.clone(),
        participants: Vec::new(),
        status: ScenarioRunStatus::Starting,
        failure: None,
    };
    create_run(&project, &run)?;
    let mut launched = Vec::new();
    let timeout = Duration::from_secs(scenario.timeout_seconds);
    let stages = [
        &[ParticipantRole::Server][..],
        &[ParticipantRole::Client][..],
        &[ParticipantRole::LateClient][..],
    ];
    for roles in stages {
        let stage: Vec<_> = scenario
            .participants
            .iter()
            .filter(|participant| roles.contains(&participant.role))
            .collect();
        for participant in &stage {
            let user_data_dir = run_root.join("user-data").join(&participant.name);
            let arguments = match participant
                .arguments
                .iter()
                .map(|argument| {
                    expand_argument(argument, &participant.name, &user_data_dir, &ports)
                })
                .collect::<Result<Vec<_>, _>>()
            {
                Ok(arguments) => arguments,
                Err(error) => {
                    return fail_start(
                        &project,
                        &mut run,
                        &launched,
                        &participant.name,
                        ScenarioFailurePhase::ArgumentExpansion,
                        error,
                    );
                }
            };
            let session_name = format!("scenario-{}-{}", run.name, participant.name);
            let mut environment = BTreeMap::new();
            environment.insert("GDKIT_SCENARIO".into(), run.name.clone());
            environment.insert(
                "GDKIT_SCENARIO_PARTICIPANT".into(),
                participant.name.clone(),
            );
            environment.insert(
                "GDKIT_SCENARIO_ROLE".into(),
                participant.role.as_str().into(),
            );
            environment.insert(
                "GDKIT_SCENARIO_TRANSPORT".into(),
                scenario.transport.as_str().into(),
            );
            for (name, port) in &ports {
                environment.insert(
                    format!(
                        "GDKIT_SCENARIO_PORT_{}",
                        name.to_ascii_uppercase().replace('-', "_")
                    ),
                    port.to_string(),
                );
            }
            let record = match session::launch(LaunchSpec {
                name: session_name,
                project: project.clone(),
                engine: engine.clone(),
                scene: participant.scene.clone(),
                arguments,
                headless: participant.headless,
                user_data_dir: Some(user_data_dir.clone()),
                environment,
            }) {
                Ok(record) => record,
                Err(error) => {
                    return fail_start(
                        &project,
                        &mut run,
                        &launched,
                        &participant.name,
                        ScenarioFailurePhase::Launch,
                        error,
                    );
                }
            };
            println!(
                "started {} ({}) as {}@{}",
                participant.name,
                participant.role.as_str(),
                record.name,
                record.generation
            );
            run.participants.push(ParticipantRun {
                name: participant.name.clone(),
                role: participant.role,
                session: format!("{}@{}", record.name, record.generation),
                log: record.log.clone(),
                user_data_dir,
                readiness: ParticipantReadiness::Launched,
            });
            launched.push(record);
            if let Err(error) = update_run(&project, &run) {
                cleanup(&launched);
                return Err(error);
            }
        }
        for participant in stage {
            let record = launched
                .iter()
                .find(|record| record.name == format!("scenario-{}-{}", run.name, participant.name))
                .expect("launched participant");
            let checkpoints = match wait_ready(record, participant, adapter, timeout) {
                Ok(checkpoints) => checkpoints,
                Err(error) => {
                    return fail_start(
                        &project,
                        &mut run,
                        &launched,
                        &participant.name,
                        ScenarioFailurePhase::Readiness,
                        error,
                    );
                }
            };
            if participant.role == ParticipantRole::Server {
                if let Err(error) = resolve_dynamic_ports(&scenario.ports, &checkpoints, &mut ports)
                {
                    return fail_start(
                        &project,
                        &mut run,
                        &launched,
                        &participant.name,
                        ScenarioFailurePhase::EndpointResolution,
                        error,
                    );
                }
                run.ports.clone_from(&ports);
            }
            run.participants
                .iter_mut()
                .find(|record| record.name == participant.name)
                .expect("persisted participant")
                .readiness = ParticipantReadiness::Ready;
            if let Err(error) = update_run(&project, &run) {
                cleanup(&launched);
                return Err(error);
            }
            println!(
                "ready {}: {} == {}",
                participant.name, participant.readiness.path, participant.readiness.equals
            );
        }
    }
    run.status = ScenarioRunStatus::Ready;
    if let Err(error) = update_run(&project, &run) {
        cleanup(&launched);
        return Err(error);
    }
    println!("scenario {}@{} is ready", run.name, run.generation);
    Ok(ExitCode::SUCCESS)
}

fn status(args: ScenarioNameArgs) -> Result<ExitCode, Box<dyn Error>> {
    let project = crate::engine::project_root(&args.project)?;
    let Some(run) = select_run(&project, &args.name)? else {
        eprintln!("error: no run of scenario '{}' was found", args.name);
        return Ok(ExitCode::from(1));
    };
    println!(
        "scenario: {}@{} transport={} status={}",
        run.name,
        run.generation,
        run.transport.as_str(),
        run.status.as_str()
    );
    if let Some(failure) = &run.failure {
        println!(
            "first failure: participant={} phase={} at={} message={}",
            failure.participant,
            failure.phase.as_str(),
            failure.occurred_at_unix_ms,
            failure.message
        );
    }
    for (name, port) in &run.ports {
        println!("port {name}: {port}");
    }
    for participant in &run.participants {
        let record = participant_record(&project, participant)?;
        println!(
            "{} ({}) {} readiness={} log={} user_data={}",
            participant.name,
            participant.role.as_str(),
            if session::is_running(&record) {
                "running"
            } else {
                "exited"
            },
            participant.readiness.as_str(),
            crate::engine::display_path(&participant.log),
            crate::engine::display_path(&participant.user_data_dir)
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn control_participant(
    args: ScenarioParticipantArgs,
    crash: bool,
) -> Result<ExitCode, Box<dyn Error>> {
    let project = crate::engine::project_root(&args.project)?;
    let Some(run) = select_run(&project, &args.name)? else {
        eprintln!("error: no run of scenario '{}' was found", args.name);
        return Ok(ExitCode::from(1));
    };
    let Some(participant) = run
        .participants
        .iter()
        .find(|participant| participant.name == args.participant)
    else {
        eprintln!(
            "error: scenario '{}' has no participant '{}'",
            args.name, args.participant
        );
        return Ok(ExitCode::from(1));
    };
    let record = participant_record(&project, participant)?;
    let changed = if crash {
        session::crash_record(&record)?
    } else {
        session::disconnect_record(&record)?
    };
    if !changed {
        eprintln!("error: participant '{}' is not running", participant.name);
        return Ok(ExitCode::from(1));
    }
    println!(
        "{} participant '{}'",
        if crash { "crashed" } else { "disconnected" },
        participant.name
    );
    Ok(ExitCode::SUCCESS)
}

fn stop(args: ScenarioNameArgs) -> Result<ExitCode, Box<dyn Error>> {
    let project = crate::engine::project_root(&args.project)?;
    let Some(run) = select_run(&project, &args.name)? else {
        eprintln!("error: no run of scenario '{}' was found", args.name);
        return Ok(ExitCode::from(1));
    };
    let mut failed = false;
    for participant in run.participants.iter().rev() {
        let record = participant_record(&project, participant)?;
        if !session::is_running(&record) {
            continue;
        }
        match session::disconnect_record(&record) {
            Ok(true) => println!("disconnected participant '{}'", participant.name),
            Ok(false) => {}
            Err(error) => {
                failed = true;
                eprintln!(
                    "error: participant '{}' did not disconnect: {error}",
                    participant.name
                );
            }
        }
    }
    Ok(if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

pub(crate) fn run(args: ScenarioArgs) -> Result<ExitCode, Box<dyn Error>> {
    match args.command {
        ScenarioCommand::Start(args) => start(args),
        ScenarioCommand::Status(args) => status(args),
        ScenarioCommand::Disconnect(args) => control_participant(args, false),
        ScenarioCommand::Crash(args) => control_participant(args, true),
        ScenarioCommand::Stop(args) => stop(args),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_run_atomically_replaces_the_record() {
        let project = std::env::temp_dir().join(format!(
            "gdkit-scenario-record-{}-{}",
            std::process::id(),
            timestamp()
        ));
        let mut run = ScenarioRun {
            schema_version: SCENARIO_SCHEMA_VERSION,
            name: "atomic".into(),
            generation: "test".into(),
            transport: ScenarioTransport::DedicatedEnet,
            started_at_unix_ms: timestamp(),
            ports: BTreeMap::from([("game".into(), 7000)]),
            participants: Vec::new(),
            status: ScenarioRunStatus::Starting,
            failure: None,
        };
        create_run(&project, &run).unwrap();

        run.status = ScenarioRunStatus::Failed;
        run.failure = Some(ScenarioFailure {
            participant: "server".into(),
            phase: ScenarioFailurePhase::Readiness,
            message: "timed out".into(),
            occurred_at_unix_ms: timestamp(),
        });
        update_run(&project, &run).unwrap();

        let path = run_path(&project, &run);
        assert_eq!(
            fs::read(&path).unwrap(),
            serde_json::to_vec_pretty(&run).unwrap()
        );
        assert_eq!(read_runs(&project).unwrap().len(), 1);
        assert_eq!(fs::read_dir(records_root(&project)).unwrap().count(), 1);
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn validates_explicit_transports_and_expands_named_ports() {
        let config: ScenarioConfig = toml::from_str(
            r#"
transport = "dedicated_enet"
timeout_seconds = 10
ports = { game = { checkpoint = "/network/port" } }

[[participants]]
name = "server"
role = "server"
arguments = ["--server", "--port={port.game}"]
readiness = { path = "/network/listening", equals = true }

[[participants]]
name = "client-1"
role = "client"
arguments = ["--connect=127.0.0.1:{port.game}"]
readiness = { path = "/network/connected", equals = true }

[[participants]]
name = "client-2"
role = "client"
readiness = { path = "/network/connected", equals = true }

[[participants]]
name = "late-client"
role = "late_client"
readiness = { path = "/network/connected", equals = true }
"#,
        )
        .unwrap();
        validate_config("late_join", &config).unwrap();
        let mut ports = launch_ports(&config.ports);
        assert_eq!(ports["game"], 0);
        resolve_dynamic_ports(
            &config.ports,
            &serde_json::json!({"network": {"port": 7000}}),
            &mut ports,
        )
        .unwrap();
        assert_eq!(ports["game"], 7000);
        assert_eq!(
            expand_argument(
                "--connect=127.0.0.1:{port.game}",
                "client-1",
                Path::new("data"),
                &ports
            )
            .unwrap(),
            "--connect=127.0.0.1:7000"
        );
    }

    #[test]
    fn rejects_implicit_or_mixed_transport_shapes() {
        let missing = toml::from_str::<ScenarioConfig>(
            "participants = [{ name = 'late', role = 'late_client', readiness = { path = '/ready', equals = true } }]",
        );
        assert!(missing.is_err());

        let steam_with_server: ScenarioConfig = toml::from_str(
            r#"
transport = "steam_p2p"
[[participants]]
name = "server"
role = "server"
readiness = { path = "/ready", equals = true }
[[participants]]
name = "late"
role = "late_client"
readiness = { path = "/ready", equals = true }
"#,
        )
        .unwrap();
        assert!(validate_config("mixed", &steam_with_server).is_err());

        let unreported_dynamic: ScenarioConfig = toml::from_str(
            r#"
transport = "dedicated_enet"
ports = { game = 0 }
[[participants]]
name = "server"
role = "server"
readiness = { path = "/ready", equals = true }
[[participants]]
name = "late"
role = "late_client"
readiness = { path = "/ready", equals = true }
"#,
        )
        .unwrap();
        assert!(validate_config("dynamic", &unreported_dynamic).is_err());
    }
}
