use std::path::PathBuf;

use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(about = "Inspect animation assets and graphs")]
    Animation(AnimationArgs),
    #[command(about = "Construct serialized Godot resources")]
    Resource(ResourceArgs),
    #[command(about = "Refresh and manage Godot project caches")]
    Cache(CacheArgs),
    #[command(about = "Refresh Godot project caches (alias for cache refresh)")]
    Import(CacheProjectArgs),
    #[command(about = "Query the configured engine and project's API")]
    Api(ApiArgs),
    #[command(about = "Print autoload initialization order")]
    Autoloads(AutoloadsArgs),
    #[command(about = "Associate the current Godot project with an engine")]
    Init(InitArgs),
    #[command(about = "Check a Godot project with its configured engine")]
    Check(CheckArgs),
    #[command(about = "Explain a Godot project's gdkit environment")]
    Doctor(DoctorArgs),
    #[command(about = "Format a GDScript source file")]
    Format(FormatArgs),
    #[command(about = "Format every GDScript file in the current Godot project")]
    FormatProject(FormatProjectArgs),
    #[command(about = "Inspect a Godot project's multiplayer topology")]
    Net(NetArgs),
    #[command(about = "Print a compact tree for a Godot text scene")]
    SceneTree(SceneTreeArgs),
    #[command(about = "Launch a durable named Godot session")]
    Run(RunArgs),
    #[command(about = "List named Godot sessions")]
    Sessions(SessionsArgs),
    #[command(about = "Print a named session's log")]
    Logs(SessionArgs),
    #[command(about = "Stop a named Godot session")]
    Stop(SessionArgs),
    #[command(about = "Restart a named Godot session")]
    Restart(SessionArgs),
    #[command(about = "Inspect a live named session")]
    Inspect(InspectArgs),
    #[command(about = "Run and control named multiplayer scenarios")]
    Scenario(ScenarioArgs),
}

#[derive(Debug, Args)]
pub struct AnimationArgs {
    #[command(subcommand)]
    pub command: AnimationCommand,
}

#[derive(Debug, Subcommand)]
pub enum AnimationCommand {
    #[command(about = "List animations packed in a binary glTF file")]
    List(AnimationListArgs),
    #[command(about = "Inspect effective AnimationTree graphs in a scene using Godot")]
    Inspect(AnimationInspectArgs),
}

#[derive(Debug, Args)]
pub struct AnimationListArgs {
    #[arg(help = "Binary glTF file to inspect")]
    pub path: PathBuf,
    #[arg(long, help = "Print only animation names")]
    pub names: bool,
    #[arg(long, value_name = "PATTERN", help = "Keep names containing this text")]
    pub filter: Option<String>,
    #[arg(
        long,
        value_enum,
        default_value = "human",
        help = "Select human or JSON result output"
    )]
    pub output: NetOutput,
}

#[derive(Debug, Args)]
pub struct AnimationInspectArgs {
    #[arg(help = "Project-local scene path or res:// path")]
    pub scene: String,
    #[arg(long, value_name = "NODE_PATH", help = "Inspect one AnimationTree")]
    pub tree: Option<String>,
    #[arg(long, default_value = ".", help = "Godot project directory")]
    pub project: PathBuf,
    #[arg(long, value_name = "PATH", help = "Use this Godot executable")]
    pub godot: Option<PathBuf>,
    #[arg(
        long,
        value_enum,
        default_value = "human",
        help = "Select human or JSON result output"
    )]
    pub output: NetOutput,
}

#[derive(Debug, Args)]
pub struct CacheArgs {
    #[command(subcommand)]
    pub command: CacheCommand,
}

#[derive(Debug, Subcommand)]
pub enum CacheCommand {
    #[command(about = "Rescan files and update UID, script-class, and import caches")]
    Refresh(CacheProjectArgs),
    #[command(about = "Discard derived project indexes and regenerate them with Godot")]
    Rebuild(CacheProjectArgs),
    #[command(about = "Remove derived indexes, imported assets, and shader caches")]
    Clean(CacheCleanArgs),
    #[command(about = "Show cache presence and size without starting Godot")]
    Status(CacheStatusArgs),
    #[command(about = "Stop a background import editor left by an older gdkit")]
    Stop(CacheStopArgs),
}

