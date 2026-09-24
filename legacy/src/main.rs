mod animation;
mod api;
mod cache;
mod check;
mod cli;
mod doctor;
mod engine;
mod import_worker;
mod net;
mod process;
mod project_files;
mod resource;
mod runtime_probe;
mod scenario;
mod session;

use std::{
    error::Error,
    fs,
    io::{self, Read, Write},
    path::Path,
    process::ExitCode,
};

use clap::{CommandFactory, Parser};
use gdkit::formatter::{Options, format_source};

use cli::{AutoloadsArgs, Cli, Command, FormatArgs, FormatProjectArgs, SceneTreeArgs};

fn autoloads(args: AutoloadsArgs) -> Result<ExitCode, Box<dyn Error>> {
    let project = gdview::Project::discover(args.path)?;
    print!("{}", project.autoloads()?);
    Ok(ExitCode::SUCCESS)
}

fn replace_file(path: &Path, original: &str, formatted: &str) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() {
        return Err(io::Error::other(
            "input must be a regular file, not a symlink",
        ));
    }
    let parent = path.parent().unwrap_or(Path::new("."));
    let mut attempt = 0;
    let (temporary, mut file) = loop {
        let temporary = parent.join(format!(".gdkit-{}-{attempt}.tmp", std::process::id()));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists && attempt < 100 => {
                attempt += 1
            }
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        file.set_permissions(metadata.permissions())?;
        file.write_all(formatted.as_bytes())?;
        file.sync_all()?;
        if fs::read_to_string(path)? != original {
            return Err(io::Error::other("input changed during formatting"));
        }
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn format(args: FormatArgs) -> Result<ExitCode, Box<dyn Error>> {
    let stdin = args.path == Path::new("-");
    let mut source = String::new();
    if stdin {
        io::stdin().read_to_string(&mut source)?;
    } else {
        source = fs::read_to_string(&args.path)?;
    }
    let options = Options {
        line_width: usize::from(args.line_width),
        ..Options::default()
    };
    let formatted = format_source(&source, &options)?;
    if args.check {
        return Ok(if source == formatted {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        });
    }
    if stdin {
        io::stdout().lock().write_all(formatted.as_bytes())?;
    } else {
        if source != formatted {
            replace_file(&args.path, &source, &formatted)?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn format_project(args: FormatProjectArgs) -> Result<ExitCode, Box<dyn Error>> {
    let root = std::env::current_dir()?;
    gdview::Project::open(&root).map_err(|error| -> Box<dyn Error> {
        match error {
            gdview::ProjectError::NotFound { .. } | gdview::ProjectError::ConfigNotAFile { .. } => {
                format!(
                    "{} is not a Godot project root: project.godot is missing or not a file",
                    root.display()
                )
                .into()
            }
            error => Box::new(error),
        }
    })?;
    let options = Options {
        line_width: usize::from(args.line_width),
        ..Options::default()
    };
    let mut changes = Vec::new();
    for path in project_files::collect(&root, &["gd"])? {
        let source =
            fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let formatted = format_source(&source, &options)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        if source != formatted {
            changes.push((path, source, formatted));
        }
    }
    if args.check {
        return Ok(if changes.is_empty() {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        });
    }
    for (path, source, formatted) in changes {
        replace_file(&path, &source, &formatted)?;
    }
    Ok(ExitCode::SUCCESS)
}

fn scene_tree(args: SceneTreeArgs) -> Result<ExitCode, Box<dyn Error>> {
    let options = gdkit::scene::TreeOptions {
        connections: args.connections,
        groups: args.groups,
    };
    if let Some(depth) = args
        .expand_depth
        .map(usize::from)
        .or(args.expand.then_some(64))
    {
        print!(
            "{}",
            gdkit::scene::compact_tree_expanded_with_options(&args.path, depth, options)?
        );
    } else {
        let source = fs::read_to_string(&args.path)?;
        let scene = gdkit::scene::parse(&source)
            .map_err(|error| format!("{}: {error}", args.path.display()))?;
        print!("{}", scene.compact_tree_with_options(options)?);
    }
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    if std::env::args_os().len() == 1 {
        return match Cli::command().print_help() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("error: {error}");
                ExitCode::from(2)
            }
        };
    }
    let result = match Cli::parse().command {
        Command::Animation(args) => animation::run(args),
        Command::Cache(args) => cache::run(args),
        Command::Import(args) => cache::refresh(args),
        Command::Api(args) => api::run(args),
        Command::Resource(args) => resource::run(args),
        Command::Autoloads(args) => autoloads(args),
        Command::Init(args) => engine::init(args),
        Command::Check(args) => check::run(args),
        Command::Doctor(args) => doctor::run(args),
        Command::Format(args) => format(args),
        Command::FormatProject(args) => format_project(args),
        Command::Net(args) => net::run(args),
        Command::SceneTree(args) => scene_tree(args),
        Command::Run(args) => session::run(args),
        Command::Sessions(args) => session::list(args),
        Command::Logs(args) => session::logs(args),
        Command::Stop(args) => session::stop(args),
        Command::Restart(args) => session::restart(args),
        Command::Inspect(args) => runtime_probe::inspect(args),
        Command::Scenario(args) => scenario::run(args),
    };
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(2)
        }
    }
}
