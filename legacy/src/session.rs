use std::{
    collections::BTreeMap,
    collections::HashMap,
    error::Error,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::cli::{RunArgs, SessionArgs, SessionsArgs};

const SESSION_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionRecord {
    schema_version: u32,
    pub(crate) name: String,
    pub(crate) generation: String,
    project: PathBuf,
    engine: PathBuf,
    engine_version: String,
    scene: Option<String>,
    arguments: Vec<String>,
    headless: bool,
    pid: u32,
    process_started: u64,
    launched_at_unix_ms: u64,
    pub(crate) log: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) user_data_dir: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    environment: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) probe: Option<crate::runtime_probe::ProbeEndpoint>,
}

pub(crate) struct LaunchSpec {
    pub(crate) name: String,
    pub(crate) project: PathBuf,
    pub(crate) engine: PathBuf,
    pub(crate) scene: Option<String>,
    pub(crate) arguments: Vec<String>,
    pub(crate) headless: bool,
    pub(crate) user_data_dir: Option<PathBuf>,
    pub(crate) environment: BTreeMap<String, String>,
}

#[cfg(windows)]
struct StandardHandleInheritance(Vec<(windows_sys::Win32::Foundation::HANDLE, u32)>);

#[cfg(windows)]
impl StandardHandleInheritance {
    fn suppress() -> io::Result<Self> {
        use windows_sys::Win32::{
            Foundation::{
                GetHandleInformation, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE,
                SetHandleInformation,
            },
            System::Console::{
                GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
            },
        };
        let mut handles = Vec::new();
        for kind in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            unsafe {
                let handle = GetStdHandle(kind);
                if handle.is_null() || handle == INVALID_HANDLE_VALUE {
                    continue;
                }
                let mut flags = 0;
                if GetHandleInformation(handle, &mut flags) == 0 {
                    return Err(io::Error::last_os_error());
                }
                if flags & HANDLE_FLAG_INHERIT != 0 {
                    if SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) == 0 {
                        return Err(io::Error::last_os_error());
                    }
                    handles.push((handle, flags));
                }
            }
        }
        Ok(Self(handles))
    }
}

#[cfg(windows)]
impl Drop for StandardHandleInheritance {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation};
        for (handle, flags) in &self.0 {
            unsafe {
                let _ = SetHandleInformation(*handle, HANDLE_FLAG_INHERIT, *flags);
            }
        }
    }
}

#[cfg(windows)]
pub(crate) fn spawn_session(command: &mut Command) -> io::Result<std::process::Child> {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP;
    let _inheritance = StandardHandleInheritance::suppress()?;
    command.creation_flags(CREATE_NEW_PROCESS_GROUP).spawn()
}

#[cfg(unix)]
pub(crate) fn spawn_session(command: &mut Command) -> io::Result<std::process::Child> {
    command.spawn()
}

fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn validate_name(name: &str) -> Result<(), Box<dyn Error>> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("session names must contain 1-64 ASCII letters, numbers, '-' or '_'".into());
    }
    Ok(())
}

fn root(project: &Path) -> PathBuf {
    project.join(".godot/gdkit/sessions")
}

fn records_root(project: &Path) -> PathBuf {
    root(project).join("records")
}

pub(crate) fn read_records(project: &Path) -> Result<Vec<SessionRecord>, Box<dyn Error>> {
    let directory = records_root(project);
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut records = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            let record: SessionRecord = serde_json::from_slice(&fs::read(&path)?)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            if record.schema_version != SESSION_SCHEMA_VERSION {
                return Err(format!(
                    "{}: unsupported session schema version {}",
                    path.display(),
                    record.schema_version
                )
                .into());
            }
            records.push(record);
        }
    }
    records.sort_by(|left, right| {
        left.launched_at_unix_ms
            .cmp(&right.launched_at_unix_ms)
            .then_with(|| left.generation.cmp(&right.generation))
    });
    Ok(records)
}

