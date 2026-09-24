# gdkit

A Rust command-line toolkit for Godot 4 projects. gdkit formats GDScript, validates
that a project actually loads in a real engine, reflects over the engine and
project API, authors `.tres` resources from JSON, inspects scenes and multiplayer
topology offline, and manages durable runtime sessions and repeatable multiplayer
scenarios, all driven by a single engine executable you associate with the project.

Most commands that touch Godot run against the engine you configure rather than
guessing from a bundled copy, so results always describe the engine your project
really uses.

## Highlights

- **Formatter**: opinionated GDScript formatting built on a lossless syntax tree
  (comments and string contents preserved). Single file, stdin, or whole project,
  with `--check` for CI.
- **Project checker**: copies the project to a scratch directory, imports it with
  the configured engine, loads every script, scene, resource, and shader, and
  reports structured diagnostics. Optionally runs headless gameplay smoke tests
  of chosen scenes and project-owned `SceneTree` test scripts.
- **API queries**: reflect the configured engine's `ClassDB`, including
  GDExtension-registered classes, merged with a static index of the project's own
  `class_name` scripts. Look up classes, members, and inheritance, or search by name.
- **Resource authoring**: create or interrogate serialized Resources from
  declarative JSON specs, with full-precision Variant encoding, verified writes,
  and schema discovery (`resource schema`) for tooling and agents.
- **Multiplayer analysis**: a static report of the project's RPC configuration,
  spawner/synchronizer setup, and connection lifecycle (`net`), plus live
  observation of a running session's peers, authority, and RPC traffic (`inspect --net`).
- **Runtime sessions**: launch, log, restart, and stop named game processes with
  immutable per-launch records; observe live sessions through project-defined
  state checkpoints.
- **Scenarios**: declarative multiplayer topologies (dedicated ENet or Steam P2P)
  with participant roles, readiness checkpoints, dynamic ports, and deterministic
  late-join / disconnect / crash control.
- **Scene inspection**: print a `.tscn` node tree offline, with packed-scene and
  inherited-scene expansion, signal connections, and groups. No engine required.
- **Cache management**: refresh, rebuild, or clean Godot's derived caches
  (UIDs, script classes, imports) after moving files outside the editor.

## Install

```sh
cargo install --git https://github.com/xandwr/gdkit gdkit
```

Or build from a checkout:

```sh
cargo build --release
```

Requires a Godot 4 editor build for the engine-backed commands. gdkit is tested
against recent 4.x editor builds, including custom builds; anything that passes
gdkit's compatibility probe works. Purely offline commands (`format`, `scene-tree`,
`autoloads`, `cache status`) need no engine at all.

## Quick start

Associate your project with an engine, then check it:

```sh
cd /path/to/game-project
gdkit init --godot /path/to/godot.editor.exe
gdkit check
```

`init` writes a `gdkit.toml` that pins the engine executable (a relative path
resolves against the file, so the config is portable in monorepos). Engine
selection precedence everywhere is `--godot` flag -> `GDKIT_GODOT` environment
variable -> `gdkit.toml`. The engine is probed once per project and cached; run
`gdkit doctor` to see the resolved engine, version, and cache health at any time.

```sh
gdkit init --godot ../engine/bin/godot.windows.editor.x86_64.console.exe
```

## Formatter

```sh
gdkit format script.gd            # rewrite in place (atomic)
gdkit format script.gd --check    # exit 1 if it would change
gdkit format - < script.gd        # stdin -> stdout
gdkit format-project              # every non-ignored .gd under project.godot
```

Line width defaults to 100 columns (`--line-width`); indentation uses tabs.
Long calls wrap one argument per line with a trailing comma. Script-scoped
signals and fields are stably reordered above enums, functions, and inner
classes as signals -> constants -> exports -> onready -> public -> private; export
annotations and comments move with their declarations. Disable formatting for
a region without weakening syntax validation:

```gdscript
# gdkit: off
var deliberately   =   spaced
# gdkit: on
```

Files with parser diagnostics are rejected rather than partially formatted.
File selection is Git-aware: parent and nested `.gitignore` files, `!` exceptions,
`.git/info/exclude`, global ignores, `.gdignore`, and hidden paths are all
respected (Git not required).

## Project checker

```sh
gdkit check game                        # full resource validation
gdkit check game --output json          # versioned report on stdout
gdkit check game --strict-methods       # reject un-guaranteed method calls
gdkit check game --scene res://scenes/match_lobby.tscn   # headless gameplay smoke test
gdkit check game --script res://tests/contract.gd        # project-owned SceneTree scripts
gdkit check --timings --verbose
```

`check` never reads or writes the source project's Godot caches: it copies the
project aside, imports the copy with the configured engine, and loads every
GDScript, scene, resource, and shader in a fresh process. It also cross-checks
the project's `class_name` declarations against Godot's global script-class
cache. Exit codes: `0` pass, `1` validation failure, `2` tooling failure.

