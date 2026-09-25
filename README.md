# gdkit

Godot 4 development utility belt, rebuilt as three crates. The v0.1.0 workspace
contains implemented features alongside `todo!()` scaffolds and ignored acceptance tests.

Landed: `check` end to end, including `--static-only`, `--slice`, `--baseline`,
and `--script`, with JSON and human reports. Engine validation imports a clean
disposable copy, scans scripts, audits the class cache, and loads resources from
a full-file inventory using the union of editor and runtime loader registries.
Source assets are copied; the source `.godot` cache is not seeded into the copy.
API-cache diagnostic enrichment remains deferred.

Exit codes: `0` passed, `1` failed or incomplete (including check-phase timeouts),
`2` tool/startup, engine-probe, or configuration errors. A baseline permits engine
validation of carried static findings but does not erase their failed verdict;
new static findings block engine phases. Project scripts must call `quit` before
their deadline; the startup marker alone is not success.

Runtime behavior is verified on Linux. macOS shares the POSIX process-group
implementation but is not runtime-verified here. Windows has direct-child cleanup;
Job objects are out of scope, and Windows process-tree cleanup is not guaranteed.

| Crate | Job | Depends on |
| --- | --- | --- |
| [`crates/gdview`](crates/gdview) | Read-only project introspection. No engine, no writes. | nothing |
| [`crates/gdproject`](crates/gdproject) | A project bound to its engine: config, probe, process supervision, harness protocol, every engine-backed operation. | gdview |
| [`crates/gdkit`](crates/gdkit) | The CLI. Args, rendering, exit codes. No logic. | both |

The intended command surface is described in [docs/AGENT_USE.md](docs/AGENT_USE.md): `check` (static then engine, `--slice`, `--script`, `--baseline`), `api`,
`refs`, `settings`, `resource`, `scene-tree`, `autoloads`, `net`, `import`,
`run`, `init`, `doctor`. Nothing else is stubbed, on purpose.

Start with [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): the rules each crate
enforces, the call flow for every command, the harness protocol, and the test
matrix. Each module's doc comment lists its acceptance tests by name under
`crates/*/tests/`. Implemented module gates have completed tests; this does not
mean every workspace scaffold is done. Real-engine tests remain opt-in, and
explicitly deferred tests are distinct from completed gates.

```sh
cargo check --workspace --all-targets --all-features
cargo test  --workspace --all-features                 # implemented offline tests; scaffolds ignored
# With GDKIT_TEST_GODOT set to a real engine executable:
cargo test -p gdproject --all-features --test check real_engine_ -- --ignored
```

Engine-facing offline tests use per-executable fake-engine scenario/log sidecars.
The CLI tests automatically build `fake-godot` once with an isolated, offline Cargo
build; no manual helper build or pre-existing sibling binary is required.

`legacy/` holds v0.1 unchanged, outside the workspace, for reference while
porting: the Variant codec in `legacy/src/resource.gd`, the report contract in
`legacy/src/report.rs`, and the gdview syntax frontend pinned in `legacy/Cargo.toml`.

Dual-licensed under MIT or Apache-2.0. See [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).
