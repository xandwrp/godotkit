# Net research follow-up

Pre-implementation evidence for [NET_SCOPE.md](NET_SCOPE.md). Statements about
current code below describe the research baseline; the first implementation slice
subsequently corrected literal RPC argument handling and retained annotation errors.

## Method

Inspected the current declaration index, syntax AST, scene parser, and private
xref resolution machinery. Ran isolated GDScript probes under `/tmp` using
`/usr/bin/godot`, version `4.7.2.stable.arch_linux.ed1daf0bf`. Each invocation was
headless, bounded by `timeout 20s`, with its working directory and `--path` set
to the scratch directory, not this repository. No project scripts were executed.

These results establish behavior of that installed version, not every Godot 4.x.
Negative annotation experiments intentionally produced script parse errors; the
probe continued to record each result. Negative channels were compiled, not
exercised in network traffic.

## 1. RPC annotation parsing has existing correctness gaps

Engine experiment: for each annotation, create a fresh `GDScript`, set source to
`extends Node\n<annotation>\nfunc endpoint(): pass\n`, call `reload()`, and inspect
`get_rpc_config()` only on successful compilation.

| Annotation | Godot 4.7.2 result |
| --- | --- |
| `@rpc` / `@rpc()` | Accepted; config contains `rpc_mode: 2` (authority) |
| `@rpc("any_peer", "call_local", "reliable", 3)` | Accepted; mode 1, local true, transfer 2, channel 3 |
| `@rpc("authority", "any_peer")` | Rejected: permission specified more than once |
| `@rpc("reliable", "unreliable")` | Rejected: transfer mode specified more than once |
| `@rpc("call_local", "call_remote")` | Rejected: locality specified more than once |
| `@rpc(2)` | Rejected: argument 1 must be String |
| `@rpc(2, "reliable")` | Rejected: argument 1 must be String |
| `@rpc("reliable", 2)` | Rejected: argument 2 must be String |
| `@rpc("any_peer", "call_remote", "reliable", -1)` | Accepted; config retains channel -1 |

Current `RpcConfig::from_arguments`:

- Allows repeated categories and documents “later wins, as in Godot.” That claim
  is false for the tested engine.
- Treats an integer in any position as a channel.
- Uses `u32` for channel, rejecting a negative channel the script compiler retains.
- Strips quotes before interpretation, so quoted numeric strings can be mistaken
  for an integer channel. This is a code-inspection concern, not yet an engine
  differential test.
- Declaration indexing discards config parse errors via `.ok()`; malformed
  annotations disappear from the typed endpoint surface unless net inspects the
  original annotation records.

`rpc_config_rejects_unknown_arguments` currently expects the three arguments
`any_peer, unreliable_ordered, 1` to succeed. Existing passing tests therefore do
not establish annotation agreement with Godot.

**Recommendation:** correct annotation validation in one coordinated declaration
change with differential fixtures, rather than introduce a second net-specific
RPC config parser. Preserve signed source config channels or explicitly distinguish
source configuration from validated transport constraints. Do not claim a channel
that compiles is necessarily usable by a particular peer transport. Constant
expressions and non-literal annotation arguments need a separate policy: unresolved
is preferable to speculative evaluation.

### Additional argument probes before the first implementation slice

Godot 4.7.2 also accepted category strings in any order, StringName literals,
raw strings and Unicode escapes. Quoted numeric channels were rejected. Decimal
floats were accepted and converted to integers (`2.5` became `2`); hexadecimal,
underscored integers, arithmetic and named constants were accepted when constant.
More than four arguments and identical repeated categories were rejected.

The first source-only implementation deliberately supports literal strings and
signed decimal integer channels. Other expressions retain a `cannot resolve`
annotation error rather than being labelled invalid GDScript or assigned defaults.
Tests pin both the supported literal results and this analysis boundary.

## 2. Replication defaults and compatibility fields

Probe A: create a `SceneReplicationConfig`, call
`add_property(NodePath(".:position"))`, inspect public getters and serialized
`properties/*` fields. Probe B: create fresh configs using
`set("properties/0/path", NodePath(".:position"))`, then apply the listed fields.
Probe C: write actual `.tres` files with those property lines in that order and
load them with Godot. Resource loads confirmed the key order-sensitive results.

| Authored property sequence after path | Resulting mode | Spawn |
| --- | --- | --- |
| none | 1 / Always | true |
| `sync = false` | 0 / Never | true |
| `sync = true` | 1 / Always | true |
| `watch = true` | 2 / OnChange | true |
| `watch = false` | 1 / Always | true |
| `sync = false`, then `watch = true` | 2 / OnChange | true |
| `watch = true`, then `sync = false` | 2 / OnChange | true |
| `sync = true`, then `watch = true` | 2 / OnChange | true |
| `watch = true`, then `sync = true` | 1 / Always | true |
| `replication_mode = 2`, then `sync = true` | 1 / Always | true |
| `sync = true`, then `replication_mode = 2` | 2 / OnChange | true |
| `spawn = false`, then `replication_mode = 2` | 2 / OnChange | false |

Consequences:

- `spawn` and replication mode are independent facts. `watch` is a legacy input
  to mode, not an independent modern synchronization dimension.
- “sync=false always means Never” is too broad: legacy watch can still request
  OnChange. The acceptance test must specify the rest of the config.
