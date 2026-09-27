# Net contract fixtures

`actor.gd`, `replication.tscn`, and `legacy.tscn` are copied into a fresh project
by both `gdview/tests/net_schema.rs` and `gdproject/tests/net_real.rs`.

- `report.json` pins the entire offline schema-3 report (including local candidate
  indexes and conservative coverage wording).
- The real-engine test uses the runner with a 30-second deadline to compare RPC
  settings, replication spawn/mode, root_path and spawn_path against Godot.
  It does not invoke RPC traffic or run the scene in a game tree.
- Verified with Godot 4.7.2 on Linux. Normal net tests require no engine.

Run:

```sh
cargo test -p gdview --test net_schema
GDKIT_TEST_GODOT=/usr/bin/godot cargo test -p gdproject --test net_real -- --ignored
```

To review a schema change, copy the three source fixtures into a temporary
project with `project.godot` containing `config_version=5`, run
`gdkit net --project <temporary-project> --output json`, and review the complete
diff against `report.json`. Never regenerate against a directory containing the
engine probe script; that changes the source inventory.
