# gdkit poweruser roadmap

Implementation status, acceptance checklists, and decision records live in the
[poweruser progress tracker](poweruser-progress.md).

gdkit should make it quick to answer three questions: what does this project mean,
what happened when it ran, and did this change fix the problem? The best product
direction is a fast command-line interface backed by Godot's own knowledge, with
reliable runtime sessions and artifacts that humans, editors, CI, and agents can
all consume.

The recommendation is to deepen that loop before expanding into a large catalog
of editor commands. A successful workflow should connect a source diagnostic to
the affected scene, reproduce it with the necessary game state, capture the first
failure, and rerun the same scenario after a fix. This is a product judgment,
not a claim that competing tools cannot do these things.

## Current foundation

The repository already has useful architectural choices: a Rust CLI, a reusable
gdview syntax/scene frontend, explicit per-project engine selection, cached engine
compatibility probes, Git-aware file discovery, and cacheless disposable editor
imports. Resource validation also uses a fresh process, so persistent script-cache
state is never the authority for a successful check. These findings come from the
repository sources listed under Local evidence.

The September 10 validation changes add `check --strict-methods`, the equivalent
`[check] strict_methods = true` configuration, and explicit bounded scene startup
checks. The strict policy is installed in the checker constructor before autoload
loading. It uses Godot's unsafe-method diagnostic, preserves targeted suppression
and directory exclusions, and does not modify project settings on disk. The smoke
runner treats script errors as failures even when the child exits successfully.

The remaining limitations matter. Ordinary checks retain the project's warning
policy. Strict methods are not whole-program proof. A smoke check observes a
limited startup window, not an interaction scenario. Resource loading and import
can execute project code such as autoloads or tool scripts; only the selected
gameplay scenes are excluded from ordinary scene startup. Text logs remain the
fallback source of failure detection. Wall-clock limits currently cover scene
smoke processes, not every engine operation.

## Existing tools and integration choices

This landscape assessment examines documented capabilities and selected source,
not a comparative hands-on benchmark or an exhaustive market inventory. Upstream
pages and branches change; the source snapshot was consulted on September 10,
2026. Adoption counts and claims of feature completeness are not used to rank
products.

| Existing surface | Evidence | Implication for gdkit |
| --- | --- | --- |
| Godot CLI | Import, script checks, scene launch, export, debugger options, and movie recording already exist. [1] | Own lifecycle, diagnostics, configuration, and reproducibility around the engine. |
| Godot LSP/DAP and VS Code tools | Godot exposes language/debug adapters; the official extension documents debugging and runtime inspection. [2][3] | Reuse semantic navigation and debugger capabilities after capability negotiation. |
| GDScript Toolkit | Independent parser, linter, formatter, and complexity metrics. [4] | Differentiate through project and runtime understanding; keep formatting compatibility explicit. |
| GUT and GdUnit4 | Both provide command-line test execution; GdUnit4 also documents scene-runner workflows. [5][6][22] | Add adapters to existing suites instead of requiring a new assertion framework. |
| Coding-Solo Godot MCP | Documents project launching and debug-output capture alongside project operations. [7] | A basic launch/log wrapper is an established capability, not sufficient differentiation. |
| hybridindie Godot MCP | Its architecture describes an editor bridge and an opt-in runtime probe for live interaction. [8] | Study the request/session boundary and state freshness rules. |
| letsagents Godot MCP | Documents runtime observation and action; its plugin automatically registers a runtime autoload. [9] | Convenient setup does not eliminate game-side instrumentation. Make that dependency explicit. |

The recommended integration posture is selective reuse. Godot owns GDScript
semantics. gdview owns lossless syntax and text-scene structure. Test frameworks
own their assertions. gdkit owns the coherent workflow and result contract. A
future MCP interface should call that same core rather than become a second
implementation of project operations.

## Product priorities

The ranking below reflects engineering judgment about daily usefulness, fit with
the existing code, and dependency order. It is not based on market-size estimates.
Effort is relative: small means a bounded feature, medium crosses several existing
components, and large introduces a durable subsystem.

