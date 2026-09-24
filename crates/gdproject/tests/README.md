# gdproject tests

Two tiers, by file:

- Offline: use `fake-godot` (built by the `test-engine` feature, path in
  `env!("CARGO_BIN_EXE_fake-godot")`) or spawn `sleep`/`cmd`. They run in CI on
  Linux, macOS, and Windows. Every process-lifecycle property is tested here.
- Engine: `#[ignore = "requires GDKIT_TEST_GODOT"]`; prefixed `real_engine_`.
  They prove the harnesses against a real Godot and refresh golden fixtures.

Test names are the acceptance checklist and are listed in each module's doc comment.
