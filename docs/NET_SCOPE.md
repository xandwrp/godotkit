# Net: delivered scope and verification

The offline `gdkit net` refactor is implemented end to end and marked Ready.
The contract is **source observations with explicit unknowns**, not a proof of
runtime multiplayer correctness. `run --net` remains a separate runtime feature.

## Delivered

- AST-based GDScript observations: RPC endpoints (including qualified inner
  classes), Node/Callable/MultiplayerAPI calls, authority uses, and observed
  `get_tree().set_multiplayer` contexts.
- Correct literal RPC annotation category/argument rules, signed channels, and
  retained invalid/unresolved annotation errors. Nonliteral expressions are not
  evaluated. Original expressions and source locations remain available.
- Receiver candidates bounded by script/class scope, direct scene attachments,
  literal relative/unique/absolute autoload paths, @onready node aliases, and
  singleton autoload registration. No global method-name fallback. Local and
  member bindings prevent accidental global resolution.
- Direct authored MultiplayerSpawner/MultiplayerSynchronizer nodes, local and
  external text replication configs, independent spawn flags and replication
  modes, and Godot defaults for omitted authored properties.
- Legacy sync/watch handling. Mixed/order-sensitive setters and invalid values
  remain unknown instead of being assigned plausible defaults.
- UID-resolved autoloads in initialization order; missing/ambiguous UID targets
  and unsupported language/binary/extension files remain visible.
- Deterministic schema version **3**, distinguishing the refactor from legacy's
  incompatible schema 2. Locations serialize as `resource` + `line`.
- `--explain` selectors for method, receiver.method, scene/script and node path.
  Endpoint and context indexes are remapped into the explanation, never left
  pointing into the original report.
- Human and single-document JSON output. No engine/config lookup, workspace,
  cache, import or authored-file writes. Exit 0 for observation reports (including
  partial reports); exit 1 for explanation misses; exit 2 for setup/fatal I/O or
  invalid arguments.

## Architecture / ownership

```
CLI: Project::discover → net::analyze_project → optional explain → render

gdview::net:
  project.rs      read-only inventory, UID/autoload resolution, load errors
  source.rs       AST observations; no I/O or endpoint linking
  analysis.rs     pure linking, anchors, context candidates, sorting
  replication.rs authored scene/resource property interpretation
  explanation.rs filtering and index remapping
  net.rs          versioned public report/input types
```

`NetInput` supplies script observations, parsed text scenes/resources, resolved
autoloads, UID information and initial unknowns. Source bodies are scanned before
pure analysis; there is no hidden source read in `analyze`.

Shared changes are limited to the prior RPC declaration correction and typed AST
accessors (field/callee structure, StringName/NodePath literals, lexical binding
names). The shared scene/property parser and xref APIs were not changed. Net has
no dependency on resource creation, resource schema harnesses, VariantJson, or
engine selection. The resource workstream's files were not included in net's
implementation commit; the real-engine differential test is a new net-owned test.

## Proof / acceptance gates

All ten originally scaffolded net acceptance tests now have implementations and
run offline. Additional tests exercise ambiguity, invalid annotations, parser
recovery, local/member/loop/lambda/pattern shadowing, unique names, UID collisions,
non-singleton autoloads, external configs, incomplete inputs, and schema integrity.

- `crates/gdview/tests/net.rs`: primary acceptance and linking/replication tests.
- `net_edges.rs` and `net_paths.rs`: adversarial source/path/UID cases, ordered
  relative traversal, repeated attachments, spawner associations and coverage limits.
- `net_syntax.rs`: typed AST accessors used by net.
- `net_schema.rs` + `fixtures/net/report.json`: full schema-3 golden report and
  explanation candidate remapping.
- `tests/net_cli.rs`: actual binary; invalid/unusable engine configuration,
  JSON purity, exit codes, discovery, human output, ignore handling, and recursive
  before/after project-content/directory snapshots proving no writes.
- Existing status/help drift tests confirm net is Ready while unrelated stubs
  remain guarded. The former combined scene-tree/net placeholder is now only
  a scene-tree placeholder; net does not wait for or enable scene-tree.
- `crates/gdproject/tests/net_real.rs`: opt-in, bounded runner invocation on a
  temporary project. The same committed actor/modern/legacy scene fixtures are
  compared against actual Godot RPC configs, replication properties and defaults.
  This is differential test evidence, **not** production engine use.

Verification result for this delivery: **33 net-specific offline tests passed**;
the real-engine differential test passed separately. The all-feature workspace
run passed **404 tests**, with 54 opt-in/unrelated scaffold tests ignored and no
net acceptance scaffold remaining. Workspace clippy passed with warnings denied.

Verification commands:

```sh
cargo test -p gdview
cargo test --test net_cli
cargo test --test cli status_table_matches_what_each_command_does
cargo test --test cli help_tags_every_command_that_is_not_ready
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
GDKIT_TEST_GODOT=/usr/bin/godot cargo test -p gdproject --test net_real -- --ignored
```

Real-engine fixture comparison was verified on Linux with
`4.7.2.stable.arch_linux.ed1daf0bf`. This does not establish compatibility with
every Godot version or prove network delivery. Initial experiments and design
tradeoffs are recorded in [NET_RESEARCH.md](NET_RESEARCH.md).

## Explicit limits (not hidden completion claims)

1. No evaluation of runtime node creation, RPC reconfiguration, script swapping,
   actual authority, delivery, transport constraints, connection state or security.
2. No full scene instance/inheritance or inherited script endpoint expansion.
   Direct authored nodes are reported; instance/inherited scene boundaries produce
   unknowns. Ordinary scene roots are **not** invented as `/root/SceneName` paths.
3. No general type inference or dataflow analysis. Ambiguous `.rpc` receivers stay
   uncertain; local names are handled conservatively. @onready aliases are source
   candidates, not a claim that later assignments cannot change them.
4. Only source String/StringName and signed decimal integer RPC annotation
   arguments are interpreted; constants/arithmetic/coercions remain unresolved.
5. The shared scene parser discards property assignment order and resource header
   type. Mixed compatibility setters yield unknown modes. External config types
   rely on authored ext_resource hints, not verified engine resource classes.
6. Replication reports spawn/mode, not timing intervals, visibility rules, or full
   spawn/authority contracts. Observed multiplayer contexts are not assumed active.
7. C#, binary scenes/resources and GDExtension code are not analyzed. Legacy peer
   construction/assignment inventories, lifecycle tracing and sender-identity
   contracts were explicitly excluded from this refactor's first net delivery.

These limits appear in `coverage.limitations` (and relevant `unknowns`), so an
empty result or an empty unknown list is never advertised as complete runtime
knowledge. Follow-up expansion belongs in separate milestones with new fixtures.
