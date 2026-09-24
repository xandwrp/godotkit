# gdkit poweruser progress tracker

Companion to the [poweruser roadmap](poweruser-roadmap.md). The roadmap explains
direction and tradeoffs; this document tracks delivery, validation, and decisions.
Command names below are proposed interfaces until their milestone is delivered.

Created: 2026-09-10. Last updated: 2026-09-11.

## How to maintain this tracker

- Use `Planned`, `In progress`, `Blocked`, `In review`, `Done`, or `Deferred` for status.
- Check an item only when its implementation or validation is complete. Link proof
  in the milestone's evidence field using the item ID, commit, test, or artifact.
- Mark a milestone `Done` only when all required items and its acceptance checks
  are complete. Record any agreed scope change in the decision log; do not silently
  count deferred work as delivered.
- Update the dashboard and milestone record together. Record blockers with the
  dependency or decision needed to resume. Use owner and date fields when work starts.
- Keep validation claims scoped to the tested engine, platform, build, and renderer.
  An unchecked acceptance case is unverified, even if nearby cases pass.
- Progress is completed milestones out of 12. Individual checkboxes vary in size,
  so their total is not an estimate of effort or time remaining.

Initial status is transcribed from the roadmap's proposed delivery sequence, not
a fresh implementation audit. Existing capabilities below are roadmap-reported
baseline context and do not count toward new milestone completion.

## Dashboard

**Recorded delivery: 0/12 milestones complete.** M01 and its M02 capture dependency
are in progress. No target dates or blockers recorded yet.

Dependencies below are a working execution plan derived from the roadmap. They
can be revised with a recorded reason. Delivery order follows the roadmap;
independent design work can proceed before prerequisite implementations finish.

