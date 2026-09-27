# gdkit for agents

What an agent working in a Godot project with no editor open actually reaches
for, in the order it reaches for it, and what it does with the answer. This is
the priority list for implementation: if a command is not on this page, it can
wait. `check` (including engine phases, slices, baselines, and project scripts),
`api`, `refs`, `settings`, `autoloads`, `doctor`, `init`, and `config` are implemented; the other workflows below describe the
intended surface, not a claim that all command scaffolds are complete. API-cache diagnostic enrichment
for `check` remains deferred.

The shape every command on this page shares:

- `--output json` puts exactly one JSON document on stdout and nothing else.
  Progress and errors go to stderr. An agent never has to strip prose.
- Exit code is the verdict. `0` the thing passed, `1` the thing failed and the
  JSON says why, `2` gdkit itself could not do the job (wrong engine path,
  invalid config, startup/probe failure). Check-phase timeouts produce an
    `incomplete` report and exit `1`; a probe timeout is a tool error, exit `2`.
    An agent branches on this before reading JSON.
- Diagnostic resource paths are `res://` (artifact paths are filesystem paths),
  locations use `resource` + `line` when available, and
  every report carries `schema_version`. An agent can grep, sort, and diff.
- Every engine call has a deadline. A hung autoload produces a timeout in the
  report, never a hung agent.

## 1. `check`: the feedback loop

```sh
gdkit check --output json
gdkit check --slice scripts/player.gd --output json
gdkit check --slice scripts/player.gd --slice scenes/player.tscn --output json
```

```sh
gdkit check --static-only --output json            # milliseconds, no engine
gdkit check --baseline .godot/gdkit/last.json --output json
```

Without an editor there are no red squiggles. `godot --check-only` parses one
script and knows nothing about scenes, resources, missing preloads, or a
`class_name` that moved. `check` runs in two layers. First, static
cross-reference checks with no engine: a scene connection to a method the script
no longer declares, `$Player/Camera3D` when no owning scene has that node, a
`preload` or `ExtResource` path that points at nothing, a `uid://` nothing
resolves. Then it imports a clean disposable copy of the project, scans scripts
in the editor, audits the generated class cache, and loads eligible entries from
a full-file inventory in a fresh runtime process. Eligibility uses the union of
editor and runtime loader registries, including imported assets and custom runtime
loaders, rather than a fixed extension list. Copies include source assets but
exclude `.godot` and `.git` (matched case-insensitively) and directories marked
with `.gdignore`; no source cache is seeded. Authored files and source import
caches are untouched; probe metadata and reports live under `.godot/gdkit`.

`--static-only` is the sub-second version for the inner loop. `--baseline`
diffs against a previous report by diagnostic identity (message and resource,
not line; paths from the disposable copy are rewritten to `res://`). Matching
counts occurrences: a second copy of a baselined finding is new. Carried static
findings permit engine validation but retain their failures and final failed
verdict; new static findings block engine phases. `baseline.new` helps
prioritize changes, not waive existing failures. A baseline from a different
`schema_version` is rejected (exit 2).

What the agent reads from the report:

```
outcome                       passed | failed | incomplete
phases[].id                   which step failed (import, resource_loading, …)
phases[].diagnostics[]        severity, message, resource, line, column, occurrences, suggestions
failures[]                    kind + message, one line each, for the summary
baseline.new[]                only with --baseline: diagnostics this change introduced
counts                        how much was actually validated
artifact_dir                  raw engine output when the diagnostic is not enough
```

The loop: edit → `check --slice <what I touched>` → read `diagnostics` where
`severity == "error"` → fix the first `resource:line` → repeat → full `check`
before declaring done. `--slice` is what makes the loop fast: one script in a
few seconds instead of the whole project in a minute. A slice has no autoloads
or project settings unless `project.godot` is sliced in, so a missing global is
a real diagnostic there, and the agent knows to widen the slice rather than
"fix" the script. Slice entries are project-relative (`./` is accepted) and must
exist; a typo is a usage error (exit 2), never a vacuous pass. Each sliced file
brings its `.uid` and `.import` sidecars.

`incomplete` is distinct from `failed` on purpose. It means the engine did not
report completion (crash, timeout, malformed output). The agent should not treat
it as "my change is wrong"; it should read `failures[].kind` and probably rerun.
Both `failed` and `incomplete` exit `1`; startup, probe, and configuration errors
exit `2` without a check report.

## 2. `api`: stop guessing names

```sh
gdkit api CharacterBody3D move_and_slide
gdkit api CharacterBody3D
gdkit api String split                # builtin Variant classes
gdkit api lerp                        # utility functions
gdkit api range                       # GDScript's own functions and @annotations
gdkit api search multiplayer --limit 40
gdkit api WeaponDefinition            # project class_name scripts are included
gdkit api BoxManager                  # so are autoloads, by autoload name
gdkit api --dump --output json > .godot/gdkit/api.json
```