pub(crate) fn select_record(
    project: &Path,
    selector: &str,
) -> Result<Option<SessionRecord>, Box<dyn Error>> {
    let (name, generation) = selector
        .split_once('@')
        .map_or((selector, None), |(name, generation)| {
            (name, Some(generation))
        });
    validate_name(name)?;
    let records = read_records(project)?;
    Ok(match generation {
        Some(generation) => records
            .into_iter()
            .find(|record| record.name == name && record.generation == generation),
        None => records.into_iter().rev().find(|record| record.name == name),
    })
}

fn unique_generation(project: &Path, name: &str) -> Result<(String, PathBuf), Box<dyn Error>> {
    fs::create_dir_all(records_root(project))?;
    fs::create_dir_all(root(project).join("logs"))?;
    let now = timestamp();
    for attempt in 0..100 {
        let generation = format!("{now:x}-{:x}-{attempt}", std::process::id());
        let record = records_root(project).join(format!("{name}-{generation}.json"));
        let log = root(project)
            .join("logs")
            .join(format!("{name}-{generation}.log"));
        if !record.exists() && !log.exists() {
            return Ok((generation, log));
        }
    }
    Err("could not allocate a session generation".into())
}

fn validate_scene(project: &Path, scene: Option<&str>) -> Result<Option<PathBuf>, Box<dyn Error>> {
    let Some(scene) = scene else {
        return Ok(None);
    };
    let path = project.join(scene.strip_prefix("res://").unwrap_or(scene));
    let path = fs::canonicalize(&path)
        .map_err(|error| format!("session scene {}: {error}", path.display()))?;
    if !path.starts_with(project)
        || !path.is_file()
        || !path
            .extension()
            .is_some_and(|extension| extension == "tscn" || extension == "scn")
    {
        return Err("session scenes must be .tscn or .scn files inside the project".into());
    }
    Ok(Some(path))
}

fn write_record(project: &Path, record: &SessionRecord) -> Result<(), Box<dyn Error>> {
    let path = records_root(project).join(format!("{}-{}.json", record.name, record.generation));
    let bytes = serde_json::to_vec_pretty(record)?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(())
}

