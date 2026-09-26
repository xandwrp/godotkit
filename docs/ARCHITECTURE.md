# gdkit architecture

Three crates, one direction of dependency:

```
gdkit  (bin)  ──►  gdproject  ──►  gdview
 CLI, render        engine, process,     read-only: project, settings, syntax,
 exit codes         harnesses, run,      declarations, scenes, uids, xref,
                    check, api, resource variant grammar, API model, static net
```

`gdview` never spawns a process or writes a file. `gdproject` spawns only through
`runner` and writes only under `.godot/gdkit/**`, scratch copies, or brand-new files.
`gdkit` contains no logic; a command file that grows past ~150 lines is a smell.
A command is a stub until `src/status.rs` marks it `Ready`: help tags stubs and
`main` refuses them with exit 2. `status_table_matches_what_each_command_does`
fails if a `Ready` command panics or a stub stops panicking, so flip the entry
in the same change that implements the command.

The command surface is exactly what [AGENT_USE.md](AGENT_USE.md) lists. Anything
not on that page is not stubbed, on purpose. These flows describe the intended
architecture: `check`, `api`, `init`, and `config` are implemented end to end, but other
command scaffolds remain. Check's API-cache diagnostic enrichment is explicitly deferred.

## Rules that are enforced by structure, not discipline

| Rule | Where it lives |
| --- | --- |
| Every engine invocation has a deadline | `runner::Invocation.deadline` is a required field; `process::run` takes `Duration`, not `Option` |
| No `Command::new(engine)` outside the runner | `process` is the only module that spawns; `runner` is the only module that calls `process` with an engine |
| Scoped process ownership | `process::ChildGuard` cleans up on drop: POSIX process groups (excluding escaped descendants), Windows direct child only; abrupt supervisor death bypasses drop |
| Terminate always waits | `ChildGuard::terminate` reaps the direct child; POSIX escalates TERM → KILL, Windows kills the direct child immediately |
| One Variant JSON grammar | `gdview::variant::VariantJson` in Rust, `harness/protocol.gd` in GDScript, golden-fixture tested against each other |
| One result envelope, version-checked | `protocol::Envelope`; `parse_envelope` rejects a version mismatch before decoding the payload |
| Tool failure vs project failure | Startup/probe/configuration errors are `Err` (exit 2). Failed or incomplete check reports, including phase timeouts, exit 1 |
| Never mutate an authored file | `workspace::publish_new_file` is create-new only; `IsolatedCopy` and `ArtifactDir` are the only other write paths; probe metadata and artifacts use `.godot/gdkit`; check never seeds or updates the source import cache. The one in-place edit is the user's global config, by `global::GlobalConfig::{set_engine, unset_engine}` only (`gdkit config set`/`unset`), atomically and keeping comments. The engine runs on the real project only for `api`'s `--gdscript-docs` (imported projects, workspace lock, headless, no `--editor`, output to scratch); `real_engine_script_docs_leave_the_project_untouched` pins that it writes nothing there |
| Static before dynamic | `check` runs `gdview::xref` before any engine phase; `--static-only` needs no engine at all |
| Diagnostics have a stable identity | `Diagnostic.identity` excludes line (including lines embedded in resource parse messages) and occurrence count, and scratch-copy paths are rewritten to `res://`, so `--baseline` survives edits and runs |
| Platform scope is explicit | Runtime verified on Linux; macOS shares POSIX code but is not runtime-verified here. Windows Job objects are out of scope; no Windows process-tree cleanup guarantee |

## Application flows

Each numbered step names the function that owns it.

### `gdkit check`

```
gdkit::commands::check
  1. ctx.workspace(args)                 gdview::Project::discover → gdproject::Workspace::open
  2. build CheckRequest from args and baseline file
  3. ctx.engine(ws, args)                Config::load → select_engine → Engine::attach   (skipped with --static-only)
  4. gdproject::check::run(ws, engine, req, observer)
       0. static_analysis: declarations + UidMap + ProjectGraph::load → gdview::xref::analyze   phase StaticAnalysis
       a. IsolatedCopy::full | ::slice   (source assets included, no .godot/.git or cache seeding, symlinks refused)
       b. scan: default gdview files query on the copy → full-file manifest
       c. run_engine  --editor --quiet --import                      phase Import
       d. run_harness ImportScan --editor                            phase Import (scan/load scripts, envelope + editor extensions)
       e. class_cache_audit: gdview::declarations vs copy/.godot/global_script_class_cache.cfg
       f. run_harness Check (manifest, policy, editor extensions)    phase ResourceLoading (editor + runtime registry union)
       g. for --script: run_harness ScriptBootstrap (deadline)       phase ProjectScript (started marker, then process exit)
       h. API-cache diagnostic enrichment deferred (no API load or dump)
       i. baseline: partition by identity into new / carried / resolved
       every engine phase: preserve raw streams to ArtifactDir → diagnostics::parse → apply_ignore_rules → observer
       new static findings block engine phases; baseline-carried static findings permit them but retain the failed verdict
       failed engine phases skip all later phases with a reason
  5. render::emit(report)                stdout: human summary | one JSON doc
  6. Exit::from(report.outcome)          0 passed, 1 failed/incomplete; Err → 2
```