Scene smoke checks run each scene in a fresh process with the project's autoloads
and normal runtime warnings, including `_ready()` and whatever I/O the project
normally performs. A timeout, nonzero exit, or `ERROR:` output fails the check.
Per-check artifacts (`report.json`, captured engine output) are kept under
`.godot/gdkit/checks/`.

Known third-party plugin errors can be tolerated with narrowly scoped import
exceptions in `gdkit.toml` (exact message + source path match):

```toml
[[check.ignore_import_errors]]
message = "ERROR: Script inherits from native type 'MarginContainer', ..."
source = "res://addons/some_plugin/plugin.gd"
```

### Check a script or extracted slice

```sh
gdkit check game --slice scripts/example.gd
gdkit check game --slice scripts/domain --slice assets/shared.tres
gdkit check extracted-directory --slice example.gd --godot /path/to/godot.editor.exe
```

Repeat `--slice` to explicitly select files or directories relative to the input
root. Only those sources are copied and imported, preserving their relative paths.
The scratch project starts with a minimal `project.godot`, without the source
project's autoloads, plugins, or project settings. Include `--slice project.godot`
when the slice needs those settings, along with all their required dependencies.
Dependencies are never added automatically: missing preloads, global classes,
autoloads, and resources produce normal engine diagnostics. A single script is
validated, not executed (execution still requires `--script`). The input directory
need not contain `project.godot`; engine selection and gdkit check policy still
come from the input root's `gdkit.toml`, environment, or `--godot` as usual.
Do not select `.godot`, `.git`, parent paths, or symbolic links.

## API queries

```sh
gdkit api --dump-json --godot /path/to/godot.editor.exe > api.json
gdkit api --dump-json --project game > project-engine-api.json
```

`--dump-json` emits one JSON object with `schema_version`, engine executable and
fingerprint, and the full `api` index (engine version, classes, parents, methods,
arguments, defaults, properties, signals, enums, constants, and reflection
capability flags). No lookup or search limit applies. This is the reflected native
ClassDB reference, including registered project GDExtensions, not documentation
prose, examples, built-in Variant types, or project GDScript declarations. Members
are declared per class; use `parent` to resolve inheritance. It also works outside
a project with an explicit or environment-selected engine, using an empty scratch
project. Within a project it uses the same engine-keyed cache as API lookups.


```sh
gdkit api CharacterBody3D move_and_slide   # signature + full inherited chain
gdkit api CharacterBody3D                  # properties, signals, enums, constants
gdkit api search multiplayer               # search native + project names
```

Class and member lookups include the project's own `class_name` scripts
(declared members, script inheritance, `res://` locations) alongside the engine's
native `ClassDB`, including GDExtension classes, because the index runs inside
the selected project. Member queries suggest nearby names for typos. The native
index is cached in `.godot/gdkit/api-index.json` and invalidated automatically
when the engine or extension set changes. Results identify the engine executable
and version.

## Resource authoring

Create serialized Resources from JSON without hand-editing `.tres` files:

```sh
gdkit resource create --spec weapon.json --out res://weapons/shotgun.tres
gdkit resource schema --class StandardMaterial3D --output json
gdkit resource schema --script res://resources/weapon_definition.gd --output json
```

```json
{
  "script": "res://resources/weapon_definition.gd",
  "properties": {
    "damage": 15,
    "display_name": "Shotgun",
    "offset": { "$variant": { "type": "Vector3", "value": [1.5, -2.25, 3.125] } },
    "icon": { "$ref": "res://textures/shotgun.svg" },
    "stats": { "$resource": { "script": "res://resources/weapon_stats.gd",
                              "properties": { "damage": 15 } } }
  }
}
```

Specs target one native class or project script. All supported Variant shapes
are covered: tagged large integers, `StringName`/`NodePath`, vectors, colors,
transforms, packed arrays, typed arrays, dictionaries, and nested Resource
graphs via `$ref` and `$resource`. Every value is verified by assigning it,
saving, and reloading the file without caches: setter transformations or
serialization precision loss fail the operation instead of silently diverging.
The destination is published atomically and never overwrites an existing file.

`resource schema` reports each field's type, default, hints, enum choices,
storage, and accepted inputs, a machine-readable contract aimed at code
generators and autonomous agents.

## Multiplayer tooling

**Static topology**: `gdkit net` reports the project's authored multiplayer
surface without entering its main scene: RPC configuration, call sites,
spawner/synchronizer contracts, authority assignments, and networked autoloads.
`gdkit net explain <method> --project game` relates each resolvable RPC call to
its endpoints, permissions, and the node paths peers must agree on. Results are
typed observations (unknowns stay explicit), not lint failures.

**Live sessions**: launch, observe, and control named game processes:

```sh
gdkit run --name server --headless -- --server
gdkit run --name client --scene res://scenes/game.tscn
gdkit sessions
gdkit inspect server --net                  # live peers, authority, RPC profiler
gdkit inspect server --checkpoints          # project-defined live state
gdkit inspect server --checkpoints --compare client
gdkit logs server ; gdkit stop server ; gdkit restart server
```

