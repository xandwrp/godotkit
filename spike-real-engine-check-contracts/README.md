# Real-engine check contracts spike

Isolated experiment, not production implementation. Only files beneath this directory are owned by this spike. No fake engine scenarios, dependencies, production edits, or commits.

## Run

From the repository root:

```sh
python3 spike-real-engine-check-contracts/run.py
```

Python 3.9+ and `/usr/bin/godot` are required. Options: `--engine /absolute/path` and `--repeats 1..10` (default 3). The version assertion deliberately expects 4.7.2. On Linux, each engine is a new process group; deadlines kill that group and reap the process. Limits: version 5s, imports/cache verification 15s each, project scripts 3s each, kill/drain 3s. An outer `timeout 240s python3 spike-real-engine-check-contracts/run.py` can bound the whole default run.

Each run creates a new timestamped `artifacts/` directory. Fixtures, project caches, isolated HOME/XDG directories, raw stdout/stderr, class-cache snapshots, exact argv, exit status, timeout flag, elapsed time, diagnostics, and assertions all stay there. No warm project cache is reused between import cases. The driver intentionally retains failures. It does not invoke Cargo or change production code.

Files:

- `import_scan.gd`: deferred editor scan, initial settle, explicit rescan, second settle, manifest loads with `CACHE_MODE_REPLACE`, completion payload, normal quit.
- `script_bootstrap.gd`: deferred target load/validation, native base check, marker, `set_script`, explicit `_initialize`; user owns quit.
- `run.py`: real fixture generation, process deadlines, assertions and artifact capture.

## Evidence (2026-09-24)

Engine: `4.7.2.stable.arch_linux.ed1daf0bf`, `/usr/bin/godot`, Linux/headless.

**Final run:** `artifacts/20260924-183950/summary.json`: **106 checks passed, 0 failed, 41 engine invocations** (including version).

**Exploratory run:** `artifacts/20260924-183741/summary.json`: 79 passed, 15 failed. Its assertions exposed incorrect initial assumptions: editor `can_instantiate()` as a validity test, `--import` as a parser check, and no editor shutdown errors. Those logs are preserved, not rewritten. The final harness replaced the instantiability test with null-load telemetry, and the driver separately records validation and narrowly identified RID shutdown diagnostics. One intermediate command failed before engine launch due to Python indentation introduced during editing; corrected before the final run.

### Import scan

Matrix: direct cold scan and cold `--editor --import` followed by scan; valid and malformed-script projects; **three independent repetitions per combination, 12 projects total**. Fixtures contain two global classes with inheritance, a typed consumer, and an SVG preload that needs actual import. Broken fixtures add a malformed global class alongside valid ones.

All 12:

- Started without `.godot`.
- Completed with `scanning == false`, `importing == false`, expected manifest count, one completion envelope and exit 0.
- Had `.godot/global_script_class_cache.cfg` before quit and after process exit.
- Persisted `SpikeBase` and `SpikeChild` with expected resource paths and produced a `.ctex` import.
- Passed a **fresh runtime process** that resolves and instantiates `SpikeChild`, checks inherited state, and loads the imported texture, including projects containing the broken script.

All six broken scans emitted a parse error and failed-load diagnostic despite completion and exit 0. **`ResourceLoader.load` still returned a non-null Script for the malformed script** (`null_loads == []`). The broken class name itself also appeared in the cache. Neither successful load return, cache membership, completion nor exit zero proves script validity.

All six preliminary `--import` invocations exited 0 with no engine errors, including the three broken projects. They registered the class names and imported the SVG but did not validate that malformed script. The explicit manifest-loading phase is necessary.

All 12 editor-script scans emitted five `ERROR: N RID allocations of type '...' were leaked at exit.` lines after the stdout completion envelope, plus Canvas/CanvasItem/ObjectDB warnings on stderr. These occurred on valid and broken inputs. The driver preserves all of them as `engine_errors` and additionally separates the exact RID-leak line pattern into `cleanup_errors` for this experiment. It never applies that exception to runtime scripts. `--import` alone did not show those errors in this run.

**Interpretation:** this algorithm settled and persisted cache state reliably in the tested fixtures; this is not proof for arbitrary editor plugins, GDExtensions, threaded importers, or all Godot versions. Direct cold scan worked here, but that is insufficient evidence to remove the planned preliminary import stage. The experiment does not isolate which editor subsystem writes the cache: do not claim that loading scripts itself is what persists it.

### Script bootstrap

All project script tests run in a project with an autoload singleton. No preliminary editor import is needed for these fixtures.