| Priority | Capability | User-visible result | Effort | Main uncertainty |
| --- | --- | --- | --- | --- |
| P0 | Structured diagnostics and coverage | Every failure has a phase, source, severity, and machine-readable result. | Medium | Engine output differences and startup/shutdown coverage |
| P0 | Shared engine process supervisor | Timeouts, cancellation, output capture, and cleanup work across all commands. | Medium | Cross-platform descendant lifecycle |
| P1 | Engine-aware API lookup and doctor | Ask what a class supports in the actual selected engine. | Small/medium | Project GDExtension and feature-override cache identity |
| P1 | Named run sessions and runtime inspection | Launch, inspect, stop, and compare a game without losing context. | Large | Public adapter coverage versus private debugger dependencies |
| P1 | Scene/resource dependency queries | Explain where a resource is used and where a property came from. | Medium/large | Dynamic paths, binary resources, inheritance, and UIDs |
| P2 | Scenario runner and multiplayer orchestration | Reproduce a bug across a server and multiple clients. | Large | Project-defined readiness and state contracts |
| P2 | Performance and visual captures | Compare an exact scenario before and after a change. | Medium/large | Rendering variability and sampling accuracy |
| P2 | Incremental watch mode | Fast feedback with explicit invalidation and trustworthy full checks. | Medium | Transitive dependency completeness |
| P3 | Transactional scene refactoring | Rename/move with a reviewable patch and native editor undo. | Large | Dynamic references and conflicting unsaved buffers |
| P3 | Export verification and framework adapters | One reproducible check/build/test entry point. | Medium | Platform SDKs, templates, and framework versions |

## 1. A result contract that can be trusted

Create a `CheckReport` model before adding more checking flags. It should identify
the engine fingerprint, project snapshot, check policy, requested phases,
completed phases, skipped phases, failures, and artifact locations. Represent
diagnostics as records with severity, engine code where available, message,
resource path, line, column when available, stack frames, process/session identity,
timestamp, and occurrence count.

Keep original stdout and stderr as artifacts. Normal display can consolidate
duplicate messages, but consolidation must retain occurrence counts and original
order in the event record. The current diagnostic display deduplicates strings,
which is useful for readability but insufficient for finding a repeated runtime
failure. Missing completion, tool failure, and a passing check must remain
different outcomes.

Add `--output json` for one final report and an event-stream mode for watches and
live sessions. Keep the existing exit-code convention: 0 means the requested
checks passed, 1 means validation/runtime failure, and 2 means tooling or
configuration failure. Include a schema version. Standard output should contain
only the chosen machine format in that mode; human progress belongs on stderr.

Godot's `Logger` API supplies error type, source information, and script
backtraces, and the current import harness already uses it. Prefer structured
capture where supported, while retaining process-output scanning for diagnostics
before registration, after teardown, or on engines without the capability. [10]
The checker's failure decision must never depend only on a process exit code.

Acceptance cases include the original missing method, zero-exit runtime errors,
errors on either stream, ordinary warnings, repeated errors, malformed completion,
crashes, and a stopped operation. Each case should produce equivalent meaning in
human and JSON output. Suppressed diagnostics should be accounted for explicitly.

## 2. One engine process supervisor

Move process launching out of individual commands. The supervisor should own
launch arguments, process identity, output spooling, deadlines, cancellation,
termination, exit status, and artifact retention. Apply it to compatibility
probes, disposable editor imports, resource checking, smoke checks, exports, and
test adapters.

On Windows, investigate Job Objects with kill-on-close and assignment before the
engine can create descendants. On Unix, investigate process groups and group
termination. These are proposed implementations, not claims that the current
runner provides equivalent behavior on all platforms. The current Windows smoke
timeout uses a targeted process-tree termination and has a passing blocked-ready
fixture. Cross-platform lifecycle behavior needs its own integration tests.

Use a retained session identity, not only a PID, for commands issued later.
Distinguish completion from early cancellation. Add a bounded disk-output policy
so a noisy game cannot consume unlimited storage; retain the first failure plus a
tail, and report truncation. Stream notifications while capturing instead of
waiting for the process to end before revealing an error.

Acceptance means a blocked autoload, a stuck importer, and a stuck scene each
terminate within their configured deadline, with useful partial logs and no
owned processes left behind. A canceled operation must not stop another running
editor or game. Failures during spawn, output setup, polling, and cleanup need
explicit behavior.

## 3. API lookup from the selected engine

Proposed commands include `gdkit api RichTextLabel`, `gdkit api RichTextLabel clear`,
and `gdkit api search tornado`. Return signatures, inherited origin, properties,
signals, enum values, and engine identity. A nonexistent method should be an
ordinary negative query result with useful nearby candidates, not an invented API
or a generic parser failure.