Every launch creates an immutable generation record (engine, scene, arguments,
PID, combined log). Checkpoints are defined by the project in `gdkit.toml`:

```toml
[inspect]
checkpoint_adapter = "res://tools/gdkit_checkpoints.gd"
```

The adapter script implements `collect_checkpoints(tree: SceneTree) -> Dictionary`,
returning JSON-safe snapshots such as `network_session` or `round_state`.
`inspect --checkpoints --compare` collects the same checkpoints from two live
sessions and reports differences by JSON Pointer.

**Scenarios**: repeatable multiplayer topologies with roles, readiness
checkpoints, and dynamic ports:

```toml
[scenarios.late_join]
transport = "dedicated_enet"
timeout_seconds = 20
ports = { game = { checkpoint = "/network_session/port" } }

[[scenarios.late_join.participants]]
name = "server"
role = "server"
arguments = ["--server", "--port={port.game}"]
readiness = { path = "/network_session/listening", equals = true }

[[scenarios.late_join.participants]]
name = "late-client"
role = "late_client"
arguments = ["--enet-client", "--port={port.game}"]
readiness = { path = "/network_session/connected", equals = true }
```

```sh
gdkit scenario start late_join
gdkit scenario status late_join
gdkit scenario disconnect late_join client-1
gdkit scenario crash late_join client-2
gdkit scenario stop late_join
```

The server starts and reports readiness first; gdkit reads the OS-selected port
from the declared checkpoint, then starts clients and late clients in their
declared phases. Transports are `dedicated_enet` and `steam_p2p`. Each
participant gets its own user-data directory, logs, and scenario environment
variables. Run records retain startup failures, resolved ports, and readiness
state for post-mortems.

## Animation inspection

```sh
gdkit animation list character.glb
gdkit animation list character.glb --names --filter run
gdkit animation inspect res://player.tscn --tree AnimationTree
gdkit animation inspect res://player.tscn --output json
```

`animation list` reads binary glTF metadata without starting Godot. `animation
inspect` asks the configured engine for the effective AnimationTree graph,
AnimationPlayer inventory, generated parameters, track targets, skeleton bones,
and structural findings. Human output is concise; JSON retains individual track
evidence and authored text-scene line locations. An inspection with structural
errors exits 1. Inspection instantiates the scene off-tree, so script constructors
run but `_ready` and gameplay processing do not.

## Scene inspection

```sh
gdkit scene-tree player.tscn
gdkit scene-tree player.tscn --expand --connections --groups
gdkit scene-tree scene.tscn --expand-depth 2
```

Parses the text scene offline: node hierarchy, attached scripts, native types,
signal connections, and saved groups. `--expand` recursively resolves packed
scene instances and inherited base scenes (depth-capped at 64; cycles rejected).

## Caches and imports

```sh
gdkit cache refresh game   # after adding/moving/deleting files outside Godot
gdkit cache rebuild game   # discard and regenerate the UID/script-class indexes
gdkit cache status game    # presence and sizes, no engine needed
gdkit cache clean game     # also remove imported assets and shader caches
```

`cache refresh` (alias: `gdkit import`) runs a headless editor import to
completion and persists Godot's UID, script-class, and import caches, useful
after refactors done outside the editor. Move a script's `.uid` sidecar with it
to preserve identity. `cache clean --dry-run` previews removals; every removed
target is printed.

## Library use

gdkit is also a library. The GDScript frontend from
[gdview](https://github.com/xandwr/gdview) is re-exported, giving lossless,
source-faithful parsing:

```rust
use gdkit::syntax::{ast::{AstNode, Function}, parse};

let parsed = parse("func greet(name: String): return name\n");
assert!(parsed.is_valid());
assert_eq!(parsed.root().text(), "func greet(name: String): return name\n");

for function in parsed.root().children().filter_map(Function::cast) {
    println!("{:?}", function.name());
}
```

The tree retains comments, whitespace, spelling, and line endings, with
byte-ranged diagnostics even on malformed input. The formatter API
(`gdkit::formatter::format_source`) returns the first diagnostic with its byte
range, and `gdkit::scene::expand` exposes the scene-hierarchy expansion behind
`scene-tree --expand`.

Upstream revision and attribution are recorded in
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

## Testing

```sh
cargo test
```

Integration tests that drive a real engine are `#[ignore]`d; point them at an
editor to include them:

```sh
$env:GDKIT_TEST_GODOT = 'P:/path/to/godot.console.exe'
cargo test --test check -- --include-ignored
```

The repository pins a Godot version in `godot.lock.json` purely as a
reproducible test dependency; normal operation never reads it. On Windows,
`pwsh -File scripts/provision-godot.ps1` provisions the pinned editor into
`.tools/` with checksum verification.

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), your
call. The gdview syntax frontend this project re-exports keeps its own MIT
notice, see [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

## Status

Pre-1.0. Command surface and JSON report formats may change between minor
versions; report outputs carry version fields to detect mismatches. The
[development roadmap](docs/poweruser-roadmap.md) documents researched future
directions; commands described there are not yet available.