#[derive(Debug, Args)]
pub struct CacheStatusArgs {
    #[arg(default_value = ".", help = "Godot project directory")]
    pub project: PathBuf,
    #[arg(
        long,
        value_enum,
        default_value = "human",
        help = "Select human or JSON output"
    )]
    pub output: NetOutput,
}

#[derive(Debug, Args)]
pub struct CacheStopArgs {
    #[arg(default_value = ".", help = "Godot project directory")]
    pub project: PathBuf,
}

#[derive(Debug, Args)]
pub struct CacheCleanArgs {
    #[arg(default_value = ".", help = "Godot project directory")]
    pub project: PathBuf,
    #[arg(
        long,
        help = "List deletion targets without stopping the worker or removing files"
    )]
    pub dry_run: bool,
}

#[derive(Debug, Args)]
pub struct CacheProjectArgs {
    #[arg(default_value = ".", help = "Godot project directory")]
    pub project: PathBuf,
    #[arg(long, value_name = "PATH", help = "Use this Godot executable")]
    pub godot: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct NetArgs {
    #[command(subcommand)]
    pub command: Option<NetCommand>,
    #[arg(default_value = ".", help = "Godot project directory")]
    pub project: PathBuf,
    #[arg(long, value_name = "PATH", help = "Use this Godot executable")]
    pub godot: Option<PathBuf>,
    #[arg(
        long,
        value_enum,
        default_value = "human",
        help = "Select human or JSON result output"
    )]
    pub output: NetOutput,
}

#[derive(Debug, Subcommand)]
pub enum NetCommand {
    #[command(about = "Explain matching RPC and replication contracts")]
    Explain(NetExplainArgs),
}

#[derive(Debug, Args)]
pub struct NetExplainArgs {
    #[arg(help = "RPC method, receiver.method, scene, or replication node")]
    pub query: String,
    #[arg(long, default_value = ".", help = "Godot project directory")]
    pub project: PathBuf,
    #[arg(long, value_name = "PATH", help = "Use this Godot executable")]
    pub godot: Option<PathBuf>,
    #[arg(
        long,
        value_enum,
        default_value = "human",
        help = "Select human or JSON result output"
    )]
    pub output: NetOutput,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum NetOutput {
    Human,
    Json,
}

#[derive(Debug, Args)]
pub struct ApiArgs {
    #[arg(
        value_name = "CLASS|search",
        required_unless_present = "dump_json",
        conflicts_with = "dump_json",
        help = "Native or project class name, or search"
    )]
    pub query: Option<String>,
    #[arg(
        long,
        conflicts_with = "member",
        help = "Export the complete reflected native API as JSON"
    )]
    pub dump_json: bool,
    #[arg(value_name = "MEMBER|TERM", help = "Member name, or search term")]
    pub member: Option<String>,
    #[arg(long, default_value = ".", help = "Godot project directory")]
    pub project: PathBuf,
    #[arg(long, value_name = "PATH", help = "Use this Godot executable")]
    pub godot: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct AutoloadsArgs {
    #[arg(default_value = ".", help = "Godot project or a path inside it")]
    pub path: PathBuf,
}