ClassDB exposes native methods, argument metadata, properties, and inheritance.
Release builds can provide less method metadata than editor builds, so the API
index should record its capabilities. [11] Cache native metadata per engine and
project extension configuration; keep script-defined APIs separate from native
APIs. Do not assume a ClassDB miss proves a call on an object is invalid, because
an attached script or subclass can extend that object.

A local isolated probe enumerated 1,054 native classes and 17,008 declared methods
in both tested engines. Enumeration took 47.618 ms in official 4.7.2 and 55.034 ms
in custom 4.7.3-rc. These are single observations inside the engine, excluding
startup and index serialization, not comparative benchmarks. Both reported
`RichTextLabel.clear` present, `push_tornado` absent, and the unsafe-method warning
level at its default 0.

Pair this with `gdkit doctor`: show the engine selection source, actual executable,
version, capabilities, worker state, import problems, and relevant warning policy.
Check option support rather than assuming it from the version string: Godot's
documentation states that unknown command-line arguments can be ignored. [1]
An API mismatch should point to the configured engine, not suggest installing a
random different engine.

Acceptance includes inherited methods, custom engine classes, GDExtension changes,
engine replacement at the same path, and consistent results after cache rebuild.
Lookup should work without entering gameplay scenes. Documentation matching the
selected version can supplement metadata, with the engine remaining authoritative.

## 4. Named runtime sessions

Design `gdkit run`, `gdkit sessions`, `gdkit inspect`, and `gdkit stop` around a
single durable session model. A session records the project, engine, scene,
arguments, renderer, run ID, source fingerprint, log artifacts, and lifecycle.
Windowed play should be a first-class workflow; headless operation should be
explicit when launching an interactive development session.

Start with read-only observations: runtime tree, selected node properties,
autoload state, script errors, and source locations. Runtime node IDs become
invalid across a restart, so responses must include session and generation.
Every sample should include collection time or frame/tick identity. A stale
cached property must not appear as a fresh answer.

Godot already provides LSP and DAP endpoints, with the active project hosted by
an engine instance. [2] Use LSP for definitions, references, and rename candidates;
use DAP for supported debugger interactions. A breakpoint that pauses `_ready()`
must be installed before startup advances, not opportunistically after play.
Evaluate negotiated adapter capabilities and actual startup ordering in a spike.

For arbitrary live state, there are two implementation routes. Public
`EditorDebuggerPlugin` and `EngineDebugger` provide a custom message channel
between editor and game. [12][13] A small game-side probe can answer tree, property,
monitor, and input requests using that channel. Alternatively, native debugger
messages expose useful existing operations, but their wire format should be
treated as an engine-versioned internal interface.

The local engine has scene-tree requests, object inspection, property changes,
and screenshot message handlers. That establishes feasibility, not a stable
cross-version contract. The recommendation is a public probe for routine runtime
features, DAP for standard debugging, and an optional versioned native adapter
only for capabilities that justify its maintenance cost.

Bridge setup must state whether it adds an editor plugin or runtime autoload.
Separate observation from mutation: setting a property or calling a method can
change the game. Match mutations to a session and return the resulting state.
Use editor undo for changes to authored scenes, and keep runtime-only tuning
distinct from saving an asset.

## 5. Scene and resource intelligence

Build a typed project graph over the existing gdview parser. Nodes should include
scripts, scenes, external/subresources, UIDs, native types, script classes, and
autoloads. Edges should explain their origin: preload, scene instance, inheritance,
attached script, saved signal connection, or serialized resource reference.
Literal dynamic loads can be indexed; computed paths remain explicitly unresolved.

Useful proposed queries are `gdkit refs <resource>`, `gdkit scene explain <scene>`,
and `gdkit scene diff <old> <new>`. The explanation should identify the owning
scene, inherited source, local override, script type, and relevant resource
sharing. This would turn a text dump into an explanation of why a node behaves
as authored.

ResourceUID maintains identity-to-path mappings across resource moves. [14]
Combine engine-resolved identity with text evidence instead of treating every UID
string as an independent asset. A candidate for being unused is only a candidate
when runtime path construction or external content can still reference it.
Deletion should not be an automatic consequence of a static graph miss.

Resource sharing deserves a dedicated inspection. Godot resources are commonly
shared, and local-to-scene resources have different instancing behavior. [15]
Show which scene instances share a material or mesh, whether an override creates
an independent resource, and which assignment introduced it. This is particularly
useful for visual tuning where changing one instance unexpectedly changes others.