pub(crate) fn launch(spec: LaunchSpec) -> Result<SessionRecord, Box<dyn Error>> {
    validate_name(&spec.name)?;
    let scene_path = validate_scene(&spec.project, spec.scene.as_deref())?;
    let launch_scene = scene_path
        .as_deref()
        .map(|path| {
            path.strip_prefix(&spec.project)
                .map(|path| format!("res://{}", path.to_string_lossy().replace('\\', "/")))
        })
        .transpose()?;
    let (version, _) = crate::engine::validated_version(&spec.engine, &spec.project)?;
    let (generation, log_path) = unique_generation(&spec.project, &spec.name)?;
    let prepared_probe = crate::runtime_probe::prepare(&spec.project, &generation)?;
    let mut log = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&log_path)?;
    writeln!(log, "gdkit session {}@{generation}", spec.name)?;
    writeln!(
        log,
        "engine: {} ({version})",
        crate::engine::display_path(&spec.engine)
    )?;
    writeln!(
        log,
        "project: {}",
        crate::engine::display_path(&spec.project)
    )?;
    log.flush()?;
    let stdout = log.try_clone()?;
    let stderr = log.try_clone()?;
    let mut debugger = crate::runtime_probe::start_debugger(
        &prepared_probe,
        &spec.engine,
        log.try_clone()?,
        log.try_clone()?,
    )?;
    let mut command = Command::new(&spec.engine);
    if spec.headless {
        command.arg("--headless");
    }
    command.arg("--path").arg(&spec.project);
    command
        .arg("--remote-debug")
        .arg(format!("tcp://127.0.0.1:{}", debugger.debugger_port))
        .arg("--script")
        .arg(&prepared_probe.script)
        .env("GDKIT_PROBE_GENERATION", &generation)
        .env("GDKIT_PROBE_TOKEN", &prepared_probe.token)
        .env("GDKIT_PROBE_READY", &prepared_probe.ready)
        .env(
            "GDKIT_PROBE_SCENE",
            launch_scene.as_deref().unwrap_or_default(),
        );
    if let Some(directory) = &spec.user_data_dir {
        fs::create_dir_all(directory)?;
        #[cfg(windows)]
        command
            .env("APPDATA", directory.join("roaming"))
            .env("LOCALAPPDATA", directory.join("local"));
        #[cfg(unix)]
        command
            .env("XDG_DATA_HOME", directory.join("data"))
            .env("XDG_CONFIG_HOME", directory.join("config"))
            .env("XDG_CACHE_HOME", directory.join("cache"));
    }
    command.envs(&spec.environment);
    if !spec.arguments.is_empty() {
        command.arg("--").args(&spec.arguments);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    let mut child = match spawn_session(&mut command) {
        Ok(child) => child,
        Err(error) => {
            let _ = debugger.child.kill();
            let _ = debugger.child.wait();
            return Err(error.into());
        }
    };
    let process_started = match process_started(child.id()) {
        Ok(started) => started,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = debugger.child.kill();
            let _ = debugger.child.wait();
            return Err(error.into());
        }
    };
    let probe = match crate::runtime_probe::await_ready(&prepared_probe, &mut child, &debugger) {
        Ok(probe) => probe,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = debugger.child.kill();
            let _ = debugger.child.wait();
            return Err(error);
        }
    };
    thread::spawn(move || {
        let _ = debugger.child.wait();
    });
    let record = SessionRecord {
        schema_version: SESSION_SCHEMA_VERSION,
        name: spec.name,
        generation,
        project: spec.project.clone(),
        engine: spec.engine,
        engine_version: version,
        scene: spec.scene,
        arguments: spec.arguments,
        headless: spec.headless,
        pid: child.id(),
        process_started,
        launched_at_unix_ms: timestamp(),
        log: log_path,
        user_data_dir: spec.user_data_dir,
        environment: spec.environment,
        probe: Some(probe),
    };
    if let Err(error) = write_record(&spec.project, &record) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    Ok(record)
}

fn spec_from_record(record: &SessionRecord) -> LaunchSpec {
    LaunchSpec {
        name: record.name.clone(),
        project: record.project.clone(),
        engine: record.engine.clone(),
        scene: record.scene.clone(),
        arguments: record.arguments.clone(),
        headless: record.headless,
        user_data_dir: record.user_data_dir.clone(),
        environment: record.environment.clone(),
    }
}

fn print_started(record: &SessionRecord) {
    println!(
        "started {}@{} pid {}",
        record.name, record.generation, record.pid
    );
    println!("log: {}", crate::engine::display_path(&record.log));
}