The inventory is not a hardcoded resource-extension list. ImportScan hands off
editor-recognized extensions; Check unions them with runtime loaders registered
during autoload startup, then counts attempted loads of eligible inventory entries.
Unknown extensions are not loaded. Raw streams and observation-order events are
persisted before verdict interpretation; zero exit alone does not hide diagnostics
or replace a required completion payload.

### `gdkit api`

```
gdkit::commands::api
  1. ctx.workspace, ctx.engine
  2. gdproject::api::load_native                                   cache: .godot/gdkit/api-index.json
       key: engine fingerprint + gdview::api::API_INDEX_SCHEMA_VERSION; corrupt or stale = miss
       miss, in a bare scratch dir via runner::run_projectless (no --path; under --path
       Godot 4.7.2 writes the dump into the project and aborts):
         a. --dump-extension-api-with-docs → gdview::api::ApiIndex::from_extension_api_json
         b. --doctool <dir>                → gdview::api::doc_xml::parse_class per file
                                           → ApiIndex::merge_doctool (@GDScript, property defaults, enum property types)
         either run failing or writing nothing usable is an error (exit 2); nothing is cached
  3. a class, member, or global the native index knows → answer from it alone
     otherwise (search, project classes, misses) → ProjectApi::with_scripts:
       gdproject::api::load_scripts                                cache: .godot/gdkit/api-scripts.json
         key: engine, schema, imported?, class cache, project.godot, every script's bytes
         imported (has .godot/global_script_class_cache.cfg) and the lock is free:
           run_engine --path <project> --doctool <scratch> --gdscript-docs res:// <placeholder scene>
         else: the same on IsolatedCopy::slice(scripts + project.godot)
         the placeholder scene skips resolving a uid:// run/main_scene, which otherwise aborts with an OS alert
         XML → ApiClass keyed by class_name, else autoload name (uid:// autoloads via UidMap), else res:// path
         scripts the engine did not document → gdview::declarations, from_engine: false, with the engine's reason
         a failed run → every script from source, not cached
       ApiIndex::add_scripts (api_type "script"; a class_name equal to an engine class is not added)
  4. gdview::api::answer::{lookup, lookup_member, search} → Answer (class | member | search | miss)
  5. emit; a miss is Exit::Failed with suggestions
--dump: load_native (or load_native_standalone outside a project) → print ApiIndex JSON
```

### `gdkit init` and `gdkit config …`

```
init:   ctx.workspace → refuse an existing gdkit.toml → ctx.engine (flag > env > global; probe, cache)
        → Config::write_initial (--godot pins it) | Config::write_initial_unpinned (follows the default)
config: GlobalConfig::locate (GDKIT_CONFIG_DIR > XDG_CONFIG_HOME/HOME > APPDATA)
        get/list: GlobalConfig::load → emit              (get exits 1 when unset)
        set:      ctx.standalone_engine(value) (probe) → GlobalConfig::set_engine → emit
        unset:    GlobalConfig::unset_engine → emit
```

`ctx.engine` and `ctx.standalone_engine` load the global config for every
engine-backed command; it is the last source `config::select_engine` consults.

### `gdkit refs res://x` and `gdkit settings …` (offline)

```
refs:     Project::discover → index_project + UidMap::build + ProjectGraph::load → xref::references_to → emit
settings: Project::discover → settings() → input_actions | layer_names | window | main_scene (uid via UidMap) | get
```

### `gdkit resource create --spec s.json --out res://x.tres`

```
  1. read spec → gdproject::resource::CreateSpec::from_json  (gdview::variant validation, offline)
  2. workspace, engine
  3. gdproject::resource::create
       a. destination: ResPath, .tres, not under .godot, does not exist
       b. stage temp file in the destination's directory
       c. run_harness ResourceCreate (spec path, staged path)  → echo of every property
       d. compare echo to spec in Rust (second verification)
       e. workspace::publish_new_file(staged, destination)      create-new, atomic
       any failure: remove staged, Err (exit 2; nothing published)
```

### `gdkit run --scene res://x.tscn --frames 120 --until /round/phase=playing`