Acceptance should cover inherited scenes, repeated instances, unique names,
subresources, UID-only references, binary resources, cycles, case differences,
and scripts whose classes are discovered through imports. Every graph edge should
carry a source location or an engine-resolution explanation.

## 6. Reproducible scenarios and multiplayer

Extend smoke checks into opt-in named scenarios. Start with project-owned fixture
scenes and a tiny result protocol: setup, ready, checkpoint, assertion, finish.
Tests should wait for actual readiness or signals, not a fixed number of seconds.
Allow existing GUT/GdUnit4 suites to plug into the same reporting and session
supervision without imposing either framework on every project. [5][6]

A high-value multiplayer scenario is: start one server, start two clients, wait
for both to join, change shared state, join a late client, compare selected state,
then disconnect a peer. Godot's high-level multiplayer APIs expose peer identity
and connection signals, but the meaning of synchronized game state is
project-specific. [16] gdkit should orchestrate participants and compare declared
checkpoints, while game adapters define the state contract.

Each process needs a distinct role, log stream, and run identity. Provide
project-configured launch arguments and per-instance data locations so clients do
not overwrite each other's settings. Allocate network endpoints deliberately and
capture the configuration used. Dedicated-server feature settings and export
behavior need to be represented separately from simply launching headlessly. [17]

Input playback needs exact semantics. `Input.action_press()` updates action state
but does not invoke `_input()`; `parse_input_event()` can feed actual input events
to the game. [18] The scenario format must distinguish those operations, input
press/release timing, process frames, and physics ticks. It should also preserve
the distinction between two separately bound actions.

Call the initial product repeatable scenario execution, not universal deterministic
replay. Fixed time steps do not by themselves specify random state, filesystem
inputs, threaded ordering, network arrival, or platform physics behavior. Record
seeds and environment information, and define tolerances for compared quantities.
Exact replay should be an explicit per-project capability with its own evidence.

Acceptance includes late join, disconnect during setup, occupied port, crashed
client, failed assertion, readiness timeout, and clean shutdown of every owned
participant. The failure artifact should show which participant first diverged
and the relevant checkpoint values.

## 7. Performance and visual evidence

Provide named captures with a warmup interval, measurement interval, renderer,
resolution, engine/build identity, and scenario checkpoint. Godot's Performance
API exposes standard and custom monitors, but some update slowly or are unavailable
in release builds. [19] Do not represent a repeated cached monitor value as a new
high-frequency sample or mix debug-only zeroes into a release comparison.

Report frame-time distributions and spikes rather than only average FPS. Capture
memory/resource counts at meaningful checkpoints, such as after repeated scene
transitions or rounds. Correlate changes with a scenario, not an arbitrary idle
screen. Define project budgets for intentional persistence; a growing round-long
effect count is not automatically a leak.

Visual verification needs actual rendered output. Godot's movie tools can produce
frame sequences and fixed-step captures. [20] Use explicit renderer/resolution,
warmup, camera/state setup, and ignored regions for changing UI. Keep original
images with diffs, and allow thresholds for nondeterministic pixels. A headless
startup pass must not be presented as proof that shadows, mirrors, materials, or
GPU-specific shaders look correct.

Acceptance compares the same checkpoint and metadata. A renderer mismatch should
be reported as a comparison precondition problem, not as a visual regression.
Performance budgets should be calibrated through repeated local runs before CI
enforces them.

## 8. Watch mode, refactoring, and exports

Watch mode should debounce edits, publish diagnostic deltas, and invalidate by
content and dependency, not just timestamps. Retain a full-check path as the
reference. Changes to engine binaries, warning policy, global classes, autoloads,
extensions, import settings, or dependencies must invalidate the appropriate
state. Persisting an editor is useful; persisting an unqualified pass result is
a different and much stronger correctness claim.

Refactoring should generate a reviewable transaction with expected source hashes.
Start with constrained operations such as renaming an unambiguous literal scene
reference. Identify unresolved dynamic references in the preview. For a live
editor, use the native undo/redo manager and detect unsaved conflicting buffers;
Godot exposes a dedicated manager for editor action history. [21] File-only mode
should preserve source formatting and unrelated edits.

