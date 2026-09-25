extends SceneTree
const Protocol := preload("../crates/gdproject/src/harness/protocol.gd")


func _initialize() -> void:
	var typed: Array[Vector2] = [Vector2(1, 2)]
	var typed_floats: Array[float] = [1.0, 2.5]
	var inline := Resource.new()
	inline.resource_name = "inline"
	var scripted: Resource = load("res://scripted_resource.gd").new()
	# Literal 0.0 and -0.0 share one compiler constant, so build zeros at runtime.
	var zero := float(0)
	var mixed_keys := {1: "int"}
	mixed_keys[1.0] = "float"
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
		# Integral and signed-zero floats keep their type; bare numbers are ints.
		3.0, -zero, zero, 1e20, 0.1, [2.0, 2], mixed_keys, typed_floats,
		9007199254740991, -9007199254740991, 9007199254740992,
		Vector2(-zero, NAN), PackedFloat64Array([1.0, -zero, INF]),
		# Control characters JSON.stringify would otherwise leave raw.
		"bell\u0001unit\u001fdel\u007f", {"key\u0002": "\u001b[0m"},
		{"$variant": "literal", "$resource": 1}, [], {}, [[], {}],
	]
	var failures: Array = []
	var results: Array = []
	for value in values:
		var encoded: Variant = Protocol.encode(value)
		var wire := Protocol.stringify(encoded)
		var decoded: Variant = Protocol.decode(JSON.parse_string(wire))
		var roundtrip := Protocol.stringify(Protocol.encode(decoded))
		if wire != roundtrip:
			failures.append("wire %s != %s" % [wire, roundtrip])
		if typeof(decoded) != typeof(value):
			failures.append("type %s != %s" % [type_string(typeof(decoded)), wire])
		elif typeof(value) == TYPE_FLOAT and not is_nan(value):
			if decoded != value or 1.0 / decoded != 1.0 / value:
				failures.append("float %s != %s" % [decoded, wire])
		elif typeof(value) not in [TYPE_OBJECT, TYPE_FLOAT, TYPE_VECTOR2]:
			if decoded != value:
				failures.append("value %s != %s" % [var_to_str(decoded), wire])
		results.append(encoded)
	var type_names: Array = []
	for type in range(TYPE_MAX):
		type_names.append(type_string(type))
		if Protocol.type_name(type) != type_string(type):
			failures.append("type name %d" % type)
		if Protocol.type_from_name(type_string(type)) != type:
			failures.append("type code %d" % type)
	Protocol.emit_ok(
		"variant_contracts",
		{"cases": results.size(), "encoded": results, "type_names": type_names, "failures": failures}
	)
	quit()