| Order | Milestone | Priority | Status | Prerequisites | Finish line |
| --- | --- | --- | --- | --- | --- |
| 1 | [M01 Reports and diagnostics](#m01-reports-and-diagnostics) | P0 | In progress | Baseline | Stable structured outcomes and raw logs |
| 2 | [M02 Process supervisor](#m02-process-supervisor) | P0 | In progress | M01 contract | Bounded operations, cancellation, owned-process cleanup |
| 3 | [M03 Doctor](#m03-doctor) | P1 | Planned | M01, M02 | Explain selected engine, capabilities, and worker health |
| 4 | [M04 Native API lookup](#m04-native-api-lookup) | P1 | In progress | M02, M03 identity/capabilities | Correct inherited signatures on official/custom engines |
| 5 | [M05 Named sessions](#m05-named-sessions) | P1 | In progress | M01, M02 | Launch, logs, status, stop, restart, stale-session rejection |
| 6 | [M06 Runtime probe](#m06-runtime-probe) | P1 | Planned | M03, M05 | Fresh observations and verified startup coverage |
| 7 | [M07 Project graph](#m07-project-graph) | P1 | In progress | M01, engine identity, gdview | Source-backed references, UIDs, inheritance, overrides |
| 8 | [M08 Scenario protocol](#m08-scenario-protocol) | P2 | In progress | M01, M02, M05; G1 for bridge use | Signal readiness, bounded assertions, failure artifacts |
| 9 | [M09 Multiplayer scenarios](#m09-multiplayer-scenarios) | P2 | Planned | M08 | Server, two clients, late join, isolated state, cleanup |
| 10 | [M10 Performance and visuals](#m10-performance-and-visuals) | P2 | Planned | M08, selected G1 adapters | Comparable captures with environment metadata |
| 11 | [M11 Incremental watch](#m11-incremental-watch) | P2 | Planned | M01, M02, M07 | Results match fresh checks across invalidation cases |
| 12 | [M12 Adapters and refactoring](#m12-adapters-and-refactoring) | P3 | Planned | M01, M02, M07, M08 | Explicit build coverage, verified exports, transactional edits |

| Gate | Status | When | Decision needed |
| --- | --- | --- | --- |
| [G1 Runtime integration](#g1-runtime-integration) | Pending | After M06 | Select supported probe/DAP/native capabilities |
| [G2 Reproduction usefulness](#g2-reproduction-usefulness) | Pending | After M09 | Demonstrate useful repro and first divergence before expanding commands |

## Roadmap-reported foundation

These are existing starting points, not newly completed tasks. See the roadmap's
[foundation](poweruser-roadmap.md#current-foundation) and
[local evidence](poweruser-roadmap.md#local-evidence-and-validation) for provenance.

| Existing capability | Constraint to preserve |
| --- | --- |
| Rust CLI and reusable gdview frontend | Keep syntax/scene structure separate from Godot semantics |
| Explicit project engine selection and cached probes | Identify the actual selected executable and cache validity |
| Git-aware discovery and disposable editor import | Checks never trust source `.godot` state |
| `check --strict-methods` and config equivalent | Install policy before autoloads; preserve suppression/exclusions; do not edit project settings |
| Bounded scene startup and zero-exit error detection | Startup coverage is limited; ordinary checks retain project warning policy |
| Existing strict/runtime fixtures | Roadmap reports official/custom engine validation on Windows only |

## Milestone checklists

### M01 Reports and diagnostics

[Roadmap section 1](poweruser-roadmap.md#1-a-result-contract-that-can-be-trusted).

Status: In progress | Owner: Codex | Started: 2026-09-10 | Target: - | Completed: -
Blocker: None recorded. Evidence / commits: M01.01-M01.02 report schema and
serialization tests in `src/report.rs`; raw per-phase stream artifacts and
occurrence-aware display consolidation in `src/check.rs`, plus live ordered
stream capture in `src/process.rs`. Final JSON reports now populate the contract,
including fingerprints, phase coverage, failures, and suppression counts.
Live stdout events, ordered worker capture, structured Logger capture, and
stopped-operation coverage remain.

- [x] M01.01 Define versioned `CheckReport`: engine fingerprint, project snapshot,
  check policy, requested/completed/skipped phases, failures, artifact locations.
- [x] M01.02 Define diagnostic severity, optional engine code, message, resource,
  optional line/column, stack frames, process/session identity, timestamp, count.
- [ ] M01.03 Preserve original stdout/stderr artifacts and ordered diagnostic events;
  display consolidation retains occurrence counts.
- [ ] M01.04 Distinguish pass, validation/runtime failure, tooling failure, missing
  completion, and stopped operations; account explicitly for suppressed diagnostics.
- [ ] M01.05 Implement final `--output json` and a live event-stream format; keep
  human progress on stderr and machine stdout free of presentation text.
- [ ] M01.06 Preserve exit codes 0/1/2; combine structured Logger capture with output
  fallback before registration, after teardown, and on unsupported engines.
- [ ] M01.07 Validate equivalent human/JSON meaning for missing methods, zero-exit
  runtime errors, errors on either stream, ordinary warnings, and repeated errors.
- [ ] M01.08 Validate malformed completion, crash, stopped operation, and suppressed
  diagnostics; no success decision relies only on process exit status.

### M02 Process supervisor

[Roadmap section 2](poweruser-roadmap.md#2-one-engine-process-supervisor).

Status: In progress | Owner: Codex | Started: 2026-09-10 | Target: - | Completed: -
Blocker: None recorded. Evidence / commits: Shared live stream capture and deadline
termination in `src/process.rs`; direct check processes migrated (partial M02.01-M02.02).

- [ ] M02.01 Centralize arguments, process identity, output spooling, deadlines,
  cancellation, termination, exit status, and artifact retention.
- [ ] M02.02 Migrate probes, disposable editor imports, resource checks, and smoke
  runs to the shared lifecycle primitives.
- [ ] M02.03 Define supervisor integration for future exports/test adapters;
  actual adapter adoption is verified in M12.
- [ ] M02.04 Investigate and record Windows Job Object assignment before descendant
  creation and kill-on-close; implement and test the selected ownership strategy.
- [ ] M02.05 Investigate and record Unix process groups and group termination;
  implement and test lifecycle behavior on the supported Unix platforms.
- [ ] M02.06 Retain session identity beyond PID lifetime and distinguish cancellation
  from completion; stream failure notifications before process exit.
- [ ] M02.07 Bound disk output, preserve first failure plus log tail, report truncation.
- [ ] M02.08 Validate blocked autoload, stuck importer, and stuck scene deadlines:
  partial logs survive and no owned descendants remain.
- [ ] M02.09 Validate cancellation leaves unrelated editors/games alive and handles
  spawn, output setup, polling, and cleanup failures explicitly.

### M03 Doctor

[Roadmap section 3](poweruser-roadmap.md#3-api-lookup-from-the-selected-engine).

Status: In progress | Owner: Codex | Started: 2026-09-10 | Target: - | Completed: -
Blocker: None recorded. Evidence / commits: None recorded.

- [x] M03.01 Implement `gdkit doctor` with engine selection source, actual executable,
  version, probe-cache health, worker state, cached import problems, relevant warning
  policy, and project gotchas. Capability inventory remains in M03.02-M03.03.
- [ ] M03.02 Probe option/capability support instead of trusting version strings or
  apparent acceptance of unknown CLI arguments.
- [ ] M03.03 Identify unsupported capabilities and direct mismatch guidance to the
  configured engine; return results through the common reporting contract.
- [ ] M03.04 Validate selection precedence, unsupported features, unavailable/stale
  worker state, and engine replacement at the same path.

### M04 Native API lookup

[Roadmap section 3](poweruser-roadmap.md#3-api-lookup-from-the-selected-engine).

Status: In progress | Owner: Codex | Started: 2026-09-10 | Target: - | Completed: -
Blocker: None recorded. Evidence / commits: pending current commit.

- [x] M04.01 Implement class/member queries and search, including
  `api RichTextLabel`, `api RichTextLabel clear`, and `api search tornado`.
- [x] M04.02 Return signatures, inherited origin, properties, signals, enum values,
  engine identity, and available metadata capabilities.
- [x] M04.03 Return useful nearby candidates for a missing native member; keep
  script-defined APIs separate and do not treat native misses as script-call proof.
- [x] M04.04 Key native metadata by engine and project extension configuration,
  including feature overrides; account for reduced release-build metadata.
- [x] M04.05 Validate inherited methods, custom classes, changed GDExtensions,
  same-path engine replacement, and consistency after cache rebuild.
- [ ] M04.06 Validate official/custom engines without gameplay startup; any supplemental
  docs match the selected version and never override engine metadata.
- [ ] M04.07 Measure cold/warm lookup including startup/serialization where applicable;
  record median/p95 against the proposed cached-query target in the benchmark table.

### M05 Named sessions

[Roadmap section 4](poweruser-roadmap.md#4-named-runtime-sessions).

Status: In progress | Owner: Codex | Started: 2026-09-11 | Target: - | Completed: -
Blocker: None recorded. Evidence / commits: `run`, `sessions`, `logs`, `stop`,
and `restart` use immutable generation records, combined logs, and process
creation identity. Renderer and source fingerprint recording remain.

- [ ] M05.01 Define durable session records: project, engine, scene, arguments,
  renderer, run ID, source fingerprint, logs, generation, and lifecycle.
- [x] M05.02 Implement `run`, `sessions`, and `stop`, with logs/status/restart workflows
  and retained identity for later commands.
- [x] M05.03 Make windowed play first-class and headless interactive runs explicit.
- [ ] M05.04 Reject stale session/generation references, including IDs from a prior
  restart; use the supervisor for cancellation, deadlines, and ownership.
- [ ] M05.05 Validate launch, failure, logs, status, stop, restart, multiple concurrent
  sessions, stale-session rejection, and cleanup without affecting unrelated games.

### M06 Runtime probe

[Roadmap section 4](poweruser-roadmap.md#4-named-runtime-sessions).

Status: In progress | Owner: Codex | Started: 2026-09-11 | Target: - | Completed: -
Blocker: None recorded. Evidence / commits: `inspect <session> --net` now takes
fresh generation-scoped, bounded observations through a typed loopback request.
It reports live peer/root configuration, authority, replication nodes, connection
events, and RPC traffic captured from the engine's built-in multiplayer profiler
through a constrained loopback debugger relay.

- [ ] M06.01 Spike read-only `inspect`: live tree, selected properties, autoload state,
  script errors, and source locations.
- [x] M06.02 Include session/generation and collection time or frame/tick in observations;
  invalidate node IDs on restart and distinguish cached state from fresh samples.
- [ ] M06.03 Negotiate LSP/DAP capabilities; evaluate definitions, references, rename
  candidates, and debugger interactions without duplicating language semantics.
- [ ] M06.04 Demonstrate a breakpoint installed before `_ready()` advances and capture
  startup errors; record unsupported startup ordering explicitly.
- [ ] M06.05 Evaluate public `EditorDebuggerPlugin`/`EngineDebugger` probe versus
  versioned native debugger access and document plugin/autoload setup effects.
- [x] M06.06 Define typed requests with IDs, generations, deadlines, and response bounds;
  observation is the default, with mutation a separately identified capability.
- [ ] M06.07 Specify session-matched mutations returning resulting state, runtime-only
  tuning versus authored saves, and editor undo for authored changes.
- [ ] M06.08 Validate freshness, restart, multiple sessions, unavailable capabilities,
  and startup coverage; populate G1 with evidence before choosing the adapter mix.

### M07 Project graph

[Roadmap section 5](poweruser-roadmap.md#5-scene-and-resource-intelligence).

Status: In progress | Owner: Codex | Started: 2026-09-11 | Target: - | Completed: -
Blocker: None recorded. Evidence / commits: The API command indexes named project
script classes, declarations, script inheritance, native base fallback, and source
locations with gdview. `net` adds a typed source-backed multiplayer topology over
RPCs, peer lifecycle, autoloads, multiplayer subtree contexts, authority calls and
assignments, and serialized replication contracts. `net explain` relates calls to
compatible endpoints and stable peer paths. General reference edges remain.

- [ ] M07.01 Build typed nodes for scripts, scenes, external/subresources, UIDs,
  native types, script classes, and autoloads using gdview and engine resolution.
- [ ] M07.02 Record edge origin for preload, scene instance, inheritance, attached
  script, saved signal connection, and serialized resource reference.
- [ ] M07.03 Index literal dynamic loads; mark computed paths unresolved. Resolve UID
  identity to paths and explain binary-resource resolution and unknowns.
- [ ] M07.04 Implement `refs`, `scene explain`, and `scene diff` with source-backed
  owning scene, inherited source, local override, script type, and sharing details.
- [ ] M07.05 Explain material/mesh sharing, local-to-scene instancing, independent
  overrides, and the assignment that introduced each relationship.
- [ ] M07.06 Label unused-resource results as candidates; a graph miss never triggers
  automatic deletion when dynamic/external references remain possible.
- [ ] M07.07 Validate inherited scenes, repeated instances, unique names, subresources,
  UID-only references, binary resources, cycles, case differences, and imported classes.
- [ ] M07.08 Verify every edge includes a source location or engine-resolution
  explanation and a moved resource retains its resolved identity.

### M08 Scenario protocol

[Roadmap section 6](poweruser-roadmap.md#6-reproducible-scenarios-and-multiplayer).

Status: In progress | Owner: Codex | Started: 2026-09-11 | Target: - | Completed: -
Blocker: None recorded. Evidence / commits: Project-owned `SceneTree` scripts now
run through `gdkit check --script` with deadlines, diagnostics, and durable phase
artifacts. Every check uses a fresh temporary project without changing the source
project's imported state. Named scenarios persist startup state, the first
failure, resolved ports, participant sessions, and readiness through cleanup.

- [ ] M08.01 Define opt-in named scenarios using project-owned fixture scenes and
  setup, ready, checkpoint, assertion, finish messages.
- [ ] M08.02 Wait on readiness/signals with bounded assertions and deadlines; preserve
  durable failure artifacts through the shared report/session infrastructure.
- [ ] M08.03 Provide framework integration hooks without requiring GUT/GdUnit4 or
  inventing a replacement assertion framework; complete adapters in M12.
- [ ] M08.04 Distinguish action-state updates from injected input events, press/release
  timing, process frames, physics ticks, and separately bound actions.
- [ ] M08.05 Record seeds, environment, filesystem inputs/configuration as applicable,
  and numeric tolerances; describe repeatability without promising universal replay.
- [ ] M08.06 Define exact replay as an optional project capability requiring evidence.
- [ ] M08.07 Validate success, failed assertion, readiness timeout, missing completion,
  crash, cancellation, input semantics, artifact retention, and owned-process cleanup.

### M09 Multiplayer scenarios

[Roadmap section 6](poweruser-roadmap.md#6-reproducible-scenarios-and-multiplayer).

Status: In progress | Owner: Codex | Started: 2026-09-11 | Target: - | Completed: -
Blocker: None recorded. Evidence / commits: pending current commit. Named scenarios
now orchestrate explicit dedicated ENet or Steam P2P participants with staged late
join, named port allocation, readiness checkpoints, isolated logs/user data, and
distinct orderly-disconnect and forced-crash controls.

- [ ] M09.01 Orchestrate a server and two clients, await joins, change shared state,
  join a late client, compare declared state, and disconnect a peer.
- [x] M09.02 Assign distinct participant role, log stream, run identity, arguments,
  and data directory so participants cannot overwrite each other's settings.
- [x] M09.03 Allocate endpoints deliberately and capture network configuration;
  distinguish dedicated-server features/exports from headless launch.
- [ ] M09.04 Let project adapters define synchronized state and checkpoint tolerances;
  identify the first divergent participant with relevant checkpoint values.
- [ ] M09.05 Validate late join, disconnect during setup, occupied port, crashed client,
  failed assertion, readiness timeout, and clean shutdown of all owned participants.
- [ ] M09.06 Recreate and diagnose a real multiplayer failure; record timings and repro
  artifacts for G2 before expanding the command surface.

### M10 Performance and visuals

[Roadmap section 7](poweruser-roadmap.md#7-performance-and-visual-evidence).

Status: Planned | Owner: Unassigned | Started: - | Target: - | Completed: -
Blocker: None recorded. Evidence / commits: None recorded.

- [ ] M10.01 Define named captures with warmup/measurement intervals, renderer,
  resolution, engine/build identity, and scenario checkpoint.
- [ ] M10.02 Capture standard/custom monitors with availability and update cadence;
  do not count cached values as fresh high-frequency samples or unavailable zeroes.
- [ ] M10.03 Report frame-time distributions/spikes and memory/resource counts at
  meaningful checkpoints, including repeated transitions or rounds.
- [ ] M10.04 Define project budgets that allow intentional persistence and calibrate
  performance limits using repeated local runs before enforcing CI thresholds.
- [ ] M10.05 Capture actual rendered frames with explicit camera/state setup, warmup,
  renderer/resolution, ignored regions, and nondeterministic-pixel tolerances.
- [ ] M10.06 Retain original images and diffs; headless startup results do not establish
  visual correctness of shadows, mirrors, materials, or GPU-specific shaders.
- [ ] M10.07 Validate comparison metadata/checkpoints and report renderer mismatch as
  an unmet precondition, not a visual regression.
- [ ] M10.08 Demonstrate a frame-time spike investigation and a rendered visual fix
  at the same camera checkpoint; attach before/after evidence.

### M11 Incremental watch

[Roadmap section 8](poweruser-roadmap.md#8-watch-mode-refactoring-and-exports).

Status: Planned | Owner: Unassigned | Started: - | Target: - | Completed: -
Blocker: None recorded. Evidence / commits: None recorded.

- [ ] M11.01 Debounce edits and publish diagnostic deltas through the event stream.
- [ ] M11.02 Invalidate by content and dependency, not only timestamp; preserve a
  fresh full-check reference path.
- [ ] M11.03 Cover engine binaries, warning policy, global classes, autoloads,
  extensions, import settings, and transitive dependencies in invalidation rules.
- [ ] M11.04 Keep persistent editor state distinct from a cached passing result;
  use fresh authoritative checks until incremental correctness is established.
- [ ] M11.05 Validate incremental/full equivalence across each invalidation category,
  rapid edits, and incomplete dependency knowledge.
- [ ] M11.06 Measure cold/warm median and p95, memory, and cancellation time on small,
  large, and import-heavy fixtures; evaluate the proposed warm-feedback target.

### M12 Adapters and refactoring

[Roadmap sections 6](poweruser-roadmap.md#6-reproducible-scenarios-and-multiplayer)
and [8](poweruser-roadmap.md#8-watch-mode-refactoring-and-exports).

Status: Planned | Owner: Unassigned | Started: - | Target: - | Completed: -
Blocker: None recorded. Evidence / commits: None recorded.

Frameworks and builds:

- [ ] M12.01 Adapt GUT/GdUnit4 results to common reports and supervision; record
  supported framework versions and validate pass, failure, timeout, and cleanup.
- [ ] M12.02 Export with the configured engine/preset, capture diagnostics, record
  template/SDK prerequisites, and verify expected output artifacts.
- [ ] M12.03 Keep packaging separate from publishing; report missing prerequisites
  and failed exports without implying a successful deliverable.
- [ ] M12.04 Add explicit .NET build coverage for C# and native build/test integration
  for GDExtension; a GDScript pass does not imply either build passed.

Transactional edits:

- [ ] M12.05 Start with constrained unambiguous literal scene-reference renames;
  generate a reviewable patch with expected source hashes and unresolved references.
- [ ] M12.06 Detect stale source and conflicting unsaved editor buffers before apply;
  use editor undo/redo for authored live-editor changes.
- [ ] M12.07 Preserve formatting and unrelated edits in file-only mode; provide
  transaction rollback and verify no partial edit remains after failure.
- [ ] M12.08 Validate preview/apply, dynamic-reference warnings, source conflicts,
  unsaved buffers, rollback, and native undo/redo.

## Cross-cutting architecture checks

Track these alongside the owning milestones. They are review constraints, not a
thirteenth milestone or a commitment to build every future transport now.

- [ ] A01 CLI and any future MCP/editor transports call shared core commands with
  typed reports/events and no independent validation logic. Evidence: -
- [ ] A02 Godot owns language semantics; gdview owns lossless syntax/text scenes;
  framework adapters preserve framework assertions. Evidence: -
- [ ] A03 Project state preserves provenance and unknowns; native metadata, graph,
  and runtime state use distinct cache identities/lifetimes. Evidence: -
- [ ] A04 Persistent state uses one project service where useful, with idle shutdown
  and explicit status/stop; supervision owns only launched/registered sessions. Evidence: -
- [ ] A05 Bridge requests carry IDs, generations, deadlines, and size bounds;
  mutations are explicit and unrestricted eval is not the primary interface. Evidence: -

## Decision gates

### G1 Runtime integration

Status: Pending | Owner: Unassigned | Decision date: - | Evidence: -

Use `Verified`, `Partial`, `Unsupported`, or `Not tested` in each cell, with an
artifact link and engine version for any tested result. Suggested architecture is
not proof of adapter support.

| Capability | Public probe | DAP | Versioned native adapter |
| --- | --- | --- | --- |
| Startup errors | Not tested | Not tested | Not tested |
| Live tree | Not tested | Not tested | Not tested |
| Properties | Not tested | Not tested | Not tested |
| Breakpoints before startup advances | Not tested | Not tested | Not tested |
| Frame locals | Not tested | Not tested | Not tested |
| Input | Not tested | Not tested | Not tested |
| Screenshots | Not tested | Not tested | Not tested |
| Multiple sessions | Not tested | Not tested | Not tested |

- [ ] G1.01 Complete the feature matrix on selected official/custom engines, including
  setup requirements, startup ordering, and unsupported behavior.
- [ ] G1.02 Compare maintenance cost and engine-version sensitivity; record the chosen
  adapter for each supported capability and why.
- [ ] G1.03 Record deferred capabilities and revise affected milestone scope explicitly.

Decision: Pending. Follow-up items: None recorded.

### G2 Reproduction usefulness

Status: Pending | Owner: Unassigned | Decision date: - | Evidence: -

- [ ] G2.01 Select a real multiplayer failure and record baseline reproduction and
  diagnosis time, project snapshot, engine, and environment.
- [ ] G2.02 Reproduce through M09 and preserve a rerunnable scenario, participant logs,
  first divergence, and checkpoint values.
- [ ] G2.03 Rerun after a fix and record reproduction/diagnosis time and limitations.
- [ ] G2.04 Decide whether the workflow is useful enough to expand; if not, record
  corrective work and hold additional surface commands until the gate is resolved.

Decision: Pending. Follow-up items: None recorded.

## Benchmark and validation record

Targets are roadmap proposals, not guarantees. Record fixture/project revision,
engine fingerprint, OS/hardware, build/renderer, sample count, exact command,
measurement boundary, and artifact path with each result. Keep cold/warm runs
separate and distinguish engine-only timing from end-to-end CLI latency.

| Measurement | Proposed target | Cold median / p95 | Warm median / p95 | Evidence |
| --- | --- | --- | --- | --- |
| Cached local metadata query | Under 100 ms | Not measured | Not measured | - |
| Small-project feedback | Under 1 s warm | Not measured | Not measured | - |
| Large-project feedback | Establish separate baseline | Not measured | Not measured | - |
| Import-heavy change | Establish separate baseline | Not measured | Not measured | - |
| Cancellation time | Within configured deadline | Not measured | Not measured | - |
| Engine memory | Establish per-workload baseline | Not measured | Not measured | - |

Add one row per validation run. Do not infer cross-platform support from Windows
results or rendered correctness from headless checks.

| Date | Item IDs | Commit / fixture | Engine / OS / renderer | Command or procedure | Result / artifacts |
| --- | --- | --- | --- | --- | --- |
| - | - | - | - | - | No new validation runs recorded |

## End-to-end success checks

These validate the user workflow after the relevant milestones are delivered.

- [ ] E01 Catch a missing method before play with source, phase, and machine-readable
  failure evidence. Milestones: M01, M03, M04. Evidence: -
- [ ] E02 Locate an inherited property override and explain ownership/sharing.
  Milestone: M07. Evidence: -
- [ ] E03 Recreate a late-join state bug, identify first divergence, and rerun after
  the fix. Milestones: M08, M09; gate G2. Evidence: -
- [ ] E04 Find the source of a frame-time spike using scenario-correlated evidence.
  Milestone: M10. Evidence: -
- [ ] E05 Prove a visual fix at the same camera checkpoint with original/diff images
  and matching capture metadata. Milestone: M10. Evidence: -
- [ ] E06 Stop a failed run without owned orphan processes or interference with an
  unrelated editor/game. Milestones: M02, M05, M09. Evidence: -

## Blockers and decisions

Use stable IDs and link affected checklist items. A decision record should include
the selected option, reason, evidence, and resulting scope or dependency changes.

| ID | Date | Kind | Affected items | Blocker or decision / rationale | Owner | Resolution / next action |
| --- | --- | --- | --- | --- | --- | --- |
| - | - | - | - | None recorded | - | - |

## Progress log

| Date | Items | Change | Evidence / next step |
| --- | --- | --- | --- |
| 2026-09-11 | M08.01-M08.02 partial, M09.01-M09.03 partial, M09.05 partial | Added named multiplayer scenarios with explicit `dedicated_enet`/`steam_p2p` transport, staged server/client/late-client startup, bounded readiness checkpoints, server-bound dynamic ports, durable startup/failure state, per-participant logs and user-data roots, plus separate orderly disconnect and forced crash controls | `src/scenario.rs`, `src/session.rs`, `src/runtime_probe.gd`, `tests/scenarios.rs`; validated a real ENet server binding port 0, reporting its endpoint by checkpoint, two clients, late join, persisted readiness failure after cleanup, isolation, disconnect, crash, and cleanup on custom Godot 4.7.3; next add scenario actions beyond process control |
| 2026-09-11 | M09.02 partial | Added bounded recursive checkpoint comparison across exact live session generations with capture skew, JSON Pointer differences, and automation-friendly exit status | `src/runtime_probe.rs`, `tests/sessions.rs`; validated a declared lobby revision divergence between two custom Godot 4.7.3 sessions; next persist scenario-correlated captures |
| 2026-09-11 | M09.01 partial | Added opt-in project checkpoint adapters and bounded live collection with session, adapter, wall-clock, process-tick, physics-tick, and duration identity | `src/engine.rs`, `src/runtime_probe.rs`, `src/runtime_probe.gd`, `tests/sessions.rs`; validated named checkpoint dictionaries and combined network collection on custom Godot 4.7.3; next compare participants and persist captures |
| 2026-09-11 | M06.02, M06.06 | Added `gdkit inspect <session> --net` with fresh generation-scoped multiplayer roots, peers, authority, replication state, connection events, and built-in RPC profiler traffic over bounded typed loopback requests | `src/runtime_probe.rs`, `src/runtime_probe.gd`, `src/debugger_bridge.gd`, `tests/sessions.rs`; validated startup, human/JSON observations, connection and RPC events, stale generations, restart, concurrent sessions, and cleanup on custom Godot 4.7.3 |
| 2026-09-11 | M07.01 partial | Added `gdkit net explain` with source-backed RPC call-to-endpoint contracts, multiplayer subtree and stable-path requirements, recipients and sender identity, plus spawner/synchronizer properties and authority relationships | `src/net.rs`, `src/net_report.rs`, `tests/net.rs`; validate against Pill Poppers `_request_start_match` and custom Godot 4.7.3 |
| 2026-09-11 | M07.01 partial | Added `gdkit net` with configured-engine RPC metadata and gdview-backed RPC calls, peer construction/assignment, lifecycle, authority, autoload, and text-scene replication topology in human and JSON reports | `src/net.rs`, `src/net.gd`, `src/net_report.rs`, `tests/net.rs`; validated against Pill Poppers on custom Godot 4.7.3; next add arbitrary replication properties through the shared project graph |
| 2026-09-11 | M05.01-M05.05 partial | Added durable named run sessions with exact generation selectors, combined logs, live status, identity-checked stop, and restart; windowed runs are default and headless is explicit | `src/session.rs`, `tests/sessions.rs`; validated launch, concurrent sessions, duplicate rejection, logs, status, stop, stale stop, restart, and history on custom Godot 4.7.3; next add source/renderer identity and shared supervisor integration |
| 2026-09-11 | M07.01 partial | Added source-backed named GDScript classes and members to API lookup and search, including project inheritance and native base fallback | `src/api.rs`, `tests/api.rs`; validated with Pill Poppers domain classes on custom Godot 4.7.3; next build reference edges and resource nodes |
| 2026-09-11 | M01.07 partial | Made every check run a complete editor import and GDScript scan in a disposable cacheless project copy, preserving source caches and collecting import diagnostics | `src/check.rs`, `tests/check.rs`; next validate against configured Godot engines |
| 2026-09-11 | M08.02-M08.03 partial | Added bounded project-owned script checks and isolated fresh-project execution through the common report and artifact pipeline | `src/check.rs`, `src/report.rs`, `tests/check.rs`; validated against Pill Poppers infrastructure contract on custom Godot 4.7.3; next define named scenario protocol |
| 2026-09-10 | M01.03-M01.07 partial | Added final JSON check reports and persisted report artifacts with populated diagnostics, phase coverage, fingerprints, suppression counts, and exit-code outcomes | `src/check.rs`, `src/report.rs`, `tests/check.rs`; unit and local Godot validation; next migrate worker capture and add live stdout events |
| 2026-09-10 | M01.03, M02.01-M02.02 partial | Added shared live stdout/stderr capture with observed ordering, timestamps, process identity, exact stream reconstruction, and deadlines; migrated fresh import, resource loading, and smoke processes | `src/process.rs`, `src/check.rs`; populate reports from captured events and migrate probe/worker lifecycle paths |
| 2026-09-10 | M01.03 partial | Retained byte-exact stdout/stderr for each executed engine phase and made human diagnostic consolidation report occurrence counts | `src/check.rs`; replace post-exit stream grouping with ordered shared capture before completing M01.03 |
| 2026-09-10 | M01.01-M01.02 | Added the versioned report and diagnostic data contract with stable JSON names and round-trip coverage | `src/report.rs`; next integrate ordered capture and raw artifacts |
| 2026-09-10 | M03.01 | Added `gdkit doctor` for resolved engine identity and source, probe-cache health, worker freshness, warning policy, and detected project gotchas | `src/doctor.rs`, `src/engine.rs`, `src/import_worker.rs`, `tests/doctor.rs`; next add capability inventory and common report output |
| 2026-09-10 | Tracker | Established 12 planned milestones, acceptance checks, and two decision gates from the roadmap; no new delivery claimed | Begin M01 and record implementation evidence |
