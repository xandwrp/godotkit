# gdkit

Godot 4 development utility belt, rebuilt as three crates. This is the v0.2 scaffold:
every public signature, flow, and acceptance test is in place; bodies are `todo!()`.

| Crate | Job | Depends on |
| --- | --- | --- |
| [`crates/gdview`](crates/gdview) | Read-only project introspection. No engine, no writes. | nothing |
| [`crates/gdproject`](crates/gdproject) | A project bound to its engine: config, probe, process supervision, harness protocol, every engine-backed operation. | gdview |
| [`crates/gdkit`](crates/gdkit) | The CLI. Args, rendering, exit codes. No logic. | both |

Start with [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): the rules each crate
enforces, the call flow for every command, the harness protocol, and the test
matrix. Each module's doc comment lists its acceptance tests by name; the same
names are stubbed under `crates/*/tests/` and `#[ignore]`d until implemented.

```sh
cargo check --workspace --all-targets --all-features   # compiles today
cargo test  --workspace --all-features                 # every stub is ignored
cargo test  --workspace --all-features -- --ignored    # the finish line
```

`legacy/` holds v0.1 unchanged, outside the workspace, for reference while
porting: the Variant codec in `legacy/src/resource.gd`, the report contract in
`legacy/src/report.rs`, and the gdview syntax frontend pinned in `legacy/Cargo.toml`.

Dual-licensed under MIT or Apache-2.0. See [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).