#[derive(Debug, Args)]
pub struct InitArgs {
    #[arg(
        long,
        value_name = "PATH",
        help = "Godot editor executable to associate"
    )]
    pub godot: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct CheckArgs {
    #[arg(
        long,
        value_name = "PATH",
        conflicts_with = "stop_worker",
        help = "Copy only this file or directory into a minimal project (repeatable; paths relative to project)"
    )]
    pub slice: Vec<PathBuf>,
    #[arg(
        long,
        value_enum,
        default_value = "human",
        help = "Select human or JSON result output"
    )]
    pub output: CheckOutput,
    #[arg(long, help = "Reject method calls not guaranteed by the receiver type")]
    pub strict_methods: bool,
    #[arg(
        long,
        value_name = "PATH",
        help = "Smoke-test this scene after validation (repeatable; executes gameplay)"
    )]
    pub scene: Vec<String>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Run this project-owned SceneTree script after validation (repeatable)"
    )]
    pub script: Vec<String>,
    #[arg(long, requires = "script", default_value = "30", value_parser = clap::value_parser!(u64).range(1..=3600), help = "Wall-clock seconds allowed per project script")]
    pub script_timeout: u64,
    #[arg(long, requires = "scene", default_value = "2", value_parser = clap::value_parser!(u32).range(1..), help = "Process frames per smoke scene")]
    pub smoke_frames: u32,
    #[arg(long, requires = "scene", default_value = "30", value_parser = clap::value_parser!(u64).range(1..=3600), help = "Wall-clock seconds allowed per smoke scene")]
    pub smoke_timeout: u64,
    #[arg(default_value = ".", help = "Godot project directory")]
    pub project: PathBuf,
    #[arg(long, value_name = "PATH", help = "Use this Godot executable")]
    pub godot: Option<PathBuf>,
    #[arg(long, help = "Show full Godot output, including engine stack traces")]
    pub verbose: bool,
    #[arg(
        long,
        help = "Show file scan, engine validation, import, resource loading, and total times"
    )]
    pub timings: bool,
    #[arg(
        long,
        hide = true,
        help = "Compatibility option; checks already use fresh Godot processes"
    )]
    pub fresh: bool,
    #[arg(
        long,
        hide = true,
        help = "Compatibility option; checks already use a temporary project copy"
    )]
    pub isolated: bool,
    #[arg(
        long,
        conflicts_with = "fresh",
        conflicts_with_all = ["scene", "script", "strict_methods", "smoke_frames", "smoke_timeout", "script_timeout", "isolated", "output"],
        help = "Stop a legacy import editor (prefer gdkit cache stop)"
    )]
    pub stop_worker: bool,
}

#[derive(Debug, Args)]
pub struct DoctorArgs {
    #[arg(default_value = ".", help = "Godot project directory")]
    pub project: PathBuf,
    #[arg(long, value_name = "PATH", help = "Use this Godot executable")]
    pub godot: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum CheckOutput {
    Human,
    Json,
}

#[derive(Debug, Args)]
pub struct SceneTreeArgs {
    #[arg(help = "Godot text scene to inspect")]
    pub path: PathBuf,
    #[arg(long, help = "Show outgoing signal connections under each node")]
    pub connections: bool,
    #[arg(long, help = "Show saved group memberships under each node")]
    pub groups: bool,
    #[arg(
        long,
        conflicts_with = "expand_depth",
        help = "Recursively expand scene instances"
    )]
    pub expand: bool,
    #[arg(long, value_name = "DEPTH", value_parser = clap::value_parser!(u8).range(1..=64), help = "Expand scene instances up to this depth")]
    pub expand_depth: Option<u8>,
}

#[derive(Debug, Args)]
pub struct RunArgs {
    #[arg(default_value = ".", help = "Godot project directory")]
    pub project: PathBuf,
    #[arg(long, help = "Durable session name")]
    pub name: String,
    #[arg(long, help = "Run without a window")]
    pub headless: bool,
    #[arg(
        long,
        value_name = "PATH",
        help = "Scene to launch instead of the project's main scene"
    )]
    pub scene: Option<String>,
    #[arg(long, value_name = "PATH", help = "Use this Godot executable")]
    pub godot: Option<PathBuf>,
    #[arg(
        last = true,
        allow_hyphen_values = true,
        help = "Arguments passed to the project after --"
    )]
    pub arguments: Vec<String>,
}

#[derive(Debug, Args)]
pub struct SessionsArgs {
    #[arg(default_value = ".", help = "Godot project directory")]
    pub project: PathBuf,
    #[arg(long, help = "Include superseded session generations")]
    pub all: bool,
}

#[derive(Debug, Args)]
pub struct SessionArgs {
    #[arg(help = "Session name, or name@generation")]
    pub session: String,
    #[arg(long, default_value = ".", help = "Godot project directory")]
    pub project: PathBuf,
}

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("inspection").required(true).multiple(true).args(["net", "checkpoints"])))]
pub struct InspectArgs {
    #[arg(help = "Session name, or name@generation")]
    pub session: String,
    #[arg(long, help = "Observe live multiplayer state")]
    pub net: bool,
    #[arg(long, help = "Collect project-declared checkpoints")]
    pub checkpoints: bool,
    #[arg(
        long,
        value_name = "SESSION",
        requires = "checkpoints",
        conflicts_with = "net",
        help = "Compare checkpoints with another live session"
    )]
    pub compare: Option<String>,
    #[arg(long, default_value = ".", help = "Godot project directory")]
    pub project: PathBuf,
    #[arg(
        long,
        value_enum,
        default_value = "human",
        help = "Select human or JSON result output"
    )]
    pub output: NetOutput,
}

