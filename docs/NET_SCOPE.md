# Net refactor scope

Scope based on the local refactor and `legacy/src/net.rs`, not a commitment to
reproduce every legacy feature.

## Delivery progress

First implementation slice: RPC declaration prerequisites. Literal annotation
parsing now rejects repeated categories and misplaced channels, preserves signed
channels, and retains `rpc_error` for invalid or unresolved annotations (including
inner classes). String decoding uses the existing GDScript AST. Unsupported
constant expressions remain explicitly unresolved; this is not a constant evaluator.
The net analyzer and CLI remain stubs. `cargo test -p gdview` and
`cargo test --workspace` passed with this slice (existing ignored tests remain).
No resource implementation files were changed by the net work.

Follow-up: [NET_RESEARCH.md](NET_RESEARCH.md) records isolated Godot 4.7.2
experiments, existing RPC annotation correctness gaps, order-sensitive replication
loading, and the limits of reusing xref's resolver.

## Recommended delivery

Implement `gdkit net` as a read-only, entirely offline multiplayer map, plus
`--explain`. Report source observations and explicitly bounded candidates, not
proof of runtime connectivity, RPC compatibility, or security. No engine lookup,
workspace lock, import, cache writes, or harness invocation.

This is independent of `resource schema/create`. Runtime network observations
(`run --net`, `gdproject::probe`) are a separate workstream.

## Current state

- `crates/gdview/src/net.rs`: report types exist; `analyze` and `explain` are TODOs.
- `crates/gdview/tests/net.rs`: ten named acceptance tests, all ignored scaffolds.
- `src/commands/net.rs`: TODO; `src/status.rs` correctly leaves net a stub.
- Declaration indexing, typed GDScript syntax, text scene/resource parsing,
  project file discovery, UID maps, and autoload resolution are implemented.
- `scene::expand` is still a scaffold. Net must not silently depend on it.
- `xref::ProjectGraph::load` already inventories files and parses `.tscn`/`.tres`,
  retaining parse failures. Its loading policy is a useful reuse candidate;
  xref's private resolution machinery is not yet a shared public API.
- Legacy net combined an engine query with source scans. Reuse fixtures and
  lessons, not its orchestration, text scene scanner, or suffix-only inference.

## Contract decisions required before implementation

### 1. Analysis needs bodies, not just declarations

`IndexedScript` retains declarations and resource/node-path uses, but neither
source text nor call expressions. The current `NetInput` cannot discover RPC
calls, authority calls, or `set_multiplayer` observations.

Recommended: add a net-owned script observation pass over the existing typed AST
and supply those observations alongside declarations to pure `analyze`. Include
class scope, source location, receiver and argument evidence. A net-owned project
loader can read sources and collect these inputs; keep filesystem access outside
`analyze`. Initially parsing twice is preferable to widening the shared
`IndexedScript` schema for net alone. Measure before optimizing.

Do not use regex or split callee text on dots to distinguish call forms.

### 2. Preserve resolution and coverage evidence

Extend the input to carry resolved autoloads/UID information, loaded text
resources, and load/parse failures. The present raw `Autoloads` input cannot
resolve UID targets. An unresolved target cannot honestly fit a required
`NetAutoload.path: ResPath`; use an optional resolved path and preserve the target.
Keep initialization order and singleton status.

Add explicit scene/script anchors and observed multiplayer contexts, or equivalent
report data. Current endpoint/call records alone cannot implement meaningful
scene/node explanations or the named subtree-root acceptance test. Contexts are
observations of `set_multiplayer`, not a reconstructed runtime state.

Keep scene-relative paths separate from hypothetical `/root/...` runtime paths.
A scene root name does not prove where that scene is instantiated at runtime.
Match known subtree roots on path-segment boundaries, not string prefixes.

### 3. Replication semantics

The scaffold's `SyncedProperty { path, mode, watch }` has no documented meaning
for `watch` and omits Godot's independent `spawn` flag. Define the contract from
Godot serialization before implementing: recommend `path`, `spawn`, and explicit
replication mode, preserving uncertainty for invalid/unrecognized values.

