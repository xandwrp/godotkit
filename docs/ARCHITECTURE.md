# gdkit architecture

Three crates, one direction of dependency:

```
gdkit  (bin)  ──►  gdproject  ──►  gdview
 CLI, render        engine, process,     read-only: project, settings, syntax,
 exit codes         harnesses, records,  declarations, scenes, variant grammar,
                    every operation      glTF, API model, static net analysis
```

`gdview` never spawns a process or writes a file. `gdproject` spawns only through
`runner` and writes only under `.godot/gdkit/**`, scratch copies, or brand-new files.
`gdkit` contains no logic; a command file that grows past ~150 lines is a smell.

## Rules that are enforced by structure, not discipline

| Rule | Where it lives |
| --- | --- |
| Every engine invocation has a deadline | `runner::Invocation.deadline` is a required field; `process::run` takes `Duration`, not `Option` |
| No `Command::new(engine)` outside the runner | `process` is the only module that spawns; `runner` is the only module that calls `process` with an engine |
| Process identity is `(pid, start_time)` | `process::ProcessId`; a bare pid never crosses a function boundary |
| Terminate always waits | `process::terminate` returns `TerminateOutcome` after exit, escalating TERM → KILL |
| One Variant JSON grammar | `gdview::variant::VariantJson` in Rust, `harness/protocol.gd` in GDScript, golden-fixture tested against each other |
| One result envelope, version-checked | `protocol::Envelope`; `parse_envelope` rejects a version mismatch before decoding the payload |
| Tool failure vs project failure | `Err` = gdkit could not do its job (exit 2). `Ok(report)` with a failing verdict = exit 1 |
| Records never brick a command | `records::RecordStore::read_all` returns good records plus `problems`; writes are temp+rename |
| Never mutate an authored file | `workspace::publish_new_file` is create-new only; `IsolatedCopy` and `ArtifactDir` are the only other write paths |
| Platform support is explicit | `process.rs` has `compile_error!` for anything but Linux/macOS/Windows |

## Application flows

Each numbered step names the function that owns it. Arrows show which crate the
call crosses into.

### `gdkit check`

```
gdkit::commands::check
  1. ctx.workspace(args)                 gdview::Project::discover → gdproject::Workspace::open
  2. ctx.engine(ws, args)                Config::load → select_engine → Engine::attach (probe cache)
  3. CheckRequest::from(args, config)
  4. gdproject::check::run(ws, engine, req, observer)
       a. workspace.lock()
       b. IsolatedCopy::full | ::slice   (copy, no .godot/.git, symlinks refused)
       c. scan: gdview files query on the copy → manifest → ProjectIdentity.fingerprint
       d. run_engine  --editor --quiet --import                      phase Import
       e. run_harness ImportScan                                     phase Import (completion marker)
       f. class_cache_audit: gdview::declarations vs copy/.godot/global_script_class_cache.cfg
       g. run_harness Check (manifest, policy)                       phase ResourceLoading
       h. for --script: run_harness ScriptBootstrap (deadline)       phase ProjectScript
       i. for --scene:  run_engine <scene> --quit-after N (deadline) phase SceneSmoke
       every phase: preserve raw streams to ArtifactDir → diagnostics::parse → apply_ignore_rules → observer
       any failure before h skips h/i with reason
  5. render::emit(report)                stdout: human summary | one JSON doc
  6. Exit::from(report.outcome)          0 passed, 1 failed/incomplete; Err → 2
```

### `gdkit api Class member`

```
  1. workspace, engine (as above)
  2. gdproject::api::ProjectApi::load
       a. extension_fingerprint(root) + engine.fingerprint → cache key
       b. cache hit → ApiIndex from api-index.json
          miss → run_harness Api; if diagnostics has errors: use but do not cache
       c. gdview::declarations::index_project(project)
  3. lookup_member → Found | ClassMissing{suggestions} | MemberMissing{suggestions}
  4. emit; missing → Exit::Failed
--dump: load_native (or load_native_standalone with an empty IsolatedCopy) → print ApiIndex JSON
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

### `gdkit run --name server` and `gdkit inspect server --net`

```
run:
  1. workspace, engine
  2. gdproject::session::launch(spec)
       a. validate_name; refuse if latest generation is alive (ProcessId::is_alive)
       b. RecordStore.write(Launching)
       c. process::spawn(detached, own_group, log file, env GDKIT_PROBE_*)   --script runtime_probe.gd
       d. wait for ready file (deadline) → ProbeEndpoint
       e. RecordStore.update(Running, ProcessId, endpoint); ChildGuard::release
       failure after c: terminate → update(Failed) → Err
inspect:
  1. session::resolve(selector) → require Running + endpoint (re-check is_alive)
  2. probe::observe_network(endpoint, deadline)    one JSON line each way, token, request id
  3. emit
```

### `gdkit scenario start late_join`

```
  1. workspace, engine, Config.scenarios[name].validate
  2. gdproject::scenario::start
       a. RecordStore.write(Starting)
       b. servers:      session::launch → poll probe::collect_checkpoints until readiness or deadline
                        (AdapterError / transport timeout = not ready yet; process exit = fail fast)
       c. ports:        JSON pointer into server checkpoints → expand_argument for later participants
       d. clients, then late clients: same as b
       e. update(Ready)
       any failure: stop every launched participant (terminate, then kill) → update(Failed{cause}) → Ok(run) with Exit::Failed
```

### Offline commands (no engine, no gdproject engine path)

`scene-tree` → `gdview::scene::{parse, expand, compact_tree}`;
`autoloads` → `gdview::settings::Settings::autoloads`;
`animation list` → `gdview::gltf`;
`cache status` → `gdproject::cache::status` (reads sizes only);
`net --offline` → `gdview::net::analyze`.

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

## Test matrix

| Site | Offline (CI on 3 OSes) | Engine-backed (`GDKIT_TEST_GODOT`, `#[ignore]`) |
| --- | --- | --- |
| gdview (all modules) | fixtures + strings; every test | corpus only (`GODOT_SOURCE`) |
| gdproject::process | `sleep`/`cmd` subjects: deadline, tree kill, pid reuse, zombies, detach, guard | none |
| gdproject::engine, runner, api, check, session, scenario | `fake-godot` scripted by env vars; asserts exact argv, envelopes, timeouts, records | `real_engine_*`: harness correctness, golden fixtures |
| gdproject::protocol, diagnostics, records, workspace, config | pure | `protocol_gd` golden refresh |
| gdproject::resource, animation, probe | validation, echo mismatch, staging cleanup, fake TCP responder | round-trip every Variant type; probe under script error |
| gdkit | drives the binary; uniform flags, JSON purity, exit codes | none |

Test names in `tests/*.rs` are the acceptance checklist and are repeated in each
module's doc comment. A module is done when its stubs are un-ignored and green.

## Open decisions

- **RPC traffic capture.** `probe::NetworkEvent` covers peer lifecycle and spawns via
  MultiplayerAPI signals. Per-call RPC traffic needs either the profiler (only
  available with a debugger attached, which is what froze sessions on script errors
  in v0) or a Rust decoder for the remote-debugger wire format. Deferred until
  `real_engine_script_error_does_not_freeze_the_session` is green.
- **Watch mode / incremental check.** Not in the surface. `ProjectIdentity.fingerprint`
  exists so a future watch can decide what changed; nothing else anticipates it.
- **gdview as a separate repository.** It is a workspace member here for velocity.
  Split when its surface stops moving; nothing in gdproject depends on it being local.