Agents hallucinate Godot method names, argument orders, and which class a
method is declared on. `api` asks the configured editor itself, so answers
match that exact engine build:

- `--dump-extension-api-with-docs`: classes, builtin Variant types
  (`String.split`, `Vector3(x, y, z)`, `Vector3.ZERO`), utility functions,
  global enums and constants, singletons, and descriptions.
- `--doctool`: `@GDScript` (`range`, `preload`, `@export_range`) and property
  defaults. The engine ships no descriptions for `@GDScript`, so those entries
  have signatures only.
- `--doctool --gdscript-docs`: the project's scripts, with their `##` doc
  comments and the engine's inferred types (`speed := 2.0` is a `float`).

The answers that matter:

- A signature with the declaring class, so `move_and_slide` is known to come
  from `CharacterBody3D`, not `PhysicsBody3D`, and `take_damage` from
  `res://player.gd:42`.
- Argument names, types, and defaults, so a call site can be written once.
  Enum defaults are named (`Node.INTERNAL_MODE_DISABLED`, not `0`).
- Descriptions as plain text: code in backticks, GDScript examples fenced,
  C# dropped, references collected into `see_also`.
- On a miss, exit `1` with `suggestions[]`. "Did you mean `move_and_slide`" is
  the correction the agent needs, and it needs it to be machine-readable.

JSON answers have `kind`: `class`, `member` (with `member_kind`: `method`,
`property`, `signal`, `constant`, `enum`, `enum_value`, `utility_function`,
`gdscript_function`, `annotation`, `global_enum`, `global_enum_value`,
`global_constant`, `gdscript_constant`), `search`, or `miss`. A class lists the
members it declares; ask for a member to reach inherited ones.

The engine index is cached per engine in `.godot/gdkit/api-index.json` (about
3s to build, then instant). Project script docs are cached against the script
contents in `.godot/gdkit/api-scripts.json` and are only built when an answer
needs them. Answers that needed them carry `project_scripts`: `source` is
`project` when the engine read the imported project in place, and
`script_copy` when it could not (not imported yet, or another gdkit held the
lock), in which case scripts that depend on other classes or preloaded assets
may not resolve. A script the engine could not document is still answered from
its source (`from_engine: false`, no descriptions), and `fallbacks` says why.
Open the project in the editor once (or run an import) for complete answers.

## 3. `resource schema` + `resource create`: `.tres` as a form

```sh
gdkit resource schema --script res://resources/weapon_definition.gd --output json
gdkit resource schema --class StandardMaterial3D --output json
gdkit resource create --spec /tmp/shotgun.json --out res://weapons/shotgun.tres
```

Hand-writing `.tres` is where the quietest mistakes live: a property name that
does not exist, a Vector written as a string, a float that loses precision, an
enum written as its name instead of its value. Godot loads the file anyway and
silently ignores the bad field.

`schema` returns every stored field with `variant_type`, `class_name` (the
class an Object field requires), `enum_name`, `element` (typed arrays), `key`
and `value` (typed dictionaries), `default`, `hint`, `enum_choices` (names
with the values to write), and `accepts` (the JSON shapes that field takes).
Fields a spec cannot set (RID, Callable, Signal) say why in `unsupported`.
The agent fills a spec from that, never from memory. `--class` takes a native
class; a `class_name` script is named by `--script`. Script classes resolve
only in an imported project; when one does not, the error says how to import.
Engine output while loading (often from the project's autoloads, which Godot
runs for any script) is in `engine_diagnostics` and does not fail the command.

A spec is one resource object, the shape a nested `$resource` takes:

```json
{"script": "res://resources/weapon_definition.gd",
 "properties": {"damage": 12, "kind": 5, "tags": ["spread"],
                "offset": {"$variant": {"type": "Vector3", "value": [0, 0.5, 0]}},
                "ammo": {"$resource": {"script": "res://resources/ammo.gd", "properties": {"count": 8}}},
                "icon": {"$ref": "res://icons/shotgun.png"}}}
```

`create` assigns every property in the engine, reading each value as the
field's declared type (plain arrays and objects fill typed ones, `3` fills a
float, a string fills a StringName), saves, reloads with the cache disabled,
and echoes each value back. Any divergence is exit `2` with the offending
`field` and `stage`, and nothing is written: an enum written as its name, a
setter that changes the value, a float the engine stores differently (`0.1` in
a 32-bit `Vector2` is `0.10000000149011612`; the error names the stored value),
or `-0.0`, which `.tres` text cannot keep. `--out` must name a new `.tres` in
an existing directory; it never overwrites a file and never creates a
directory. The report lists each property as the engine stored it. The agent
gets a resource that is right or gets nothing.

## 4. `scene-tree --expand`: read a scene in one screen

