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

The command surface is exactly what [AGENT_USE.md](AGENT_USE.md) lists. Anything
not on that page is not stubbed, on purpose.

## Rules that are enforced by structure, not discipline

| Rule | Where it lives |
| --- | --- |
| Every engine invocation has a deadline | `runner::Invocation.deadline` is a required field; `process::run` takes `Duration`, not `Option` |
| No `Command::new(engine)` outside the runner | `process` is the only module that spawns; `runner` is the only module that calls `process` with an engine |
| Nothing outlives the gdkit invocation | `process::ChildGuard` kills the tree on drop; there is no pid record anywhere |
| Terminate always waits | `ChildGuard::terminate` returns `TerminateOutcome` after exit, escalating TERM → KILL |
| One Variant JSON grammar | `gdview::variant::VariantJson` in Rust, `harness/protocol.gd` in GDScript, golden-fixture tested against each other |
| One result envelope, version-checked | `protocol::Envelope`; `parse_envelope` rejects a version mismatch before decoding the payload |
| Tool failure vs project failure | `Err` = gdkit could not do its job (exit 2). `Ok(report)` with a failing verdict = exit 1 |
| Never mutate an authored file | `workspace::publish_new_file` is create-new only; `IsolatedCopy` and `ArtifactDir` are the only other write paths; `cache::refresh` is the only writer of the real `.godot` |
| Static before dynamic | `check` runs `gdview::xref` before any engine phase; `--static-only` needs no engine at all |
| Diagnostics have a stable identity | `Diagnostic.identity` excludes line and occurrence count, so `--baseline` survives edits |
| Platform support is explicit | `process.rs` has `compile_error!` for anything but Linux/macOS/Windows |

## Application flows

Each numbered step names the function that owns it.

### `gdkit check`

```
gdkit::commands::check
  1. ctx.workspace(args)                 gdview::Project::discover → gdproject::Workspace::open
  2. ctx.engine(ws, args)                Config::load → select_engine → Engine::attach   (skipped with --static-only)
  3. CheckRequest::from(args, config, baseline file)
  4. gdproject::check::run(ws, engine, req, observer)
       0. static_analysis: declarations + UidMap + ProjectGraph::load → gdview::xref::analyze   phase StaticAnalysis
       a. IsolatedCopy::full | ::slice   (copy, no .godot/.git, symlinks refused)
       b. scan: gdview files query on the copy → manifest → ProjectIdentity.fingerprint
       c. run_engine  --editor --quiet --import                      phase Import
       d. run_harness ImportScan                                     phase Import (completion marker)
       e. class_cache_audit: gdview::declarations vs copy/.godot/global_script_class_cache.cfg
       f. run_harness Check (manifest, policy)                       phase ResourceLoading
       g. for --script: run_harness ScriptBootstrap (deadline)       phase ProjectScript
       h. diagnostics::suggest with the cached api index (never dumps)
       i. baseline: partition by identity into new / carried / resolved
       every engine phase: preserve raw streams to ArtifactDir → diagnostics::parse → apply_ignore_rules → observer
       any failure before g skips g with a reason
  5. render::emit(report)                stdout: human summary | one JSON doc
  6. Exit::from(report.outcome)          0 passed, 1 failed/incomplete; Err → 2
```

### `gdkit api`

```
  1. workspace, engine
  2. gdproject::api::ProjectApi::load
       a. extension_fingerprint(root) + engine.fingerprint → cache key
       b. cache hit → ApiIndex from api-index.json
          miss → run_engine --dump-extension-api-with-docs (temp dir, --path project)
                 → gdview::api::ApiIndex::from_extension_api_json; not cached if the run printed errors
       c. gdview::declarations::index_project(project)
  3. one arg:  lookup_class, else lookup_global (utility function / global enum)
     two args: lookup_member → Found | ClassMissing{suggestions} | MemberMissing{suggestions}
     search:   search
  4. emit; a miss is Exit::Failed with suggestions
--dump: load_native (or load_native_standalone with an empty IsolatedCopy) → print ApiIndex JSON
```

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

One line on stdout: `GDKIT_RESULT:` + JSON

```json
{"protocol": 1, "harness": "check", "ok": true, "payload": {...}}
{"protocol": 1, "harness": "resource_create", "ok": false, "error": {"stage": "verify", "message": "...", "field": "properties.offset"}}
```

Payload Variants follow `gdview::variant` (`$variant`, `$ref`, `$resource`).
`harness/protocol.gd` is written next to every harness and is the only encoder.
Its output is frozen as a golden fixture by an engine-backed test and diffed
against `VariantJson::to_json` offline, so the two encoders cannot drift silently.

Harnesses: `probe`, `check`, `import_scan`, `resource_schema`, `resource_create`,
`runtime_probe` (the game-side half of `run`), `script_bootstrap`. The API index
needs no harness; it comes from the engine's own `--dump-extension-api-with-docs`.

## Test matrix

| Site | Offline (CI on 3 OSes) | Engine-backed (`GDKIT_TEST_GODOT`, `#[ignore]`) |
| --- | --- | --- |
| gdview (all modules) | fixtures + strings; every test | syntax corpus (`GODOT_SOURCE`); `extension_api.json` fixture refresh |
| gdproject::process | `sleep`/`sh`/`cmd` subjects: deadline, tree kill, log streaming, guard drop | none |
| gdproject::engine, runner, api, check, run | `fake-godot` scripted by env vars; asserts exact argv, envelopes, timeouts, artifacts | `real_engine_*`: harness correctness, golden fixtures, the GDExtension-in-dump question |
| gdproject::protocol, diagnostics, workspace, config | pure | `protocol_gd` golden refresh |
| gdproject::resource, probe, cache | validation, echo mismatch, staging cleanup, fake TCP responder, lock | round-trip every Variant type; probe under script error |
| gdkit | drives the binary; uniform flags, JSON purity, exit codes | none |

Test names in `tests/*.rs` are the acceptance checklist and are repeated in each
module's doc comment. A module is done when its stubs are un-ignored and green.

## Open questions

- **GDExtension classes in the API dump.** `--dump-extension-api-with-docs` under
  `--path` may or may not include classes registered by project extensions.
  `real_engine_dump_includes_project_gdextension_classes` answers it. If no, a
  ClassDB harness scoped to that delta is the follow-up; `ApiIndex.extension_classes`
  is where its result lands.
- **Multi-participant runs.** `run` is single-process. Servers plus clients is a
  list of `RunRequest`s with a readiness order and port hand-off, layered on top
  when there is a project that needs it. Nothing in the surface anticipates it.
- **gdview as a separate repository.** It is a workspace member here for velocity.
  Split when its surface stops moving; nothing in gdproject depends on it being local.
