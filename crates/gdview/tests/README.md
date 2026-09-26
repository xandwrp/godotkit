# gdview tests

Every test here runs with fixture files or in-memory strings. No engine, no network,
no writes outside `tempfile` directories. Test names are the acceptance checklist;
each is listed in the doc comment of the module it covers.

`syntax_corpus.rs` is the one exception: it walks `$GODOT_SOURCE/modules/gdscript/tests/scripts`
and is `#[ignore]`d when the variable is unset.

`api.rs` has the other: `real_engine_refresh_api_fixtures` runs `GDKIT_TEST_GODOT`
to rewrite `fixtures/api` (a trimmed `extension_api.json` and three `--doctool`
class files). It is `#[ignore]`d and only needed after an engine upgrade.