pub(crate) fn run(args: RunArgs) -> Result<ExitCode, Box<dyn Error>> {
    let project = crate::engine::project_root(&args.project)?;
    let engine = crate::engine::resolve(&project, args.godot.as_deref())?;
    validate_name(&args.name)?;
    if read_records(&project)?
        .iter()
        .rev()
        .find(|record| record.name == args.name)
        .is_some_and(is_running)
    {
        eprintln!(
            "error: session '{}' is already running; stop it or choose another name",
            args.name
        );
        return Ok(ExitCode::from(1));
    }
    let record = launch(LaunchSpec {
        name: args.name,
        project,
        engine,
        scene: args.scene,
        arguments: args.arguments,
        headless: args.headless,
        user_data_dir: None,
        environment: BTreeMap::new(),
    })?;
    print_started(&record);
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn list(args: SessionsArgs) -> Result<ExitCode, Box<dyn Error>> {
    let project = crate::engine::project_root(&args.project)?;
    let records = read_records(&project)?;
    let records = if args.all {
        records
    } else {
        let mut latest = HashMap::new();
        for record in records {
            latest.insert(record.name.clone(), record);
        }
        let mut records: Vec<_> = latest.into_values().collect();
        records.sort_by(|left, right| left.name.cmp(&right.name));
        records
    };
    if records.is_empty() {
        println!("no sessions");
        return Ok(ExitCode::SUCCESS);
    }
    println!("NAME\tGENERATION\tPID\tSTATUS\tMODE\tSCENE");
    for record in records {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            record.name,
            record.generation,
            record.pid,
            if is_running(&record) {
                "running"
            } else {
                "exited"
            },
            if record.headless {
                "headless"
            } else {
                "windowed"
            },
            record.scene.as_deref().unwrap_or("<main>")
        );
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn logs(args: SessionArgs) -> Result<ExitCode, Box<dyn Error>> {
    let project = crate::engine::project_root(&args.project)?;
    let Some(record) = select_record(&project, &args.session)? else {
        eprintln!("error: no session matches '{}'", args.session);
        return Ok(ExitCode::from(1));
    };
    let log = fs::canonicalize(&record.log)?;
    if !log.starts_with(fs::canonicalize(root(&project).join("logs"))?) {
        return Err("session log resolves outside the project session directory".into());
    }
    io::copy(&mut fs::File::open(log)?, &mut io::stdout().lock())?;
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn stop(args: SessionArgs) -> Result<ExitCode, Box<dyn Error>> {
    let project = crate::engine::project_root(&args.project)?;
    let Some(record) = select_record(&project, &args.session)? else {
        eprintln!("error: no session matches '{}'", args.session);
        return Ok(ExitCode::from(1));
    };
    if !terminate(&record)? {
        eprintln!(
            "error: {}@{} is not running",
            record.name, record.generation
        );
        return Ok(ExitCode::from(1));
    }
    println!("stopped {}@{}", record.name, record.generation);
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn disconnect_record(record: &SessionRecord) -> Result<bool, Box<dyn Error>> {
    if !is_running(record) {
        return Ok(false);
    }
    let Some(probe) = &record.probe else {
        return Err("session predates orderly disconnect support; restart it first".into());
    };
    crate::runtime_probe::disconnect(probe, &record.generation)?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while is_running(record) && std::time::Instant::now() < deadline {
        thread::sleep(std::time::Duration::from_millis(25));
    }
    if is_running(record) {
        return Err("session did not exit after the orderly disconnect request".into());
    }
    let _ = crate::runtime_probe::stop(probe);
    Ok(true)
}

pub(crate) fn crash_record(record: &SessionRecord) -> io::Result<bool> {
    terminate(record)
}

pub(crate) fn restart(args: SessionArgs) -> Result<ExitCode, Box<dyn Error>> {
    let project = crate::engine::project_root(&args.project)?;
    let Some(record) = select_record(&project, &args.session)? else {
        eprintln!("error: no session matches '{}'", args.session);
        return Ok(ExitCode::from(1));
    };
    if is_running(&record) && !terminate(&record)? {
        return Err("session changed before it could be stopped".into());
    }
    let restarted = launch(spec_from_record(&record))?;
    print_started(&restarted);
    Ok(ExitCode::SUCCESS)
}

#[cfg(windows)]
pub(crate) fn process_started(pid: u32) -> io::Result<u64> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, FILETIME},
        System::Threading::{
            GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        },
    };
    unsafe {
        let handle = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            pid,
        );
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let result = GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user);
        CloseHandle(handle);
        if result == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
    }
}

#[cfg(windows)]
pub(crate) fn is_running(record: &SessionRecord) -> bool {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, WAIT_TIMEOUT},
        System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
            WaitForSingleObject,
        },
    };
    unsafe {
        let handle = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            record.pid,
        );
        if handle.is_null() {
            return false;
        }
        let running = WaitForSingleObject(handle, 0) == WAIT_TIMEOUT
            && process_started(record.pid).ok() == Some(record.process_started);
        CloseHandle(handle);
        running
    }
}

