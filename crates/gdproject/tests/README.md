# gdproject tests

Two tiers, by file:

- Offline: use `fake-godot` (built by the `test-engine` feature, path in
  `env!("CARGO_BIN_EXE_fake-godot")`) or process subjects such as `sleep`/`cmd`.
  Engine-facing tests copy the executable per test and use
  `<executable>.scenario.json` and `<executable>.log` sidecars, avoiding
  process-global environment mutation. The fake has no environment overrides,
  so inherited variables cannot change a test. It enforces the real harnesses'
  invocation contracts (probe workspace files, `--editor` for ImportScan, one
  ScriptBootstrap argument) even when a scenario overrides the payload.
- Engine: `#[ignore = "requires GDKIT_TEST_GODOT"]` (or the `engine.rs`
  opt-in label); prefixed `real_engine_`. These are implemented and exercise
  actual harness behavior, including autoloads, strict methods, timeouts,
  imported assets, and runtime custom loaders. Fixtures start from clean source
  copies, without seeding `.godot` caches.
- Scaffold: `#[ignore = "scaffold"]` bodies are `todo!()`, including
  `real_engine_` names in modules that are not implemented yet. They are an
  acceptance checklist, not engine coverage; `--ignored` runs will panic on them.

Runtime verification is on Linux. macOS shares the POSIX process-group code but
is not runtime-verified here. Windows supports direct-child cleanup; Job objects
are out of scope and process-tree cleanup is not guaranteed. POSIX cleanup covers
children remaining in the process group, not descendants that deliberately escape it.

The `gdkit` CLI tests automatically build `fake-godot` once into an isolated target
directory using offline Cargo with a three-minute deadline, then copy it and its
scenario per test. Both package-only and workspace tests work without a manual
helper build or reliance on a pre-existing sibling executable.

Test names are the acceptance checklist and are listed in each module's doc comment.
Implemented module gates have complete tests, not blanket completion of all
workspace scaffolds. Check's API-cache enrichment test is explicitly deferred;
check does not load or dump an API index.

```sh
cargo test -p gdproject --all-features --test check
# With GDKIT_TEST_GODOT set to a real engine executable:
cargo test -p gdproject --all-features --test check real_engine_ -- --ignored
```