```sh
gdkit scene-tree scenes/level.tscn --expand --connections --groups
gdkit scene-tree scenes/level.tscn --expand --output json
```

Offline, no engine. A `.tscn` read by eye hides instanced sub-scenes, inherited
bases, and which node a signal actually lands on. The expanded tree flattens
all of it with each node's type, script, origin scene, groups, and outgoing
connections, so `$Player/Camera3D` in a script can be checked against the
scene before running anything.

`unresolved[]` lists instances whose scene could not be loaded, with the
reason, instead of failing the whole tree.

## 5. `check --script`: the cheapest executable assertion

```sh
gdkit check --script res://tests/contract.gd --script-timeout 20 --output json
```

```gdscript
extends SceneTree
func _initialize() -> void:
    assert(GameSettings.enabled)          # autoloads are live
    var scene := load("res://scenes/match.tscn").instantiate()
    assert(scene.get_node("Lobby").max_players == 4)
    print("ok")
    quit(0)
```

"Run this in the real engine, headless, with autoloads, bounded by a deadline,
and tell me if it printed `ERROR:` or exited nonzero." No test framework, no
scene tree setup, no addon. An agent writes these as throwaway checks while
working, the way it would write a `println!` test, and deletes them after.

The bootstrap emits `GDKIT_SCRIPT_STARTED` before handing off to the script,
not a success envelope. The script must call `quit` before its deadline; success
requires the marker, a clean exit, and no error diagnostics. Engine shutdown
leak reports (leaked ObjectDB instances, resources still in use, RIDs) are
reported with `is_shutdown_noise` but do not fail the script. A marker followed
by a hang is a timeout, not a pass. The script may use `_init`, `_initialize`,
or `_process`; `_initialize` is only called when the script defines it.

The report's `phases[]` entry for the script carries `outcome`
(`completed | failed | timed_out | skipped`), diagnostics, and raw-stream artifact
paths. Failed engine phases skip later phases with a reason. New static findings
block engine phases; baseline-carried static findings allow them but still fail
the final report.

Runtime behavior is verified on Linux. macOS shares the POSIX implementation but
is not runtime-verified here. Windows Job objects are out of scope: direct-child
cleanup exists, but process-tree cleanup is not guaranteed.

## 6. `net`: the multiplayer map

```sh
gdkit net --output json
gdkit net --explain fire_weapon
gdkit net --explain res://scenes/arena.tscn --output json
```

For a multiplayer project, "which RPCs exist, with what mode and transfer
settings, who calls them, what node paths must agree on both peers, what is
synchronized and when" cannot be reconstructed reliably by reading. `net`
reports authored source observations with `unknowns[]` kept explicit rather
than guessed. It is always offline: no engine selection, import, cache or writes.
`--explain` accepts a method, `receiver.method`, scene/script `res://` path, or
authored node path; it returns associated endpoints, call sites and replication
nodes. A miss exits 1; an observation report exits 0 even with unknowns.

JSON schema 3 includes `endpoints`, `calls`, `anchors`, `contexts`, `spawners`,
`synchronizers`, `authority_uses`, `autoloads`, `coverage`, and `unknowns`.
`calls[].candidates` indexes that document's endpoint array (also in explanations).
Candidates are not proof of runtime compatibility. Locations use `resource` +
`line`; dynamic call expressions are retained verbatim. Replication `spawn` and
`mode` are independent, and unresolved values are null.

Read `coverage.limitations`: only direct authored scenes and GDScript are analyzed;
instances/inherited endpoints, runtime state, binary resources and C# remain
unresolved. Ordinary scene placement is not assumed to be `/root/SceneName`.
Replication mode is reported, not intervals or visibility behavior. See
[NET_SCOPE.md](NET_SCOPE.md) for the delivered scope and verification evidence.

## 7. `refs` and `settings`: the lookups that prevent wrong guesses

```sh
gdkit refs res://scripts/player.gd --output json     # before a rename or move
gdkit refs res://scripts                             # a directory: everything under it
gdkit refs res://scripts/old_name.gd                 # after a move: what still points at the old path
gdkit settings input                                 # action names and their keys
gdkit settings layers                                # named physics/render layers
gdkit settings window
gdkit settings main-scene
gdkit settings get application config/name
gdkit autoloads                                      # initialization order, kind, file
```

