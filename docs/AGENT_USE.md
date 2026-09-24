# gdkit for agents

What an agent working in a Godot project with no editor open actually reaches
for, in the order it reaches for it, and what it does with the answer. This is
the priority list for implementation: if a command is not on this page, it can
wait.

The shape every command on this page shares:

- `--output json` puts exactly one JSON document on stdout and nothing else.
  Progress and errors go to stderr. An agent never has to strip prose.
- Exit code is the verdict. `0` the thing passed, `1` the thing failed and the
  JSON says why, `2` gdkit itself could not do the job (wrong engine path,
  missing config, hung process). An agent branches on this before reading JSON.
- Every path in output is `res://`, every location is `resource` + `line`, and
  every report carries `schema_version`. An agent can grep, sort, and diff.
- Every engine call has a deadline. A hung autoload produces a timeout in the
  report, never a hung agent.

## 1. `check`: the feedback loop

```sh
gdkit check --output json
gdkit check --slice scripts/player.gd --output json
gdkit check --slice scripts/player.gd --slice scenes/player.tscn --output json
```

Without an editor there are no red squiggles. `godot --check-only` parses one
script and knows nothing about scenes, resources, missing preloads, or a
`class_name` that moved. `check` imports a disposable copy of the project and
loads every script, scene, and resource in a fresh process, so it finds what the
editor would find, and it never touches the source project's `.godot`.

What the agent reads from the report:

```
outcome                       passed | failed | incomplete
phases[].id                   which step failed (import, resource_loading, …)
phases[].diagnostics[]        severity, message, resource, line, column, occurrences
failures[]                    kind + message, one line each, for the summary
counts                        how much was actually validated
artifact_dir                  raw engine output when the diagnostic is not enough
```

The loop: edit → `check --slice <what I touched>` → read `diagnostics` where
`severity == "error"` → fix the first `resource:line` → repeat → full `check`
before declaring done. `--slice` is what makes the loop fast: one script in a
few seconds instead of the whole project in a minute. A slice has no autoloads
or project settings unless `project.godot` is sliced in, so a missing global is
a real diagnostic there, and the agent knows to widen the slice rather than
"fix" the script.

`incomplete` is distinct from `failed` on purpose. It means the engine did not
report completion (crash, timeout, malformed output). The agent should not treat
it as "my change is wrong"; it should read `failures[].kind` and probably rerun.

## 2. `api`: stop guessing names

```sh
gdkit api CharacterBody3D move_and_slide
gdkit api CharacterBody3D
gdkit api search multiplayer
gdkit api WeaponDefinition            # project class_name scripts are included
gdkit api --dump --output json > .godot/gdkit/api.json
```

Agents hallucinate Godot method names, argument orders, and which class a
method is declared on. `api` asks the engine that will actually run the code,
including GDExtension classes and the project's own `class_name` scripts, and
merges them.

The answers that matter:

- A signature with the declaring class, so `move_and_slide` is known to come
  from `CharacterBody3D`, not `PhysicsBody3D`.
- Argument names, types, and defaults, so a call site can be written once.
- On a miss, exit `1` with `suggestions[]`. "Did you mean `set_deferred`" is the
  correction the agent needs, and it needs it to be machine-readable.

`--dump` once per engine+project and grep the file locally is the cheap mode.
The cache is keyed on the engine binary and the project's GDExtension
libraries, so a rebuilt extension is picked up without the agent knowing.

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

`schema` returns every field with `variant_type`, `class_name`, `element_type`,
`default`, `hint`, `enum_choices`, and `accepts` (the JSON shapes that field
takes). The agent fills a spec from that, never from memory.

`create` assigns every property in the engine, saves, reloads with the cache
disabled, and echoes each value back. Any divergence is exit `2` with the
offending `field` and `stage`, and nothing is written. It never overwrites an
existing file. The agent gets a resource that is right or gets nothing.

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

The report's `phases[]` entry for the script carries `outcome`
(`completed | failed | timed_out`), the diagnostics it printed, and the raw log
path. Runtime phases are skipped, and say so, when validation already failed.

## 6. `net`: the multiplayer map

```sh
gdkit net --output json
gdkit net --explain fire_weapon
gdkit net --offline                    # sources only, no engine
```

For a multiplayer project, "which RPCs exist, with what mode and transfer
settings, who calls them, what node paths must agree on both peers, what is
synchronized and when" cannot be reconstructed reliably by reading. `net`
reports it as typed observations with `unknowns[]` kept explicit rather than
guessed. `--explain` narrows to one method or node and lists every endpoint,
call site, and synchronizer that touches it.

## 0. `doctor`: run this first in an unfamiliar repo

```sh
gdkit doctor
```

Which engine will be used and why (`--godot`, `GDKIT_GODOT`, or `gdkit.toml`),
its version, whether the probe cache is warm, the project's warning policy,
which sessions are recorded and whether any record is corrupt. Thirty seconds
here saves the twenty minutes an agent otherwise spends debugging a wrong
engine path through a failing `check`.

## What the agent does not reach for

Durable named sessions and scenarios are built for humans watching a game over
time. An agent works in request/response and does not want a background process
it must poll and remember to stop. The shape it wants instead:

```sh
gdkit run --scene res://scenes/match.tscn --frames 120 --until /round_state/phase=playing --output json
```

Launch, run until N frames or a checkpoint condition, dump checkpoints and the
log, exit with the verdict. Same runtime probe, same checkpoint adapter, no
lifecycle to manage. Scenarios are then that shape with three participants and
a readiness order. This is the direction for `run`; the durable session
commands are kept for humans but are not on the agent's path.

## Implementation priority derived from this page

1. `check` end to end, `--slice` included, JSON report, exit codes.
2. `api` lookup, search, miss suggestions, `--dump`.
3. `resource schema` and `resource create` with the verify-by-reload loop.
4. `scene-tree --expand` (offline, lands early because it is pure gdview).
5. `check --script`.
6. `doctor`.
7. `net` static, then `--explain`.
8. `run` in the run-to-checkpoint shape.
