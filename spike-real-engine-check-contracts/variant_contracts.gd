extends SceneTree
const Protocol := preload("../crates/gdproject/src/harness/protocol.gd")


func _initialize() -> void:
	var typed: Array[Vector2] = [Vector2(1, 2)]
	var inline := Resource.new()
	inline.resource_name = "inline"
	var scripted: Resource = load("res://scripted_resource.gd").new()
	var values: Array = [
		null, true, 42, 9007199254740993, -9223372036854775808, 9223372036854775807,
		1.25, NAN, INF, -INF, "line\nquote\"", &"name", NodePath("child:property"),
		Vector2(1, 2), Vector2i(-1, 2), Vector3(1, 2, 3), Vector3i(1, 2, 3),
		Vector4(1, 2, 3, 4), Vector4i(1, 2, 3, 4), Color(0.25, 0.5, 0.75, 1),
		Rect2(1, 2, 3, 4), Rect2i(1, 2, 3, 4), Plane(Vector3.UP, 2),
		Quaternion(0, 0, 0, 1), AABB(Vector3(1, 2, 3), Vector3(4, 5, 6)),
		Basis(Vector3(1, 2, 3), Vector3(4, 5, 6), Vector3(7, 8, 9)),
		Transform2D(Vector2(1, 2), Vector2(3, 4), Vector2(5, 6)),
		Transform3D(Basis.IDENTITY, Vector3(4, 5, 6)), Projection.IDENTITY,
		[1, "two"], typed, {"key": [3]}, {Vector2i(1, 2): &"value"}, {"$ref": "literal"},
		PackedByteArray([0, 255]), PackedInt32Array([-1, 2147483647]),
		PackedInt64Array([-9223372036854775808, 9223372036854775807]),
		PackedFloat32Array([0.25]), PackedFloat64Array([1.25]), PackedStringArray(["a", "b"]),
		PackedVector2Array([Vector2.ONE]), PackedVector3Array([Vector3.ONE]),
		PackedVector4Array([Vector4.ONE]), PackedColorArray([Color.RED]), inline,
		load("res://probe.tres"), scripted,
	]
	var results: Array = []
	for value in values:
		var encoded: Variant = Protocol.encode(value)
		var wire := JSON.stringify(encoded, "", true, true)
		var decoded: Variant = Protocol.decode(JSON.parse_string(wire))
		var roundtrip := JSON.stringify(Protocol.encode(decoded), "", true, true)
		assert(wire == roundtrip, "Roundtrip %s: %s != %s" % [type_string(typeof(value)), wire, roundtrip])
		results.append(encoded)
	for type in range(TYPE_MAX):
		assert(Protocol.type_from_name(Protocol.type_name(type)) == type)

	Protocol.emit_ok("variant_contracts", {"cases": results.size(), "encoded": results})
	quit()