Offline, instant. `refs` lists every place a file is referenced, with
`resource` + `line` and a `kind`: `ext_resource` lines and the nodes whose
`script`/`instance` use them (`inherits` for a scene's root), placeholders and
string properties in scenes and resources; `extends`, `preload`, `load`, and
any other `res://`/`uid://` string literal in scripts (`change_scene_to_file`
targets, path constants); and `project.godot` autoloads, main scene, and other
settings (icon, themes, bus layouts, translations, plugins). A move or rename is
done with the full list in hand rather than discovered by the next `check`.

`by_uid: true` marks a reference that resolves through the file's uid and keeps
working after a move, as long as the uid moves too: `sidecars` lists the `.uid`
and `.import` files to move with it. Everything else names the path and breaks.
Uses by `class_name` follow a move and are not listed; `class_name` is reported
so a deletion is not mistaken for safe. The path need not exist, so querying the
old path after a move shows what was left behind; a missing path exits `1` with
`suggestions`. It may be `res://`, `uid://`, or project-relative.

`settings input` is the answer to `"jump"` vs `"ui_accept"`: the project's
actions, then Godot's built-in `ui_*` actions (`builtin: true`; a project entry
with a built-in's name replaces it, `in_project: true`). Keys, buttons, and axes
are the GDScript constants (`KEY_SPACE`, `JOY_BUTTON_A`); `physical_keycode` is
the key's position on a US layout. `settings layers` makes collision masks
readable (human output shows each layer's mask value). `settings window` fills
in Godot's defaults. `main-scene` resolves a `uid://`; it and `get` exit `1`
when the value is unset, and `get` suggests close keys. A value a typed view
cannot read is a tool error naming the key (exit `2`), never a silent default.

## Engine setup: `config` once per machine, `init` once per project

```sh
gdkit config set godot /usr/bin/godot     # machine-wide default; probed before it is saved
gdkit init                                # gdkit.toml that follows the default
gdkit init --godot ~/godot/4.4/godot      # gdkit.toml that pins this project's engine
gdkit config get godot                    # `list` shows the file path too
```

Engine selection, for every command: `--godot`, then `GDKIT_GODOT`, then
`[engine] executable` in the project's `gdkit.toml`, then the global default.
The global config is `$GDKIT_CONFIG_DIR/config.toml` when that is set, else
`$XDG_CONFIG_HOME/gdkit/config.toml` or `~/.config/gdkit/config.toml`
(`%APPDATA%\gdkit\config.toml` on Windows). It takes the same `[engine]
executable` table as `gdkit.toml`, and `config set`/`unset` keep hand-written
comments. `init` probes the engine before writing, and refuses to replace an
existing `gdkit.toml`: edit the file instead.

## 0. `doctor`: run this first in an unfamiliar repo

```sh
gdkit doctor
gdkit doctor --output json
```

Which engine will be used and why: every source that names one (`--godot`,
`GDKIT_GODOT`, `gdkit.toml`, the global default, in that order; a bare name
such as `godot` is looked up on `PATH`, and an empty `GDKIT_GODOT` counts as
unset), which one won, and what it overrides. Then its version, whether the
probe cache was warm (a cold one is probed and cached, as any engine command
would), whether both config files load, the project's GDScript warning policy
(`enable`, `directory_rules`, per-warning levels) and `strict_methods`, whether
the API caches are current for this engine, and how many check runs are kept
under `.godot/gdkit/artifacts/check`, with the newest.

Anything that would stop an engine command from starting is listed under
`problems`, each with a stable `code` (`project_config_invalid`,
`global_config_invalid`, `no_engine`, `engine_not_found`, `probe_failed` with the
engine's output, `warning_settings_invalid`, `state_unreadable`). Doctor keeps
going past each one, so a single run shows everything. Exit `0` with no problems,
`1` with any, `2` only outside a project. Thirty seconds here saves the twenty
minutes an agent otherwise spends debugging a wrong engine path through a failing
`check`.

## 8. `run`: execute to a checkpoint and stop

```sh
gdkit run --scene res://scenes/match.tscn --frames 120 --output json
gdkit run --until /round_state/phase=playing --timeout 30 --output json
gdkit run --net -- --server
```

Launch the scene headless with the project's autoloads, run until N frames or
until a checkpoint (from the project's `[run] checkpoint_adapter`) equals a
value, dump the checkpoints and log, kill the process, exit with the verdict.
No background process, nothing to remember to stop. The report has `frames`,
`stopped_by`, `checkpoints`, `diagnostics` from the log, and `log_path`.

There are no durable sessions or scenarios. An agent works request/response;
a process it has to poll and later stop is a liability, not a feature.
Multi-participant runs, when a project needs them, are a list of these with a
readiness order.

## Implementation priority derived from this page

1. `check --static-only`: `gdview::xref`, `uid`, `scene`, `declarations`. Pure gdview, lands first.
2. `check` end to end, `--slice` included, JSON report, exit codes, `--baseline`.
3. `api` from the extension dump: lookup, globals, search, miss suggestions, `--dump`.
4. `settings` and `refs` (offline, small, high value per line).
5. `resource schema` and `resource create` with the verify-by-reload loop.
6. `scene-tree --expand`.
7. `check --script`.
8. `doctor`, `import`.
9. `net` static, then `--explain`.
10. `run`.
