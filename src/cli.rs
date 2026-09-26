//! Argument definitions. Every command that touches a project takes the same
//! trio: `--project <dir>` (default `.`, discovery walks up), `--godot <exe>`,
//! and the global `--output human|json`.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(name = "gdkit", version, about = "Godot development utility belt")]
pub struct Cli {
    #[arg(long, global = true, value_enum, default_value = "human")]
    pub output: Output,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Output {
    Human,
    Json,
}

/// Shared by every project command.
#[derive(Debug, Args, Clone)]
pub struct ProjectArgs {
    #[arg(
        long,
        default_value = ".",
        help = "Project directory or any path inside it"
    )]
    pub project: PathBuf,
    #[arg(
        long,
        value_name = "PATH",
        help = "Godot editor executable (overrides GDKIT_GODOT and gdkit.toml)"
    )]
    pub godot: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Associate the project with an engine (writes gdkit.toml).
    Init {
        #[command(flatten)]
        project: ProjectArgs,
    },
    /// Explain the resolved engine, config, caches, and warning policy.
    Doctor {
        #[command(flatten)]
        project: ProjectArgs,
    },
    /// Static cross-reference checks, then import a disposable copy and load everything.
    Check(CheckArgs),
    /// Query the engine API (classes, builtins, utilities) and the project's class_name scripts.
    Api(ApiArgs),
    /// Everything that references a project file (offline).
    Refs {
        #[command(flatten)]
        project: ProjectArgs,
        #[arg(value_name = "RES")]
        path: String,
    },
    /// Typed views of project.godot (offline).
    Settings {
        #[command(flatten)]
        project: ProjectArgs,
        #[command(subcommand)]
        what: SettingsCommand,
    },
    /// Discover schemas and create verified .tres files from JSON.
    Resource {
        #[command(subcommand)]
        command: ResourceCommand,
    },
    /// Print a text scene's node tree (offline).
    SceneTree(SceneTreeArgs),
    /// Print autoloads in initialization order (offline).
    Autoloads {
        #[command(flatten)]
        project: ProjectArgs,
    },
    /// Static multiplayer topology report (offline).
    Net(NetArgs),
    /// Refresh Godot's derived caches with a headless editor import.
    Import {
        #[command(flatten)]
        project: ProjectArgs,
    },
    /// Run a scene to a frame count or checkpoint condition and report.
    Run(RunArgs),
}

#[derive(Debug, Args)]
pub struct CheckArgs {
    #[command(flatten)]
    pub project: ProjectArgs,
    #[arg(
        long,
        value_name = "PATH",
        help = "Check only these files/dirs (repeatable, project-relative)"
    )]
    pub slice: Vec<PathBuf>,
    #[arg(long, help = "Static cross-reference checks only; no engine")]
    pub static_only: bool,
    #[arg(long)]
    pub strict_methods: bool,
    #[arg(
        long,
        value_name = "RES",
        help = "Run this SceneTree script after validation (repeatable)"
    )]
    pub script: Vec<String>,
    #[arg(long, default_value = "30", value_parser = clap::value_parser!(u64).range(1..=3600))]
    pub script_timeout: u64,
    #[arg(long, default_value = "600", value_parser = clap::value_parser!(u64).range(1..=86400), help = "Seconds allowed for each import/load phase")]
    pub phase_timeout: u64,
    #[arg(
        long,
        value_name = "REPORT.json",
        help = "Classify diagnostics as new/carried/resolved against this report"
    )]
    pub baseline: Option<PathBuf>,
    #[arg(long, help = "Print full engine output to stderr")]
    pub verbose: bool,
}

#[derive(Debug, Args)]
pub struct ApiArgs {
    #[command(flatten)]
    pub project: ProjectArgs,
    #[arg(value_name = "CLASS|FUNCTION|search", required_unless_present = "dump")]
    pub query: Option<String>,
    #[arg(value_name = "MEMBER|TERM")]
    pub member: Option<String>,
    #[arg(long, conflicts_with_all = ["query", "member"], help = "Write the full native index as JSON")]
    pub dump: bool,
}

#[derive(Debug, Subcommand)]
pub enum SettingsCommand {
    /// Input actions and their events, including built-in ui_* actions.
    Input,
    /// Named 2D/3D render, physics, navigation, and avoidance layers.
    Layers,
    /// Window size, mode, and stretch.
    Window,
    /// Main scene, resolved from uid when needed.
    MainScene,
    /// Raw value of one key: `gdkit settings get application config/name`.
    Get { section: String, key: String },
}

#[derive(Debug, Subcommand)]
pub enum ResourceCommand {
    Schema {
        #[command(flatten)]
        project: ProjectArgs,
        #[arg(long, conflicts_with = "script")]
        class: Option<String>,
        #[arg(long)]
        script: Option<String>,
    },
    Create {
        #[command(flatten)]
        project: ProjectArgs,
        #[arg(long)]
        spec: PathBuf,
        #[arg(long, value_name = "RES")]
        out: String,
    },
}

#[derive(Debug, Args)]
pub struct SceneTreeArgs {
    pub path: PathBuf,
    #[arg(long)]
    pub connections: bool,
    #[arg(long)]
    pub groups: bool,
    #[arg(long, conflicts_with = "expand_depth")]
    pub expand: bool,
    #[arg(long, value_parser = clap::value_parser!(u8).range(1..=64))]
    pub expand_depth: Option<u8>,
}

#[derive(Debug, Args)]
pub struct NetArgs {
    #[command(flatten)]
    pub project: ProjectArgs,
    #[arg(long, help = "Explain one RPC method, receiver.method, scene, or node")]
    pub explain: Option<String>,
}

#[derive(Debug, Args)]
pub struct RunArgs {
    #[command(flatten)]
    pub project: ProjectArgs,
    #[arg(
        long,
        value_name = "RES",
        help = "Scene to run (default: the project main scene)"
    )]
    pub scene: Option<String>,
    #[arg(long, help = "Run with a window (default headless)")]
    pub windowed: bool,
    #[arg(
        long,
        default_value = "120",
        help = "Stop after this many process frames"
    )]
    pub frames: u64,
    #[arg(
        long,
        value_name = "/pointer=value",
        help = "Stop when this checkpoint equals the value"
    )]
    pub until: Option<String>,
    #[arg(
        long,
        default_value = "60",
        help = "Wall-clock seconds for the whole run"
    )]
    pub timeout: u64,
    #[arg(long, default_value = "20")]
    pub ready_timeout: u64,
    #[arg(long, help = "Also collect a network observation at the end")]
    pub net: bool,
    #[arg(last = true, allow_hyphen_values = true)]
    pub arguments: Vec<String>,
}