```
  1. workspace, engine, RunRequest from args
  2. gdproject::run::run
       a. resolve scene (arg or project main scene); artifact dir; probe env (ready file, token, [run] adapter)
       b. runner::spawn_game → ChildGuard (owns the process until return)
       c. wait for ready file (ready_deadline) → ProbeEndpoint; not ready → terminate, Outcome::NotReady
       d. loop: probe::status; if --until: probe::checkpoints and test the pointer
                stop on frames >= max, condition true, try_wait() exit, or deadline
       e. final probe::checkpoints (+ probe::network with --net); guard.terminate(grace)
       f. diagnostics::parse over the log → Verdict
  3. emit RunReport; Exit from verdict
```

### `gdkit net` and `gdkit scene-tree` (offline)

`net` → `gdview::net::analyze(declarations, scenes, autoloads)` → optional `explain`.
`scene-tree` → `gdview::scene::{parse, expand, compact_tree}`.

## Harness protocol

Envelope-based harnesses emit one result line on stdout: `GDKIT_RESULT:` + JSON

```json
{"protocol": 1, "harness": "check", "ok": true, "payload": {...}}
{"protocol": 1, "harness": "resource_create", "ok": false, "error": {"stage": "verify", "message": "...", "field": "properties.offset"}}
```

`script_bootstrap` is the exception: it emits `GDKIT_SCRIPT_STARTED` before
`set_script` (including the target's `_init`), then calls its `_initialize` when
the script defines one.
There is no success envelope. User scripts must call `quit` before the deadline;
check validates the startup marker, process exit, and diagnostics. Bootstrap
failures before handoff use an error envelope. A phase timeout, crash, or
missing/malformed completion yields an incomplete report (exit 1), whereas
startup/probe/configuration errors yield exit 2.

Payload Variants follow `gdview::variant` (`$variant`, `$ref`, `$resource`).
`harness/protocol.gd` is written next to every harness and is the only encoder.
Its output is frozen as a golden fixture by an engine-backed test and diffed
against `VariantJson::to_json` offline, so the two encoders cannot drift silently.

Harnesses: `probe`, `check`, `import_scan`, `resource_schema`, `resource_create`,
`runtime_probe` (the game-side half of `run`), `script_bootstrap`. The API index
needs no harness; it comes from the engine's own `--dump-extension-api-with-docs`,
`--doctool`, and `--doctool --gdscript-docs`.

## Test matrix

The matrix includes intended coverage for scaffolds, not a claim that every test
is implemented. Runtime verification is Linux-only; it does not establish macOS
or Windows process-lifecycle guarantees.

| Site | Offline | Engine-backed (`GDKIT_TEST_GODOT`, `#[ignore]`) |
| --- | --- | --- |
| gdview (all modules) | fixtures + strings; every test | syntax corpus (`GODOT_SOURCE`); `real_engine_refresh_api_fixtures` (trimmed dump + doctool XML) |
| gdproject::process | `sleep`/`sh`/`cmd` subjects: deadline, tree kill, log streaming, guard drop | none |
| gdproject::engine, runner, api, check, run | `fake-godot` with per-executable scenario/log sidecars; asserts exact argv, envelopes, timeouts, artifacts | `real_engine_*`: harness correctness, golden fixtures, the API dump, script docs leaving the project untouched |
| gdproject::protocol, diagnostics, workspace, config, global | pure | `protocol_gd` golden refresh |
| gdproject::resource, probe, cache | validation, echo mismatch, staging cleanup, fake TCP responder, lock | round-trip every Variant type; probe under script error |
| gdkit | drives the binary; JSON purity, exit codes; engine tests build the fake helper once per run via offline Cargo into `target/tmp` (reused across runs; three-minute deadline) | none |

Test names in `tests/*.rs` are the acceptance checklist and are repeated in each
module's doc comment. Implemented module gates have complete tests, not blanket
completion of all workspace scaffolds. Real-engine tests remain opt-in; explicitly
deferred tests (including check API-cache enrichment) remain separate. Fake-engine
tests isolate scenarios beside copied executables rather than mutating global
environment variables. Real-engine check fixtures use clean copies with source
assets, never pre-seeded import caches.

## Open questions

- **GDExtension classes in `api`.** Deferred. The dump runs without a project
  (it must: under `--path`, 4.7.2 aborts), so project extension classes cannot
  appear in it. `ClassDB.class_get_api_type` exists, so a small harness can list
  the delta; `--doctool --gdextension-docs` could document it. The result would
  land in `ApiIndex.extension_classes`. Nothing on this machine has a
  `.gdextension` to test against yet.
- **Multi-participant runs.** `run` is single-process. Servers plus clients is a
  list of `RunRequest`s with a readiness order and port hand-off, layered on top
  when there is a project that needs it. Nothing in the surface anticipates it.
- **gdview as a separate repository.** It is a workspace member here for velocity.
  Split when its surface stops moving; nothing in gdproject depends on it being local.
