# gdview tests

Every test here runs with fixture files or in-memory strings. No engine, no network,
no writes outside `tempfile` directories. Test names are the acceptance checklist;
each is listed in the doc comment of the module it covers.

`syntax_corpus.rs` is the one exception: it walks `$GODOT_SOURCE/modules/gdscript/tests/scripts`
and is `#[ignore]`d when the variable is unset.