Read both modern `properties/N/replication_mode` and old `sync`/`watch` fields.
Old-format `sync = false` without an enabled watch must not default back to
Always. Follow-up engine probes confirmed default spawn=true and mode=Always,
but also proved compatibility setters are order-sensitive: watch=true followed
by sync=true differs from the reverse. `Properties` currently loses source order.
Emit unknowns for order-sensitive combinations unless a coordinated parser change
preserves ordered assignments. Malformed values must not become plausible defaults.
See NET_RESEARCH.md for the observed conversion table.

### 4. JSON identity and explanation references

Both legacy and refactor currently say schema version 2 despite incompatible
shapes. Recommend version 3 for the implemented refactor report, with a documented
shape and JSON regression fixture. Decide serialized location spelling too:
net currently uses `path`, while current CLI conventions use `resource` + `line`.

`RpcCall.candidates` indexes the full report's endpoints. Filtering endpoints in
`Explanation` would invalidate those indexes. Either remap into the explanation's
endpoint array or adopt stable endpoint IDs. Pin referential integrity in tests.

### 5. CLI policy

- No findings or uncertain findings: exit 0; this command is not a lint verdict.
- Proposed `--explain` miss: valid empty explanation with a note, exit 1.
- Invalid project/setup or fatal I/O: exit 2 via normal error handling.
- Recoverable malformed source/scene: retain other observations and emit unknowns.
- Default operation is offline. `NetArgs` has no `--offline`, despite the example
  in `docs/AGENT_USE.md`; remove that example rather than add a redundant flag.

## First-release functionality

### RPC endpoints and calls

- Top-level and recursively nested inner-class `@rpc` functions; use declaration
  configs and Godot defaults after correcting the existing annotation parser's
  duplicate-category/channel-position behavior (see NET_RESEARCH.md).
  An annotation present but rejected by
  `RpcConfig::from_arguments` must yield an unknown, not disappear silently
  (the declaration index currently stores `.ok()`).
- Distinguish Node `rpc("method", ...)` / `rpc_id(peer, "method", ...)`, explicit
  `self` and node receivers, method-Callable `method.rpc(...)` /
  `method.rpc_id(...)`, literal `Callable(receiver, "method")` forms, and
  `multiplayer.rpc(peer, object, method, args)`.
- Preserve dynamic method/peer expressions without pretending they are literals.
  Support StringName literals as well as strings; `ast::string_literal` alone
  intentionally does not decode StringNames.
- Preserve uncertainty where receiver type determines Node-vs-Callable meaning.
  Comments, strings, and unrelated similarly named methods are not evidence of
  confirmed RPC behavior.
- Resolve self within its class scope; resolve directly attached scene scripts,
  literal node paths and singleton autoloads when evidence permits. Prefer the
  caller's scene over global labels; duplicate names and multiple attachments
  must not become arbitrary first matches.
- Method-name-only matches, if included, must be labelled speculative and have
  an unknown reason. Do not describe them as compatible endpoints.
- Source-declared inheritance can remain incomplete in the first release, but
  inherited/dynamic RPC configuration must be identified as a coverage limit.

### Authority and scene replication

- Recognize Node authority methods and MultiplayerAPI identity/server methods
  with receiver-aware rules. In particular `OS.get_unique_id()` is not a
  multiplayer observation. Unknown receiver types remain uncertain.
- Extract direct `MultiplayerSpawner` and `MultiplayerSynchronizer` nodes using
  `SceneFile`, with scene, node path and source line.
- Spawners: spawn path and spawnable scene list. Synchronizers: root path and
  replication properties in numeric property-index order.
- Resolve local SceneReplicationConfig subresources and supplied external text
  `.tres` configs. Missing, binary, invalid, or runtime-assigned configs produce
  unknowns rather than silently empty replication descriptions.
- Handle omitted authored properties with documented Godot defaults; distinguish
  omission from malformed explicit values.
- Mark relevant script and scene autoloads as networked based on positive source
  evidence. Document that false means no evidence found, not proof of no network.
- Recognize statically visible multiplayer subtree assignments, retaining dynamic
  roots and conflicting assignments as uncertainty.

### Explain and deterministic reporting

- Support method, receiver.method, scene `res://` path, and replication node path.
  A node path repeated in several scenes returns all matches with scene context.
- Return associated endpoints, calls, replication nodes and relevant unknowns;
  avoid unrelated global noise. Explicit notes for misses and unsupported links.
- Stable sorting for endpoints/calls/anchors/replication findings; sort and dedupe
  unknowns. Preserve meaningful order for autoloads and indexed properties.