| Real user script | Marker | Process result | Contract result |
| --- | --- | --- | --- |
| Normal `_initialize` / `quit(0)` | One, before user callback | 0, no errors | Success |
| Node base | None | Error envelope, exit 1 | Rejected before handoff |
| Runtime null call, deferred `quit(0)` | One | 0 **and SCRIPT ERROR** | Failure |
| Never calls quit | One | Killed at 3s deadline | Timeout |
| Autoload identifier + root node check | One, after autoload `_ready` | 0, no errors | Success |
| `quit(7)` | One | 7 | Failure; user's exit preserved |
| Parse error | None | Error envelope, exit 1, engine errors | Failure before handoff |
| Runtime null call, no scheduled quit | One | SCRIPT ERROR then deadline | Timeout with diagnostic |
| Inherits another SceneTree GDScript | One | 0, no errors | Success |
| User `_init` plus `_initialize` | One, before both | 0, no errors | Success |

The `_init` fixture establishes that `set_script(target)` executes user code. The marker must precede `set_script`, not merely precede the explicit `_initialize` call. The inherited fixture establishes that checking `get_instance_base_type() == &"SceneTree"` accepts script inheritance. Deferred loading lets autoload singleton identifiers resolve and observes `_ready` state before handoff.

## Recommended production contracts

### ImportScan: completed work is distinct from validated project

1. Run against a disposable project copy with a hard process-group deadline. Retain raw streams on every outcome.
2. Use `@tool extends SceneTree` under `--editor`; defer execution, yield at least one frame, wait while scanning **or** importing, call `scan()`, yield and wait again. Avoid fixed sleeps or `--quit-after` as completion signals.
3. Validate the manifest, load every `.gd` with `CACHE_MODE_REPLACE`, and emit exactly one typed completion envelope only once the work is done. Define `ok: true` here as **harness work completed**, not project validation passed. Include scanned count; null-load paths are useful but not sufficient validation.
4. Require valid completion, normal exit 0, no timeout, no unhandled validation errors, and a post-exit class-cache audit for phase success. Manifest/load operational failures must be explicit errors. A malformed project may complete scanning while the phase still fails.
5. Parse diagnostics from **both complete streams**, including after completion. Do not copy the legacy rule that discards everything after its marker or accepts nonzero exit after completion. Cross-stream ordering is not established by these separately captured streams.
6. Resolve editor teardown errors deliberately: either fix the teardown path, or add a narrowly scoped, version-tested ImportScan-only cleanup policy, preserving and counting suppressed diagnostics. The exact RID-leak messages here justify investigation, not broad suppression of `ERROR:` or all post-marker output. Until that policy exists, a strict all-errors-fail import verdict will reject even these valid fixtures. Runtime script success must retain the strict no-engine-errors rule.
7. Audit persisted class names, paths and inheritance after engine exit. Cache existence and entries are metadata, **not a parse-success signal**. Keep the fresh-process resource-loading phase. Do not use `can_instantiate()` to reject normal non-tool scripts while in editor mode.

### ScriptBootstrap: startup marker, not completion envelope

1. Defer target load until runtime autoload initialization. Validate argument count, Script load/instantiability, and SceneTree native base before handoff. Pre-handoff failures emit a structured error and exit nonzero without a startup marker.
2. Print the exact stdout line **`GDKIT_SCRIPT_STARTED`**, then `set_script(target)`, then call `_initialize()`. Emit no success envelope before or after user execution. No harness-controlled auto-quit: the user owns quit and exit code.
3. Define success as **normal exit 0 AND exactly one startup marker AND no engine errors on either stream AND no timeout**. An error envelope always fails. Missing startup at exit zero is incomplete/missing-start, never success. A timeout remains a timeout even when startup was observed; retain any runtime error as well.
4. Keep the process owner outside Godot. Runtime exceptions do not reliably exit or return nonzero. Enforce deadlines and kill/reap the process group. Never infer success from startup alone.
5. Give this harness a specialized result/runner path (or explicit protocol mode), not generic `HarnessRun<T>` requiring a success envelope. The current runner comments and architecture promise an envelope from every harness; that promise must change when implementing this contract. Marker parsing is a cooperative protocol, not a security boundary against user scripts spoofing output.

## Inspected baseline and limits

- `legacy/src/import_scan.gd`: initial settle, explicit scan, second settle, manifest load, completion file and warning marker.
- `legacy/src/script_bootstrap.gd`: deferred load then set_script/initialize, without base validation or startup marker.
- `legacy/src/check.rs`: temporary-script invocation and import completion/error filtering (including broad post-marker exclusion).
- `legacy/tests/check.rs`: existing real-engine autoload test.
- `crates/gdproject/src/harness/{import_scan,script_bootstrap,protocol}.gd`: scaffold stubs.
- `crates/gdproject/src/{check,runner}.rs` and `docs/ARCHITECTURE.md`: planned two-stage import, cache audit, engine phases, generic envelope assumptions.

This is engine-contract evidence, not validation of the unimplemented production runner. Error recognition is a small fixture-focused regex, not the production diagnostic parser. No fake scenarios or production tests were added. No claim is made about blocked autoloads, malicious child processes escaping process groups, missing `_initialize` overrides, user `_init` quitting/throwing, cache invalidation on moved classes, other platforms, or other engine versions.
