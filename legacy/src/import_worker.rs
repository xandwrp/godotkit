use std::{
    error::Error,
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpStream},
    path::Path,
    time::Duration,
};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Worker {
    port: u16,
    pid: u32,
    token: String,
    diagnostics: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkerState {
    None,
    Running,
    Stale,
    Unknown,
    Malformed,
}

pub(crate) struct WorkerInspection {
    pub(crate) state: WorkerState,
    pub(crate) pid: Option<u32>,
    pub(crate) diagnostics: Vec<String>,
}

#[cfg(windows)]
fn worker_is_running(pid: u32) -> Option<bool> {
    unsafe {
        use windows_sys::Win32::{
            Foundation::{CloseHandle, WAIT_TIMEOUT},
            System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
        };
        let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if handle.is_null() {
            return Some(false);
        }
        let running = WaitForSingleObject(handle, 0) == WAIT_TIMEOUT;
        CloseHandle(handle);
        Some(running)
    }
}

#[cfg(not(windows))]
fn worker_is_running(_pid: u32) -> Option<bool> {
    None
}

pub(crate) fn inspect(project: &Path) -> WorkerInspection {
    let record = project.join(".godot/gdkit/import-worker.json");
    let bytes = match fs::read(record) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return WorkerInspection {
                state: WorkerState::None,
                pid: None,
                diagnostics: Vec::new(),
            };
        }
        Err(_) => {
            return WorkerInspection {
                state: WorkerState::Unknown,
                pid: None,
                diagnostics: Vec::new(),
            };
        }
    };
    let worker = match serde_json::from_slice::<Worker>(&bytes) {
        Ok(worker) => worker,
        Err(_) => {
            return WorkerInspection {
                state: WorkerState::Malformed,
                pid: None,
                diagnostics: Vec::new(),
            };
        }
    };
    let state = match worker_is_running(worker.pid) {
        Some(true) => WorkerState::Running,
        Some(false) => WorkerState::Stale,
        None => WorkerState::Unknown,
    };
    let diagnostics = worker
        .diagnostics
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            line.starts_with("ERROR:") || line.starts_with("SCRIPT ERROR:")
        })
        .map(str::to_owned)
        .collect();
    WorkerInspection {
        state,
        pid: Some(worker.pid),
        diagnostics,
    }
}

fn request_stop(worker: &Worker) -> Result<(), Box<dyn Error>> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, worker.port));
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(250))?;
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    serde_json::to_writer(
        &mut stream,
        &serde_json::json!({
            "token": worker.token, "changed_scripts": [], "stop": true
        }),
    )?;
    stream.write_all(b"\n")?;
    let mut response = String::new();
    BufReader::new(stream)
        .take(16 * 1024 * 1024)
        .read_line(&mut response)?;
    Ok(())
}

fn stop(worker: &Worker) -> Result<(), Box<dyn Error>> {
    let graceful = request_stop(worker).is_ok();
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::{
            Foundation::{CloseHandle, WAIT_TIMEOUT},
            System::Threading::{
                OpenProcess, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, TerminateProcess,
                WaitForSingleObject,
            },
        };
        let handle = OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_TERMINATE, 0, worker.pid);
        if !handle.is_null() {
            let mut result = WaitForSingleObject(handle, if graceful { 5000 } else { 0 });
            if result == WAIT_TIMEOUT {
                if TerminateProcess(handle, 1) == 0 {
                    let error = std::io::Error::last_os_error();
                    CloseHandle(handle);
                    return Err(error.into());
                }
                result = WaitForSingleObject(handle, 5000);
            }
            CloseHandle(handle);
            if result == WAIT_TIMEOUT {
                return Err("import worker did not terminate".into());
            }
        }
    }
    #[cfg(not(windows))]
    if !graceful {
        return Ok(());
    }
    Ok(())
}

pub fn stop_project(project: &Path) -> Result<(), Box<dyn Error>> {
    let record = project.join(".godot/gdkit/import-worker.json");
    if !record.exists() {
        return Ok(());
    }
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(project.join(".godot/gdkit/import-worker.lock"))?;
    lock.lock()?;
    if let Ok(bytes) = fs::read(&record)
        && let Ok(worker) = serde_json::from_slice::<Worker>(&bytes)
    {
        stop(&worker)?;
        let _ = fs::remove_file(record);
    }
    Ok(())
}