Exports should wrap the configured engine and preset, capture all diagnostics,
and verify expected artifacts. Record export-template and SDK prerequisites.
Keep packaging separate from publishing. C# projects need an explicit .NET build
adapter and GDExtension projects need their native build/test integration; a
successful GDScript check must not imply those builds passed.

## Proposed architecture

| Layer | Responsibility | Boundary |
| --- | --- | --- |
| CLI / future MCP / editor UI | Arguments, presentation, transport | No independent validation logic |
| Core commands | Check, query, run, inspect, scenario, capture | Return typed reports and events |
| Project model | Files, graph, engine identity, settings, capabilities | Preserve source provenance and unknowns |
| Engine supervisor | Processes, worker lifecycle, cancellation, artifacts | Own only launched/registered sessions |
| Engine adapters | Harness, LSP, DAP, optional probe/native debugger | Negotiate and test capabilities |
| gdview | Lossless syntax and text-scene structure | No duplicate Godot type system |

Keep one project service where persistent state is helpful, with idle shutdown
and explicit status/stop commands. Use fresh engine processes for authoritative
checks until incremental semantic correctness is established. Native API metadata,
scene graphs, and active game state need different cache identities and lifetimes.

For a future bridge, include request IDs, session generations, deadlines, and
bounded response sizes. Expose read-only tools by default and explicit mutation
operations. Avoid a generic unrestricted eval operation as the primary interface;
typed requests are easier to reason about, test, and replay. An advanced eval
escape hatch can remain a separately identified capability.

## Delivery sequence and decision gates

These are suggested next commits or small commit groups, not work already shipped.

| Order | Deliverable | Finish line |
| --- | --- | --- |
| 1 | Shared diagnostic/report types and JSON output | Existing fixtures have stable structured outcomes and preserved raw logs. |
| 2 | Shared process supervisor | Probe/import/check/runtime all have tested cancellation and deadlines. |
| 3 | Doctor and capability inventory | Explains engine, worker, warning policy, and unsupported features. |
| 4 | Native API index/query | Correct inherited signatures on both official and custom engines. |
| 5 | Named run/session commands | Launch, logs, status, stop, restart, and stale-session rejection work. |
| 6 | Read-only runtime probe spike | Fresh tree/properties/errors with verified startup coverage. |
| 7 | Project graph and reference query | Source-backed results for scenes, scripts, UIDs, and overrides. |
| 8 | Scenario completion protocol | Signal-based readiness, bounded assertions, durable failure artifacts. |
| 9 | Multi-process scenario orchestration | Server/two clients/late join fixture with isolated state and cleanup. |
| 10 | Monitor and rendered capture adapters | Comparable artifacts with explicit environment metadata. |
| 11 | Incremental watch | Matches a fresh reference check across invalidation fixtures. |
| 12 | Framework/export adapters and transactional edits | Clear build coverage and reviewable changes with rollback/undo. |

The first major decision gate is after the runtime probe spike. Compare public
probe, DAP, and native debugger access on a small feature matrix: startup errors,
live tree, properties, breakpoints, frame locals, input, screenshots, and multiple
sessions. Choose based on demonstrated capabilities and maintenance cost.

The second is after scenario orchestration. Measure how long it takes to recreate
and diagnose a real multiplayer failure. If the tool cannot preserve a useful
repro and explain the first divergence, more surface commands should wait.

Suggested performance targets are under 100 ms for cached local metadata queries
and under one second for warm feedback on a representative small project. These
are targets to validate, not current guarantees. Track cold/warm median and p95,
engine memory, and cancellation time. Large projects and import-heavy changes
need separate baselines.

Success should be judged by task completion: catching the missing method before
play; locating an inherited property override; recreating a late-join state bug;
finding the source of a frame-time spike; proving a visual fix at the same camera
checkpoint; and stopping a failed run without leaving orphan processes.

## Local evidence and validation

- [Checker](../src/check.rs) and [harness](../src/check.gd): resource phases,
  strict policy, diagnostics, and bounded scene execution.
- [Import worker](../src/import_worker.gd) and [supervision](../src/import_worker.rs):
  custom logging, persistent editor requests, and worker lifecycle.
- [Engine selection](../src/engine.rs), [CLI](../src/cli.rs), and
  [Cargo dependencies](../Cargo.toml): current subsystem boundaries.
- [Regression fixtures](../tests/strict_methods.rs): uncalled typed method,
  valid native/custom methods, targeted suppression, preloads/autoloads,
  configuration, real zero-exit runtime failure, timeout, and CLI constraints.
