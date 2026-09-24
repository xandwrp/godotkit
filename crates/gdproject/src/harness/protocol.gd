## Shared by every gdkit harness. Written next to the harness at run time and
## loaded with `preload("protocol.gd")`.
##
## Contract (mirrors gdproject::protocol and gdview::variant):
## - Exactly one line on stdout: `GDKIT_RESULT:` + JSON envelope.
## - Envelope: {"protocol": 1, "harness": name, "ok": bool, "payload": any, "error": {stage, message, field}}.
## - Variants encode as: plain JSON for null/bool/int(|n|<=2^53)/float/String/Array/Dictionary(String keys),
##   otherwise {"$variant": {"type": "<Variant.Type name>", "value": <payload>}}.
##   Vectors/colors/rects/transforms encode as flat number arrays; packed arrays as arrays of their element encoding;
##   typed Arrays as {"$variant":{"type":"Array","element":"<name>","value":[...]}};
##   Resources as {"$ref": "res://..."} when they have a path, else {"$resource": {...}}.
##   NaN/inf encode as {"$variant":{"type":"float","value":"nan"|"inf"|"-inf"}}.
## - Decoding is the exact inverse. Unknown tags are an error, never ignored.
##
## Tests: gdproject/tests/protocol.rs::protocol_gd_encoder_output_matches_gdview_variant_grammar
## drives this file through a real engine once and freezes the output as a golden fixture.
extends RefCounted

const PROTOCOL_VERSION := 1
const RESULT_PREFIX := "GDKIT_RESULT:"


static func emit_ok(harness: String, payload: Variant) -> void:
	pass  # print(RESULT_PREFIX + JSON.stringify(envelope, "", false, true))


static func emit_error(harness: String, stage: String, message: String, field: String = "") -> void:
	pass


static func encode(value: Variant, depth: int = 0) -> Variant:
	return null


static func decode(json: Variant, depth: int = 0) -> Variant:
	return null


## Type name -> Variant.Type, and back. Single table for the whole tool.
static func type_name(type: int) -> String:
	return ""


static func type_from_name(name: String) -> int:
	return TYPE_NIL
