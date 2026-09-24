// Parses every script under $GODOT_SOURCE/modules/gdscript/tests/scripts and
// asserts is_valid() agrees with whether the engine expects a parse error
// (the sibling `.out` file's first line). Lossless round trip is asserted on all.
#![allow(unused)]

#[test]
#[ignore = "requires GODOT_SOURCE pointing at a Godot checkout"]
fn engine_test_corpus_parses_with_expected_validity_and_round_trips() {
    todo!()
}