- Ordinary Rust tests and Clippy passed. Real-engine checker/import-worker tests
  passed on official 4.7.2. Strict/runtime integration passed on official 4.7.2
  and custom 4.7.3-rc on Windows. Other operating systems were not exercised.
- Local engine source inspection: `P:/godot/modules/gdscript/gdscript_analyzer.cpp`
  around 3751 contains the unsafe-method diagnostic path;
  `P:/godot/main/main.cpp` constructs the script loop before autoload loading;
  `P:/godot/scene/debugger/scene_debugger.cpp` contains runtime scene handlers;
  `P:/godot/modules/gdscript/language_server/gdscript_language_protocol.cpp`
  registers definition, references, and rename requests. These are local-source
  observations and should be rechecked when the fork changes.

## Sources

Sources below are primary documentation or project-maintainer material, accessed
September 10, 2026. Stable documentation and repository branches are moving
references; validate selected-engine capabilities before implementing against them.

[1]: https://docs.godotengine.org/en/stable/tutorials/editor/command_line_tutorial.html
[2]: https://github.com/godotengine/godot-docs/blob/master/tutorials/editor/external_editor.rst
[3]: https://github.com/godotengine/godot-vscode-plugin
[4]: https://github.com/Scony/godot-gdscript-toolkit
[5]: https://gut.readthedocs.io/en/latest/Command-Line.html
[6]: https://github.com/godot-gdunit-labs/gdUnit4/blob/master/documentation/doc/_advanced_testing/cmd.md
[7]: https://github.com/Coding-Solo/godot-mcp
[8]: https://github.com/hybridindie/godot-mcp/blob/main/docs/architecture.md
[9]: https://github.com/letsagents/godot-mcp/blob/main/addons/godot-mcp/plugin.gd
[10]: https://docs.godotengine.org/en/stable/classes/class_logger.html
[11]: https://docs.godotengine.org/en/stable/classes/class_classdb.html
[12]: https://docs.godotengine.org/en/stable/classes/class_editordebuggerplugin.html
[13]: https://docs.godotengine.org/en/stable/classes/class_enginedebugger.html
[14]: https://docs.godotengine.org/en/stable/classes/class_resourceuid.html
[15]: https://docs.godotengine.org/en/stable/classes/class_resource.html
[16]: https://docs.godotengine.org/en/stable/tutorials/networking/high_level_multiplayer.html
[17]: https://docs.godotengine.org/en/stable/tutorials/export/exporting_for_dedicated_servers.html
[18]: https://docs.godotengine.org/en/stable/classes/class_input.html
[19]: https://docs.godotengine.org/en/stable/classes/class_performance.html
[20]: https://docs.godotengine.org/en/stable/tutorials/animation/creating_movies.html
[21]: https://docs.godotengine.org/en/stable/classes/class_editorundoredomanager.html
[22]: https://github.com/godot-gdunit-labs/gdUnit4/blob/master/documentation/doc/_advanced_testing/sceneRunner.md

1. Godot Engine, [Command line tutorial][1].
2. Godot Engine, [External editor documentation][2].
3. Godot Engine, [Godot Tools for Visual Studio Code][3].
4. Scony and contributors, [GDScript Toolkit][4].
5. GUT maintainers, [Command Line][5].
6. godot-gdunit-labs, [GdUnit4 Command Line Tool][6].
7. Coding-Solo and contributors, [Godot MCP][7].
8. hybridindie and contributors, [Godot MCP bridge architecture][8].
9. letsagents and contributors, [Godot MCP plugin implementation][9].
10. Godot Engine, [Logger class reference][10].
11. Godot Engine, [ClassDB class reference][11].
12. Godot Engine, [EditorDebuggerPlugin class reference][12].
13. Godot Engine, [EngineDebugger class reference][13].
14. Godot Engine, [ResourceUID class reference][14].
15. Godot Engine, [Resource class reference][15].
16. Godot Engine, [High-level multiplayer][16].
17. Godot Engine, [Exporting for dedicated servers][17].
18. Godot Engine, [Input class reference][18].
19. Godot Engine, [Performance class reference][19].
20. Godot Engine, [Creating movies][20].
21. Godot Engine, [EditorUndoRedoManager class reference][21].
22. godot-gdunit-labs, [GdUnit4 Scene Runner][22].