#[derive(Debug, Args)]
pub struct ScenarioArgs {
    #[command(subcommand)]
    pub command: ScenarioCommand,
}

#[derive(Debug, Subcommand)]
pub enum ScenarioCommand {
    #[command(about = "Start a configured scenario and await participant readiness")]
    Start(ScenarioStartArgs),
    #[command(about = "Show the latest run of a configured scenario")]
    Status(ScenarioNameArgs),
    #[command(about = "Orderly disconnect one scenario participant")]
    Disconnect(ScenarioParticipantArgs),
    #[command(about = "Force one scenario participant to crash")]
    Crash(ScenarioParticipantArgs),
    #[command(about = "Orderly disconnect all running scenario participants")]
    Stop(ScenarioNameArgs),
}

#[derive(Debug, Args)]
pub struct ScenarioStartArgs {
    #[arg(help = "Scenario name declared in gdkit.toml")]
    pub name: String,
    #[arg(long, default_value = ".", help = "Godot project directory")]
    pub project: PathBuf,
    #[arg(long, value_name = "PATH", help = "Use this Godot executable")]
    pub godot: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct ScenarioNameArgs {
    #[arg(help = "Scenario name declared in gdkit.toml")]
    pub name: String,
    #[arg(long, default_value = ".", help = "Godot project directory")]
    pub project: PathBuf,
}

#[derive(Debug, Args)]
pub struct ScenarioParticipantArgs {
    #[arg(help = "Scenario name declared in gdkit.toml")]
    pub name: String,
    #[arg(help = "Participant name declared in the scenario")]
    pub participant: String,
    #[arg(long, default_value = ".", help = "Godot project directory")]
    pub project: PathBuf,
}

#[derive(Debug, Args)]
pub struct FormatArgs {
    #[arg(default_value = "-", help = "Input file, or - for stdin")]
    pub path: PathBuf,
    #[arg(long, help = "Exit 1 if formatting would change the input")]
    pub check: bool,
    #[arg(long, default_value = "100", value_parser = clap::value_parser!(u16).range(1..), help = "Maximum width of compact guards")]
    pub line_width: u16,
}

#[derive(Debug, Args)]
pub struct FormatProjectArgs {
    #[arg(long, help = "Exit 1 if formatting would change any file")]
    pub check: bool,
    #[arg(long, default_value = "100", value_parser = clap::value_parser!(u16).range(1..), help = "Maximum width of compact guards")]
    pub line_width: u16,
}

#[derive(Debug, Args)]
pub struct ResourceArgs {
    #[command(subcommand)]
    pub command: ResourceCommand,
}

#[derive(Debug, Subcommand)]
pub enum ResourceCommand {
    #[command(about = "Create and verify a new serialized Resource using Godot")]
    Create(ResourceCreateArgs),
    #[command(
        about = "Discover instance fields and defaults using Godot (executes constructors and getters)"
    )]
    Schema(ResourceSchemaArgs),
}

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("resource_type").required(true).args(["class", "script"])))]
pub struct ResourceSchemaArgs {
    #[arg(long, help = "Native Resource class")]
    pub class: Option<String>,
    #[arg(long, help = "Project-local res:// GDScript path")]
    pub script: Option<String>,
    #[arg(long, default_value = ".")]
    pub project: PathBuf,
    #[arg(long, value_name = "PATH")]
    pub godot: Option<PathBuf>,
    #[arg(long, value_enum, default_value = "human")]
    pub output: NetOutput,
}

#[derive(Debug, Args)]
pub struct ResourceCreateArgs {
    #[arg(
        long,
        help = "JSON specification containing class or script and properties"
    )]
    pub spec: PathBuf,
    #[arg(long, help = "New project-local res:// path ending in .tres")]
    pub out: String,
    #[arg(long, default_value = ".")]
    pub project: PathBuf,
    #[arg(long, value_name = "PATH")]
    pub godot: Option<PathBuf>,
    #[arg(long, value_enum, default_value = "human")]
    pub output: NetOutput,
}
