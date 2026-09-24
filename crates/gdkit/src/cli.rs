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

/// Shared by every engine-backed command.
#[derive(Debug, Args, Clone)]
pub struct ProjectArgs {
    #[arg(long, default_value = ".", help = "Project directory or any path inside it")]
    pub project: PathBuf,
    #[arg(long, value_name = "PATH", help = "Godot editor executable (overrides GDKIT_GODOT and gdkit.toml)")]
    pub godot: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Associate the project with an engine (writes gdkit.toml).
    Init { #[command(flatten)] project: ProjectArgs },
    /// Explain the resolved engine, config, caches, and sessions.
    Doctor { #[command(flatten)] project: ProjectArgs },
    /// Import a disposable copy and load every script, scene, and resource.
    Check(CheckArgs),
    /// Query the engine's ClassDB and the project's class_name scripts.
    Api(ApiArgs),
    /// Discover schemas and create verified .tres files from JSON.
    Resource { #[command(subcommand)] command: ResourceCommand },
    /// Offline glTF listing and engine-backed AnimationTree inspection.
    Animation { #[command(subcommand)] command: AnimationCommand },
    /// Print a text scene's node tree offline.
    SceneTree(SceneTreeArgs),
    /// Print autoloads in initialization order (offline).
    Autoloads { #[command(flatten)] project: ProjectArgs },
    /// Static multiplayer topology report.
    Net(NetArgs),
    /// Manage Godot's derived caches under .godot.
    Cache { #[command(subcommand)] command: CacheCommand },
    /// Launch a durable named session.
    Run(RunArgs),
    /// List sessions.
    Sessions { #[command(flatten)] project: ProjectArgs, #[arg(long)] all: bool },
    /// Print a session's log.
    Logs { #[command(flatten)] project: ProjectArgs, session: String },
    /// Stop a session.
    Stop { #[command(flatten)] project: ProjectArgs, session: String },
    /// Restart a session as a new generation.
    Restart { #[command(flatten)] project: ProjectArgs, session: String },
    /// Observe a live session.
    Inspect(InspectArgs),
    /// Start and control declared multiplayer scenarios.
    Scenario { #[command(subcommand)] command: ScenarioCommand },
}

#[derive(Debug, Args)]
pub struct CheckArgs {
    #[command(flatten)]
    pub project: ProjectArgs,
    #[arg(long, value_name = "PATH", help = "Check only these files/dirs (repeatable, project-relative)")]
    pub slice: Vec<PathBuf>,
    #[arg(long)]
    pub strict_methods: bool,
    #[arg(long, value_name = "RES", help = "Run this SceneTree script after validation (repeatable)")]
    pub script: Vec<String>,
    #[arg(long, default_value = "30", value_parser = clap::value_parser!(u64).range(1..=3600))]
    pub script_timeout: u64,
    #[arg(long, value_name = "RES", help = "Smoke-test this scene after validation (repeatable)")]
    pub scene: Vec<String>,
    #[arg(long, default_value = "2", value_parser = clap::value_parser!(u32).range(1..))]
    pub smoke_frames: u32,
    #[arg(long, default_value = "30", value_parser = clap::value_parser!(u64).range(1..=3600))]
    pub smoke_timeout: u64,
    #[arg(long, default_value = "600", help = "Seconds allowed for each import/load phase")]
    pub phase_timeout: u64,
    #[arg(long, help = "Print full engine output to stderr")]
    pub verbose: bool,
}

#[derive(Debug, Args)]
pub struct ApiArgs {
    #[command(flatten)]
    pub project: ProjectArgs,
    #[arg(value_name = "CLASS|search", required_unless_present = "dump")]
    pub query: Option<String>,
    #[arg(value_name = "MEMBER|TERM")]
    pub member: Option<String>,
    #[arg(long, conflicts_with_all = ["query", "member"], help = "Write the full native index as JSON")]
    pub dump: bool,
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

#[derive(Debug, Subcommand)]
pub enum AnimationCommand {
    /// Offline.
    List {
        path: PathBuf,
        #[arg(long)]
        names: bool,
        #[arg(long)]
        filter: Option<String>,
    },
    Inspect {
        #[command(flatten)]
        project: ProjectArgs,
        scene: String,
        #[arg(long, value_name = "NODE_PATH")]
        tree: Option<String>,
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
    #[arg(long, help = "Skip the engine and report from sources only")]
    pub offline: bool,
}

#[derive(Debug, Subcommand)]
pub enum CacheCommand {
    Status { #[command(flatten)] project: ProjectArgs },
    Refresh { #[command(flatten)] project: ProjectArgs },
    Rebuild { #[command(flatten)] project: ProjectArgs },
    Clean { #[command(flatten)] project: ProjectArgs, #[arg(long)] dry_run: bool },
}

#[derive(Debug, Args)]
pub struct RunArgs {
    #[command(flatten)]
    pub project: ProjectArgs,
    #[arg(long)]
    pub name: String,
    #[arg(long)]
    pub headless: bool,
    #[arg(long, value_name = "RES")]
    pub scene: Option<String>,
    #[arg(long, default_value = "20")]
    pub ready_timeout: u64,
    #[arg(last = true, allow_hyphen_values = true)]
    pub arguments: Vec<String>,
}

#[derive(Debug, Args)]
pub struct InspectArgs {
    #[command(flatten)]
    pub project: ProjectArgs,
    pub session: String,
    #[arg(long)]
    pub net: bool,
    #[arg(long)]
    pub checkpoints: bool,
    #[arg(long, requires = "checkpoints", conflicts_with = "net")]
    pub compare: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum ScenarioCommand {
    Start { #[command(flatten)] project: ProjectArgs, name: String },
    Status { #[command(flatten)] project: ProjectArgs, name: String },
    Disconnect { #[command(flatten)] project: ProjectArgs, name: String, participant: String },
    Crash { #[command(flatten)] project: ProjectArgs, name: String, participant: String },
    Stop { #[command(flatten)] project: ProjectArgs, name: String },
}