- Assign candidate indexes only after final endpoint ordering.
- Human output: useful summaries plus locations, candidate/unknown explanations;
  JSON: exactly one document. Keep command orchestration small; a separate human
  renderer is appropriate if needed.

## Explicit exclusions / follow-up

- Engine-derived/inherited runtime RPC configs and dynamic `rpc_config` evaluation.
- Runtime node creation, peer reachability, actual authority values or delivery.
- Full scene instance/inheritance expansion: direct authored nodes first; emit
  coverage unknowns where expansion would change the answer. Do not mark the
  unrelated scene-tree command ready as part of this work.
- C#, binary `.scn`/`.res`, and GDExtension analysis: explicit unsupported coverage.
- Legacy peer construction, peer assignment, lifecycle signal inventory, structured
  authority assignment contracts, and sender-identity tracing. These are not
  represented by the refactor scaffold and should not expand first delivery by
  accident. Generic authority-use observations remain in scope.
- New Variant grammar, resource serialization, resource creation or validation.

If full legacy parity is required, treat the excluded inventories and expansion
as additional milestones, not an unmentioned part of implementing two TODOs.

## Implementation sequence and acceptance gates

1. **Contract + fixtures:** settle the decisions above; update net-owned types.
   Add fixtures that expose receiver ambiguity, old replication format, external
   configs, schema identity, and explanation reference integrity.
2. **Source observations:** endpoint extraction and AST call/authority/context
   pass. Cover all forms, multiline syntax, StringNames, inner classes, invalid
   annotations, parser recovery, and false-positive negatives.
3. **Scenes + linking:** direct anchors, resolved autoloads, replication extraction,
   bounded candidate resolution, and coverage unknowns. Test same labels in
   different scenes, repeated script attachments, unresolved UID/config targets,
   context segment boundaries and ambiguous runtime roots.
4. **Explain + output:** all four selectors, misses, relevant unknowns, stable
   ordering under shuffled input, and valid candidate indexes after filtering.
5. **CLI delivery:** offline loader, renderers, dedicated net CLI tests and then
   mark only `net` Ready in `src/status.rs` in the same change.

Replace all ten ignored net tests with real bodies. Split the combined scaffold
`scene_tree_and_net_need_no_engine` so net's acceptance does not wait for scene-tree.
CLI tests should supply an unusable/recording engine and invalid engine config to
prove no selection/probe occurs; snapshot the project to prove no writes. Cover
empty projects, parse failures, human output, JSON purity and all exit policies.

Run targeted gdview net tests, the gdview suite (parser/declaration/xref regression),
net CLI tests, status/help gates, then the workspace suite. An opt-in real-engine
fixture check is useful for serialization defaults; normal net tests and execution
must never require Godot. Follow-up research ran 37 existing declaration, scene,
syntax and xref tests successfully, plus isolated engine probes; no net acceptance
implementation exists yet.

## Coordination with the resource planning agent

Primary net ownership:

- `crates/gdview/src/net.rs` and new `crates/gdview/src/net/*`
- `crates/gdview/tests/net.rs` and net fixtures
- `src/commands/net.rs` and optional net renderer module
- This document

Resource retains its command/module, creation/schema harnesses, protocol/Variant
codec and their tests. Net reads authored SceneReplicationConfig data through
`scene::Value`; that is not a dependency on `gdproject::resource` or `VariantJson`.

Shared integration touchpoints: `src/status.rs`, `tests/cli.rs`,
`docs/AGENT_USE.md`, `docs/ARCHITECTURE.md`, and possibly README. Coordinate separate
small edits or give one agent the final integration pass; do not rewrite these
files wholesale. Avoid workspace-wide formatting churn while work is concurrent.

Follow-up research identified one definite shared fix: declaration RPC annotation
validation needs corrected category/argument rules and tests. The first net slice
implements that fix in the declaration module and its tests. Exact legacy replication parity would additionally require
ordered scene properties; external config type verification needs the resource
header type retained. Prefer bounded unknowns if avoiding those model changes.

No other planned changes to scene/syntax parsing, UID resolution or xref public
APIs without an explicit need and coordination. Isolate shared parser fixes with
regression tests rather than introducing a second parser in net. There is no
resource implementation dependency and neither planning effort needs to wait
for the other.
