use std::{
    error::Error,
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use crate::cli::{
    CacheArgs, CacheCleanArgs, CacheCommand, CacheProjectArgs, CacheStatusArgs, NetOutput,
};

pub(crate) fn run(args: CacheArgs) -> Result<ExitCode, Box<dyn Error>> {
    match args.command {
        CacheCommand::Refresh(args) => refresh(args),
        CacheCommand::Rebuild(args) => import(args, true),
        CacheCommand::Clean(args) => clean(args),
        CacheCommand::Status(args) => status(args),
        CacheCommand::Stop(args) => {
            let project = crate::engine::project_root(&args.project)?;
            let _lock = lock(&project)?;
            crate::import_worker::stop_project(&project)?;
            println!("import worker stopped");
            Ok(ExitCode::SUCCESS)
        }
    }
}

pub(crate) fn refresh(args: CacheProjectArgs) -> Result<ExitCode, Box<dyn Error>> {
    import(args, false)
}

pub(crate) fn lock(project: &Path) -> io::Result<File> {
    let project = fs::canonicalize(project)?;
    let identity = crate::engine::display_path(&project);
    #[cfg(windows)]
    let identity = identity.to_lowercase();
    let key = blake3::hash(identity.as_bytes());
    let directory = std::env::temp_dir().join("gdkit-project-locks");
    fs::create_dir_all(&directory)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join(format!("{key}.lock")))?;
    file.lock()?;
    Ok(file)
}

fn import(args: CacheProjectArgs, rebuild: bool) -> Result<ExitCode, Box<dyn Error>> {
    let project = crate::engine::project_root(&args.project)?;
    let _lock = lock(&project)?;
    let targets = if rebuild {
        targets(&project, false)?
    } else {
        Vec::new()
    };
    if rebuild {
        ensure_idle(&project)?;
    }
    let engine = crate::engine::resolve(&project, args.godot.as_deref())?;
    let (version, _) = crate::engine::validated_version(&engine, &project)?;
    eprintln!(
        "engine: {} ({version})",
        crate::engine::display_path(&engine)
    );
    crate::import_worker::stop_project(&project)?;
    remove_targets(&targets)?;
    let result = crate::check::engine_output(
        &engine,
        &project,
        &[OsStr::new("--import"), OsStr::new("--quiet")],
    )?;
    io::stderr().write_all(&result.output.stdout)?;
    io::stderr().write_all(&result.output.stderr)?;
    let failed = !result.output.status.success() || crate::check::has_errors(&result.output);
    println!(
        "cache {} {}",
        if rebuild { "rebuild" } else { "refresh" },
        if failed { "failed" } else { "passed" }
    );
    Ok(if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

fn ensure_idle(project: &Path) -> Result<(), Box<dyn Error>> {
    if crate::session::read_records(project)?
        .iter()
        .any(crate::session::is_running)
    {
        return Err(
            "stop this project's running gdkit sessions before rebuilding or cleaning caches"
                .into(),
        );
    }
    Ok(())
}

fn validate_tree(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(io::Error::other(format!(
                "refusing cache reparse point: {}",
                path.display()
            )));
        }
    }
    if metadata.file_type().is_symlink() {
        return Err(io::Error::other(format!(
            "refusing cache symlink: {}",
            path.display()
        )));
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            validate_tree(&entry?.path())?;
        }
    }
    Ok(())
}

fn targets(project: &Path, all: bool) -> io::Result<Vec<PathBuf>> {
    let root = project.join(".godot");
    match fs::symlink_metadata(&root) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
        Ok(_) => validate_tree(&root)?,
    }
    let mut targets = Vec::new();
    for name in [
        "uid_cache.bin",
        "global_script_class_cache.cfg",
        "scene_groups_cache.cfg",
    ] {
        let path = root.join(name);
        if path.exists() {
            targets.push(path);
        }
    }
    let editor = root.join("editor");
    if editor.is_dir() {
        for entry in fs::read_dir(editor)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if ["filesystem_cache", "filesystem_update"]
                .iter()
                .any(|prefix| {
                    name.strip_prefix(prefix).is_some_and(|suffix| {
                        !suffix.is_empty() && suffix.bytes().all(|c| c.is_ascii_digit())
                    })
                })
            {
                targets.push(entry.path());
            }
        }
    }
    if all {
        for name in ["imported", "shader_cache"] {
            let path = root.join(name);
            if path.exists() {
                targets.push(path);
            }
        }
    }
    targets.sort();
    Ok(targets)
}

fn remove_targets(targets: &[PathBuf]) -> io::Result<()> {
    for path in targets {
        validate_tree(path)?;
        if path.is_dir() {
            fs::remove_dir_all(path)?;
        } else {
            fs::remove_file(path)?;
        }
        println!("removed: {}", crate::engine::display_path(path));
    }
    Ok(())
}

fn clean(args: CacheCleanArgs) -> Result<ExitCode, Box<dyn Error>> {
    let project = crate::engine::project_root(&args.project)?;
    let _lock = lock(&project)?;
    let targets = targets(&project, true)?;
    if args.dry_run {
        for path in &targets {
            println!("would remove: {}", crate::engine::display_path(path));
        }
        println!("cache clean preview: {} targets", targets.len());
    } else {
        ensure_idle(&project)?;
        crate::import_worker::stop_project(&project)?;
        remove_targets(&targets)?;
        println!(
            "cache cleaned: {} targets; run gdkit cache refresh to regenerate",
            targets.len()
        );
    }
    Ok(ExitCode::SUCCESS)
}

#[derive(serde::Serialize)]
struct CacheEntry {
    path: String,
    present: bool,
    bytes: u64,
}

fn size(path: &Path) -> io::Result<u64> {
    if path.is_dir() {
        fs::read_dir(path)?.try_fold(0, |total, entry| Ok(total + size(&entry?.path())?))
    } else {
        Ok(fs::metadata(path)?.len())
    }
}

fn status(args: CacheStatusArgs) -> Result<ExitCode, Box<dyn Error>> {
    let project = crate::engine::project_root(&args.project)?;
    let root = project.join(".godot");
    if fs::symlink_metadata(&root).is_ok() {
        validate_tree(&root)?;
    }
    let mut entries = Vec::new();
    for name in [
        "uid_cache.bin",
        "global_script_class_cache.cfg",
        "scene_groups_cache.cfg",
        "editor",
        "imported",
        "shader_cache",
        "gdkit/api-index.json",
        "gdkit/engine-probe.json",
        "gdkit/import-worker.json",
    ] {
        let path = root.join(name);
        let present = path.try_exists()?;
        entries.push(CacheEntry {
            path: format!(".godot/{name}"),
            present,
            bytes: if present { size(&path)? } else { 0 },
        });
    }
    match args.output {
        NetOutput::Json => println!(
            "{}",
            serde_json::json!({"schema_version": 1, "project": project, "entries": entries, "freshness": "unknown"})
        ),
        NetOutput::Human => {
            println!("project: {}", crate::engine::display_path(&project));
            for entry in entries {
                println!(
                    "{}: {} ({} bytes)",
                    entry.path,
                    if entry.present { "present" } else { "missing" },
                    entry.bytes
                );
            }
            println!(
                "Presence does not establish freshness or worker liveness. Run gdkit cache refresh after filesystem changes."
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}