#[cfg(windows)]
fn terminate(record: &SessionRecord) -> io::Result<bool> {
    let terminated = terminate_process(record.pid, record.process_started)?;
    if terminated && let Some(probe) = &record.probe {
        let _ = crate::runtime_probe::stop(probe);
    }
    Ok(terminated)
}

#[cfg(windows)]
pub(crate) fn terminate_process(pid: u32, started: u64) -> io::Result<bool> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, WAIT_TIMEOUT},
        System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
            TerminateProcess, WaitForSingleObject,
        },
    };
    unsafe {
        let handle = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE | PROCESS_TERMINATE,
            0,
            pid,
        );
        if handle.is_null() {
            return Ok(false);
        }
        if WaitForSingleObject(handle, 0) != WAIT_TIMEOUT
            || process_started(pid).ok() != Some(started)
        {
            CloseHandle(handle);
            return Ok(false);
        }
        let terminated = TerminateProcess(handle, 1) != 0;
        if terminated {
            let _ = WaitForSingleObject(handle, 5000);
        }
        let error = io::Error::last_os_error();
        CloseHandle(handle);
        if terminated { Ok(true) } else { Err(error) }
    }
}

#[cfg(unix)]
pub(crate) fn process_started(pid: u32) -> io::Result<u64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
    stat.rsplit_once(") ")
        .and_then(|(_, fields)| fields.split_whitespace().nth(19))
        .ok_or_else(|| io::Error::other("process start time was unavailable"))?
        .parse()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

#[cfg(unix)]
pub(crate) fn is_running(record: &SessionRecord) -> bool {
    process_started(record.pid).ok() == Some(record.process_started)
}

#[cfg(unix)]
fn terminate(record: &SessionRecord) -> io::Result<bool> {
    let terminated = terminate_process(record.pid, record.process_started)?;
    if terminated && let Some(probe) = &record.probe {
        let _ = crate::runtime_probe::stop(probe);
    }
    Ok(terminated)
}

#[cfg(unix)]
pub(crate) fn terminate_process(pid: u32, started: u64) -> io::Result<bool> {
    if process_started(pid).ok() != Some(started) {
        return Ok(false);
    }
    Ok(Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status()?
        .success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_names_and_selects_exact_generations() {
        assert!(validate_name("server_1").is_ok());
        assert!(validate_name("").is_err());
        assert!(validate_name("client a").is_err());

        let project =
            std::env::temp_dir().join(format!("gdkit-session-records-{}", std::process::id()));
        fs::create_dir_all(records_root(&project)).unwrap();
        let record = |generation: &str, launched_at_unix_ms: u64| SessionRecord {
            schema_version: SESSION_SCHEMA_VERSION,
            name: "server".into(),
            generation: generation.into(),
            project: project.clone(),
            engine: "engine".into(),
            engine_version: "test".into(),
            scene: None,
            arguments: vec!["--server".into()],
            headless: true,
            pid: 1,
            process_started: 1,
            launched_at_unix_ms,
            log: root(&project).join(format!("logs/{generation}.log")),
            user_data_dir: None,
            environment: BTreeMap::new(),
            probe: None,
        };
        write_record(&project, &record("first", 1)).unwrap();
        write_record(&project, &record("second", 2)).unwrap();
        assert_eq!(
            select_record(&project, "server")
                .unwrap()
                .unwrap()
                .generation,
            "second"
        );
        assert_eq!(
            select_record(&project, "server@first")
                .unwrap()
                .unwrap()
                .generation,
            "first"
        );
        assert!(select_record(&project, "server@stale").unwrap().is_none());
        fs::remove_dir_all(project).unwrap();
    }
}