- Always is the tested default for an added property; the older plan's concern
  about blindly copying that default is now resolved for this engine.
- There is no universal static “modern field wins” or “watch wins” rule for mixed
  fields. Godot applies setters in serialized order.

### Shared parser blocker: order is currently lost

`scene::Properties` is a `BTreeMap<String, Value>`; `scene/text.rs` inserts into
that map while reading. Two order-sensitive files can therefore produce identical
`SceneFile` data despite different engine outcomes. Duplicate property assignment
history is lost too. Pure net analysis cannot reconstruct what the parser discarded.

Options, in preference order for a bounded first release:

1. Handle unambiguous modern/legacy configs and emit an explicit unknown whenever
   the retained fields permit different results depending on order. Do not claim
   an exact mode for those cases. This avoids a shared parser change.
2. If exact legacy/mixed-format parity is required, add ordered property entries
   with provenance alongside lookup maps in the shared parser. Coordinate this
   before changing scene structures; no second line parser in net.

Another model gap: `SceneFile` preserves the `.tres` header's kind, but not its
`type="SceneReplicationConfig"`. External-resource declarations carry a type
hint, yet the loaded resource's actual header type cannot be checked. Recommend
preserving header type in a small shared parser change if external config type
verification is required, or explicitly treating the declaration as unverified.

## 3. Node defaults and API shapes verified

Fresh engine objects returned:

| Property | Default |
| --- | --- |
| `MultiplayerSynchronizer.root_path` | `NodePath("..")` |
| `MultiplayerSynchronizer.replication_interval` | 0.0 |
| `MultiplayerSynchronizer.delta_interval` | 0.0 |
| `MultiplayerSynchronizer.public_visibility` | true |
| `MultiplayerSpawner.spawn_path` | empty NodePath |
| `MultiplayerSpawner.spawn_limit` | 0 |

Interval and visibility fields are absent from the proposed first report. Thus
“what is synchronized and when” must mean replication mode, not full timing or
visibility analysis, unless those fields are deliberately added to scope.

ClassDB reflection confirmed:

- Node `rpc(method: StringName, ...)`, `rpc_id(peer_id, method: StringName, ...)`.
- MultiplayerAPI `rpc(peer, object, method: StringName, arguments = [])`.
- SceneTree `set_multiplayer(multiplayer, root_path = NodePath(""))`.
- Node `set_multiplayer_authority(id, recursive = true)`.

Do not represent an omitted `set_multiplayer` path as an authored `/root` literal.
The engine API default is empty; interpreting its effective scope should be kept
separate from source evidence. Likewise, recognize SceneTree receivers such as
`get_tree().set_multiplayer(...)`, not arbitrary `.set_multiplayer` names.

A generated Node script compiled all of these forms successfully:

```gdscript
rpc("fire")
rpc_id(1, &"fire")
self.rpc("fire")
fire.rpc()
fire.rpc_id(1)
Callable(self, &"fire").rpc()
multiplayer.rpc(1, self, &"fire")
```

The syntax layer already has `FieldExpr`, `CallExpr`, `NameRef`, `GetNodeExpr`,
`UniqueNodeExpr`, and separate StringName tokens. It can support structural
recognition. The missing piece is net's observation model, not a new parser.

## 4. Existing xref resolver: useful but not drop-in

`xref/analysis.rs` already supports:

- Script inheritance chains with cycle/depth guards.
- UID-aware external resource targets.
- Inherited scene script attachments and explicit `script = null` overrides.
- Lookup through nested scene instances and inherited bases.
- Relative paths, initial `%Unique` lookup and bounded `..` traversal.

However:

- These helpers are private and coupled to `Analysis`/`ProjectGraph`.
- `resolve_from` yields Found/Missing/Unknown, not the resolved receiver identity
  and evidence that net needs.
- `owners` uses `effective_scripts`, which walks direct/inherited attachments;
  it does not enumerate every script inside nested scene instances.
- Absolute paths deliberately yield Unknown. Net needs separate autoload and
  runtime-root reasoning, not simply lifting that restriction.
- `chain` selects `bases[0]` for duplicate class names; that is not an acceptable
  unqualified certainty claim for net candidates.
- Unique-name lookup and chained `%` paths have documented limits.

**Recommendation:** do not expose `Analysis` wholesale or make full scene expansion
an accidental prerequisite. Keep first-release direct anchors conservative. If
shared resolution is extracted later, use a dedicated read-only resolver returning
resolved targets, provenance, ambiguity and incomplete coverage, with xref and net
as consumers. That is a separate tested change, not a quick `pub` modification.

## 5. Verified baseline and revised coordination

Executed:

```sh
cargo test -p gdview --test declarations --test scene --test syntax --test xref
```

All 37 tests passed. This verifies the current baseline, not net functionality;
net's acceptance tests remain scaffolds. No production source was changed.

The new concrete shared touchpoints are:

1. `declarations.rs`, its index/error handling, and declaration tests for RPC
   annotation correctness. This is a net prerequisite worth owning explicitly.
2. Scene property order and resource header type, only if exact external/legacy
   replication analysis is chosen. Coordinate with resource planning before any
   model change. A bounded unknown fallback can avoid blocking independent work.

Neither discovery creates a dependency on resource creation, schema harnesses,
`VariantJson`, or the engine runner. Engine probes here are research evidence,
not a proposal to invoke Godot from `gdkit net`.
