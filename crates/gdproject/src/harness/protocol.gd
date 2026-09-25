## Shared wire protocol. Envelope payloads are ordinary JSON; callers explicitly
## encode Variant-valued fields. ScriptBootstrap emits only errors, never success.
## The Variant grammar is specified in gdview/src/variant.rs; this file mirrors it
## and the frozen golden fixture checks both sides against each other.
## Process-local Object/RID/Callable/Signal values are not transportable.
## Non-string dictionaries use a tagged array of [key, value] pairs. Dictionary
## keys matching reserved tags are escaped using that same representation.
@tool
extends RefCounted

const PROTOCOL_VERSION := 1
const RESULT_PREFIX := "GDKIT_RESULT:"
const MAX_DEPTH := 32
## Budget of Variant values visited by one encode/decode call, across the whole
## value rather than per container, so shared subgraphs cannot multiply work.
const MAX_ENTRIES := 100000
## 2^53 - 1: the largest integer every JSON reader represents exactly. Godot's
## parser yields float for every JSON number, so larger integers are tagged.
const SAFE_INTEGER := 9007199254740991
const RESERVED_KEYS := ["$variant", "$ref", "$resource"]
const I32_MIN := -2147483648
const I32_MAX := 2147483647
## Fixed-size tagged payloads: [component count, component kind].
const TUPLES := {
	TYPE_VECTOR2: [2, "float"],
	TYPE_VECTOR2I: [2, "i32"],
	TYPE_RECT2: [4, "float"],
	TYPE_RECT2I: [4, "i32"],
	TYPE_VECTOR3: [3, "float"],
	TYPE_VECTOR3I: [3, "i32"],
	TYPE_TRANSFORM2D: [6, "float"],
	TYPE_VECTOR4: [4, "float"],
	TYPE_VECTOR4I: [4, "i32"],
	TYPE_PLANE: [4, "float"],
	TYPE_QUATERNION: [4, "float"],
	TYPE_AABB: [6, "float"],
	TYPE_BASIS: [9, "float"],
	TYPE_TRANSFORM3D: [12, "float"],
	TYPE_PROJECTION: [16, "float"],
	TYPE_COLOR: [4, "float"],
}
## Packed array element kinds. Vector/Color elements are full tagged values.
const PACKED := {
	TYPE_PACKED_BYTE_ARRAY: "u8",
	TYPE_PACKED_INT32_ARRAY: "i32",
	TYPE_PACKED_INT64_ARRAY: "i64",
	TYPE_PACKED_FLOAT32_ARRAY: "float",
	TYPE_PACKED_FLOAT64_ARRAY: "float",
	TYPE_PACKED_STRING_ARRAY: "String",
	TYPE_PACKED_VECTOR2_ARRAY: "Vector2",
	TYPE_PACKED_VECTOR3_ARRAY: "Vector3",
	TYPE_PACKED_COLOR_ARRAY: "Color",
	TYPE_PACKED_VECTOR4_ARRAY: "Vector4",
}
const UNTYPED_ARRAY_ELEMENTS := [TYPE_NIL, TYPE_OBJECT, TYPE_RID, TYPE_CALLABLE, TYPE_SIGNAL]
const TYPE_NAMES := [
	"Nil",
	"bool",
	"int",
	"float",
	"String",
	"Vector2",
	"Vector2i",
	"Rect2",
	"Rect2i",
	"Vector3",
	"Vector3i",
	"Transform2D",
	"Vector4",
	"Vector4i",
	"Plane",
	"Quaternion",
	"AABB",
	"Basis",
	"Transform3D",
	"Projection",
	"Color",
	"StringName",
	"NodePath",
	"RID",
	"Object",
	"Callable",
	"Signal",
	"Dictionary",
	"Array",
	"PackedByteArray",
	"PackedInt32Array",
	"PackedInt64Array",
	"PackedFloat32Array",
	"PackedFloat64Array",
	"PackedStringArray",
	"PackedVector2Array",
	"PackedVector3Array",
	"PackedColorArray",
	"PackedVector4Array",
]


static func emit_ok(harness: String, payload: Variant) -> void:
	_emit(
		{
			"protocol": PROTOCOL_VERSION,
			"harness": harness,
			"ok": true,
			"payload": payload,
			"error": null,
		}
	)


static func emit_error(harness: String, stage: String, message: String, field: String = "") -> void:
	_emit(
		{
			"protocol": PROTOCOL_VERSION,
			"harness": harness,
			"ok": false,
			"payload": null,
			"error":
			{"stage": stage, "message": message, "field": null if field.is_empty() else field},
		}
	)


static func _emit(envelope: Dictionary) -> void:
	print(RESULT_PREFIX + stringify(envelope))


## JSON text that strict readers accept. JSON.stringify escapes only \b \f \n \r \t
## and writes other control characters raw; those can only occur inside strings,
## so escaping them afterwards is safe. Tab, newline and carriage return are left
## alone because indentation may use them.
static func stringify(value: Variant, indent: String = "") -> String:
	var text := JSON.stringify(value, indent, false, true)
	for code in range(1, 32):
		if code in [9, 10, 13]:
			continue
		var raw := char(code)
		if text.contains(raw):
			text = text.replace(raw, "\\u%04x" % code)
	return text


static func read_manifest(path: String) -> Dictionary:
	var file := FileAccess.open(path, FileAccess.READ)
	if file == null:
		return {
			"ok": false, "message": "Cannot open manifest (error %d)" % FileAccess.get_open_error()
		}
	var parser := JSON.new()
	if parser.parse(file.get_as_text()) != OK:
		return {
			"ok": false,
			"message":
			(
				"Invalid manifest JSON at line %d: %s"
				% [parser.get_error_line(), parser.get_error_message()]
			)
		}
	var paths: Variant = parser.data
	if not paths is Array:
		return {"ok": false, "message": "Manifest must be a JSON array of res:// paths"}
	for index in range(paths.size()):
		if not paths[index] is String or not valid_res_path(paths[index]):
			return {"ok": false, "message": "Invalid res:// path at manifest index %d" % index}
	return {"ok": true, "paths": paths}


static func valid_res_path(path: String) -> bool:
	if not path.begins_with("res://") or "\\" in path:
		return false
	for index in range(path.length()):
		if path.unicode_at(index) < 32:
			return false
	var parts := path.trim_prefix("res://").split("/")
	for part in parts:
		if part in ["", ".", "..", ".godot"]:
			return false
	return true


## A path a `$ref` or resource `script` may name: a valid res:// path that is not
## a built-in sub-resource (`res://scene.tscn::GDScript_abc`).
static func transportable_res_path(path: String) -> bool:
	return valid_res_path(path) and "::" not in path


static func _tag(type: int, value: Variant) -> Dictionary:
	return {"$variant": {"type": type_name(type), "value": value}}


## Errors are engine diagnostics, not silently accepted null values. Callers must
## reject a run with any diagnostic even if it also printed an envelope. Use
## try_encode to handle the error message instead.
static func encode(value: Variant) -> Variant:
	var result := try_encode(value)
	if not result.ok:
		push_error(result.error)
	return result.value


## {"ok": bool, "value": encoded JSON or null, "error": message}. Stops at the
## first error: cyclic containers or inline resources, more than MAX_DEPTH nesting,
## more than MAX_ENTRIES values in total, or a value the grammar cannot carry.
static func try_encode(value: Variant) -> Dictionary:
	var state := {"entries": 0, "path": [], "error": ""}
	var encoded: Variant = _encode(value, 0, state, false)
	if state.error:
		return {"ok": false, "value": null, "error": state.error}
	return {"ok": true, "value": encoded, "error": ""}


static func _fail(state: Dictionary, message: String) -> Variant:
	if not state.error:
		state.error = message
	return null


## Counts one visited value against the budget and depth limit.
static func _visit(state: Dictionary, depth: int) -> bool:
	if state.error:
		return false
	state.entries += 1
	if state.entries > MAX_ENTRIES:
		_fail(state, "Variant exceeds %d values" % MAX_ENTRIES)
		return false
	if depth > MAX_DEPTH:
		_fail(state, "Variant nesting exceeds depth %d" % MAX_DEPTH)
		return false
	return true


static func _on_path(value: Variant, state: Dictionary) -> bool:
	for ancestor in state.path:
		if is_same(ancestor, value):
			return true
	return false


## `component` marks float slots whose type the enclosing tag already fixes
## (tuple components, packed float arrays); only there may integral floats be bare.
static func _encode(value: Variant, depth: int, state: Dictionary, component: bool) -> Variant:
	if not _visit(state, depth):
		return null
	var type := typeof(value)
	match type:
		TYPE_NIL, TYPE_BOOL, TYPE_STRING:
			return value
		TYPE_INT:
			return (
				_tag(type, str(value)) if value > SAFE_INTEGER or value < -SAFE_INTEGER else value
			)
		TYPE_FLOAT:
			if is_nan(value):
				return _tag(type, "nan")
			if is_inf(value):
				return _tag(type, "inf" if value > 0 else "-inf")
			# JSON.stringify drops the sign of -0.0.
			if value == 0.0 and 1.0 / value < 0.0:
				return _tag(type, "-0.0")
			# Readers would take a bare integral number for an int.
			if not component and value == floor(value):
				return _tag(type, value)
			return value
		TYPE_STRING_NAME, TYPE_NODE_PATH:
			return _tag(type, str(value))
		TYPE_VECTOR2, TYPE_VECTOR2I:
			return _tag(type, _encode_items([value.x, value.y], depth, state, true))
		TYPE_VECTOR3, TYPE_VECTOR3I:
			return _tag(type, _encode_items([value.x, value.y, value.z], depth, state, true))
		TYPE_VECTOR4, TYPE_VECTOR4I, TYPE_QUATERNION:
			return _tag(
				type, _encode_items([value.x, value.y, value.z, value.w], depth, state, true)
			)
		TYPE_COLOR:
			return _tag(
				type, _encode_items([value.r, value.g, value.b, value.a], depth, state, true)
			)
		TYPE_RECT2, TYPE_RECT2I:
			return _tag(
				type,
				_encode_items(
					[value.position.x, value.position.y, value.size.x, value.size.y],
					depth,
					state,
					true
				)
			)
		TYPE_PLANE:
			return _tag(
				type,
				_encode_items(
					[value.normal.x, value.normal.y, value.normal.z, value.d], depth, state, true
				)
			)
		TYPE_AABB:
			return _tag(
				type,
				_encode_items(
					[
						value.position.x,
						value.position.y,
						value.position.z,
						value.size.x,
						value.size.y,
						value.size.z
					],
					depth,
					state,
					true
				)
			)
		TYPE_TRANSFORM2D:
			return _tag(
				type,
				_encode_items(
					[value.x.x, value.x.y, value.y.x, value.y.y, value.origin.x, value.origin.y],
					depth,
					state,
					true
				)
			)
		TYPE_BASIS:
			return _tag(type, _encode_items(_basis_items(value), depth, state, true))
		TYPE_TRANSFORM3D:
			return _tag(
				type,
				_encode_items(
					_basis_items(value.basis) + [value.origin.x, value.origin.y, value.origin.z],
					depth,
					state,
					true
				)
			)
		TYPE_PROJECTION:
			var items: Array = []
			for column in [value.x, value.y, value.z, value.w]:
				items.append_array([column.x, column.y, column.z, column.w])
			return _tag(type, _encode_items(items, depth, state, true))
		TYPE_ARRAY:
			if value.is_typed() and value.get_typed_builtin() == TYPE_OBJECT:
				return _fail(
					state,
					"Object-typed arrays require class/script metadata not defined by this grammar"
				)
			if _on_path(value, state):
				return _fail(state, "Variant contains a cyclic reference")
			state.path.append(value)
			var items := _encode_items(value, depth, state, false)
			state.path.pop_back()
			if value.is_typed():
				return {
					"$variant":
					{
						"type": "Array",
						"element": type_name(value.get_typed_builtin()),
						"value": items
					}
				}
			return items
		TYPE_DICTIONARY:
			if _on_path(value, state):
				return _fail(state, "Variant contains a cyclic reference")
			var plain := true
			for key in value:
				if not key is String or key in RESERVED_KEYS:
					plain = false
					break
			state.path.append(value)
			var result: Variant
			if plain:
				result = {}
				for key in value:
					result[key] = _encode(value[key], depth + 1, state, false)
					if state.error:
						break
			else:
				var pairs: Array = []
				for key in value:
					var encoded_key: Variant = _encode(key, depth + 1, state, false)
					pairs.append([encoded_key, _encode(value[key], depth + 1, state, false)])
					if state.error:
						break
				result = _tag(type, pairs)
			state.path.pop_back()
			return result
		TYPE_OBJECT:
			if value == null:
				return null
			if value is Resource:
				if transportable_res_path(value.resource_path):
					return {"$ref": value.resource_path}
				if _on_path(value, state):
					return _fail(state, "Variant contains a cyclic reference")
				var spec := {"properties": {}}
				var script: Script = value.get_script()
				if script != null:
					if not transportable_res_path(script.resource_path):
						return _fail(
							state, "Inline resource script must be a res:// file, not built in"
						)
					spec["script"] = script.resource_path
				else:
					spec["class"] = value.get_class()
				state.path.append(value)
				for property in value.get_property_list():
					if (
						property.usage & PROPERTY_USAGE_STORAGE
						and property.name not in ["script", "resource_path"]
					):
						spec.properties[property.name] = _encode(
							value.get(property.name), depth + 1, state, false
						)
						if state.error:
							break
				state.path.pop_back()
				return {"$resource": spec}
		_:
			if PACKED.has(type):
				return _tag(type, _encode_items(value, depth, state, true))
	return _fail(state, "Variant type %s is not transportable" % type_string(type))


static func _encode_items(items: Variant, depth: int, state: Dictionary, component: bool) -> Array:
	var result: Array = []
	for item in items:
		result.append(_encode(item, depth + 1, state, component))
		if state.error:
			break
	return result


static func _basis_items(value: Basis) -> Array:
	return [
		value.x.x,
		value.x.y,
		value.x.z,
		value.y.x,
		value.y.y,
		value.y.z,
		value.z.x,
		value.z.y,
		value.z.z
	]


## Decode trusted, grammar-validated specs. Invalid/unsupported input emits an
## error and returns null; use try_decode to handle the message instead.
## Resource specs can execute resource scripts; they are not an untrusted sandbox.
static func decode(json: Variant) -> Variant:
	var result := try_decode(json)
	if not result.ok:
		push_error(result.error)
	return result.value


## {"ok": bool, "value": decoded Variant or null, "error": message}.
static func try_decode(json: Variant) -> Dictionary:
	var state := {"entries": 0, "error": ""}
	var decoded: Variant = _decode(json, 0, state)
	if state.error:
		return {"ok": false, "value": null, "error": state.error}
	return {"ok": true, "value": decoded, "error": ""}


static func _decode(json: Variant, depth: int, state: Dictionary) -> Variant:
	if not _visit(state, depth):
		return null
	match typeof(json):
		TYPE_NIL, TYPE_BOOL, TYPE_STRING:
			return json
		TYPE_INT, TYPE_FLOAT:
			return _decode_number(json, false, state)
		TYPE_ARRAY:
			var items: Array = []
			for item in json:
				items.append(_decode(item, depth + 1, state))
				if state.error:
					return null
			return items
		TYPE_DICTIONARY:
			pass
		_:
			return _fail(state, "Unsupported JSON value of type %s" % type_string(typeof(json)))
	if json.has("$ref"):
		var path: Variant = json["$ref"]
		if json.size() != 1 or not path is String or not transportable_res_path(path):
			return _fail(state, "Invalid resource reference")
		var loaded := ResourceLoader.load(path)
		if loaded == null:
			return _fail(state, "Cannot load resource reference %s" % path)
		return loaded
	if json.has("$resource"):
		return _decode_resource(json, depth, state)
	if json.has("$variant"):
		if json.size() != 1 or not json["$variant"] is Dictionary:
			return _fail(state, "Invalid Variant tag")
		return _decode_tagged(json["$variant"], depth, state)
	var result := {}
	for key in json:
		result[key] = _decode(json[key], depth + 1, state)
		if state.error:
			return null
	return result


## Bare numbers are classified by value, not spelling, because Godot's parser
## yields float for every JSON number: integral values are int, others float.
static func _decode_number(json: Variant, integer: bool, state: Dictionary) -> Variant:
	if json is int:
		if json > SAFE_INTEGER or json < -SAFE_INTEGER:
			return _fail(state, "Integers beyond ±(2^53-1) must use the tagged int form")
		return json
	if not is_finite(json):
		return _fail(state, "Non-finite floats must use the tagged float form")
	if json != floor(json):
		if integer:
			return _fail(state, "Expected an integer, found %s" % json)
		return json
	if absf(json) > SAFE_INTEGER:
		return _fail(
			state, "Integral numbers beyond ±(2^53-1) are ambiguous; use a tagged int or float"
		)
	return int(json)


static func _decode_resource(json: Dictionary, depth: int, state: Dictionary) -> Variant:
	if json.size() != 1 or not json["$resource"] is Dictionary:
		return _fail(state, "Invalid resource spec")
	var spec: Dictionary = json["$resource"]
	for key in spec:
		if key not in ["class", "script", "properties"]:
			return _fail(state, "Unknown resource spec field %s" % key)
	if spec.has("class") == spec.has("script"):
		return _fail(state, "Resource requires exactly one of class or script")
	var properties: Variant = spec.get("properties", {})
	if not properties is Dictionary:
		return _fail(state, "Resource properties must be an object")
	var resource: Resource
	if spec.has("script"):
		if not spec.script is String or not transportable_res_path(spec.script):
			return _fail(state, "Resource script must be a saved res:// script path")
		var script := ResourceLoader.load(spec.script) as Script
		if (
			script == null
			or not script.can_instantiate()
			or not ClassDB.is_parent_class(script.get_instance_base_type(), "Resource")
		):
			return _fail(state, "Invalid resource script")
		resource = script.new() as Resource
	else:
		if (
			not spec["class"] is String
			or not ClassDB.can_instantiate(spec["class"])
			or not ClassDB.is_parent_class(spec["class"], "Resource")
		):
			return _fail(state, "Invalid resource class")
		resource = ClassDB.instantiate(spec["class"]) as Resource
	for property in properties:
		var value: Variant = _decode(properties[property], depth + 1, state)
		if state.error:
			return null
		resource.set(property, value)
	return resource


static func _decode_tagged(tag: Dictionary, depth: int, state: Dictionary) -> Variant:
	var name: Variant = tag.get("type")
	var type := type_from_name(name) if name is String else -1
	for key in tag:
		if key not in ["type", "value"] and not (key == "element" and type == TYPE_ARRAY):
			return _fail(state, "Unknown Variant tag field %s" % key)
	if not tag.has("value"):
		return _fail(state, "Variant tag requires a value")
	var value: Variant = tag.value
	match type:
		TYPE_INT:
			return _decode_tagged_int(value, state)
		TYPE_FLOAT:
			return _decode_tagged_float(value, state)
		TYPE_STRING_NAME, TYPE_NODE_PATH:
			if not value is String:
				return _fail(state, "%s value must be a string" % name)
			return StringName(value) if type == TYPE_STRING_NAME else NodePath(value)
		TYPE_DICTIONARY:
			if not value is Array:
				return _fail(state, "Dictionary value must be an array of [key, value] pairs")
			var result := {}
			for pair in value:
				if not pair is Array or pair.size() != 2:
					return _fail(state, "Dictionary value must be an array of [key, value] pairs")
				var key: Variant = _decode(pair[0], depth + 1, state)
				if state.error:
					return null
				if result.has(key):
					return _fail(state, "Duplicate dictionary key")
				result[key] = _decode(pair[1], depth + 1, state)
				if state.error:
					return null
			return result
		TYPE_ARRAY:
			var element: Variant = tag.get("element")
			var element_type := type_from_name(element) if element is String else -1
			if element_type < 0 or element_type in UNTYPED_ARRAY_ELEMENTS:
				return _fail(state, "Unsupported typed array element")
			if not value is Array:
				return _fail(state, "Typed array value must be an array")
			var items: Array = []
			for item in value:
				var decoded: Variant = _decode(item, depth + 1, state)
				if state.error:
					return null
				if typeof(decoded) != element_type:
					return _fail(
						state,
						(
							"Typed array item is %s, expected %s"
							% [type_string(typeof(decoded)), element]
						)
					)
				items.append(decoded)
			return Array(items, element_type, &"", null)
	if TUPLES.has(type):
		var count: int = TUPLES[type][0]
		if not value is Array or value.size() != count:
			return _fail(state, "%s value must be an array of %d numbers" % [name, count])
		var items := _decode_elements(value, TUPLES[type][1], depth, state)
		if state.error:
			return null
		return _build_tuple(type, items)
	if PACKED.has(type):
		if not value is Array:
			return _fail(state, "%s value must be an array" % name)
		var items := _decode_elements(value, PACKED[type], depth, state)
		if state.error:
			return null
		return type_convert(items, type)
	return _fail(state, "Unknown or unsupported Variant tag: %s" % str(name))


## Decodes items whose type the enclosing tag fixes. Numeric kinds accept bare
## numbers or the matching tagged scalar; other kinds must decode to that type.
static func _decode_elements(json: Array, kind: String, depth: int, state: Dictionary) -> Array:
	var items: Array = []
	for item in json:
		items.append(_decode_element(item, kind, depth + 1, state))
		if state.error:
			break
	return items


static func _decode_element(json: Variant, kind: String, depth: int, state: Dictionary) -> Variant:
	if kind not in ["float", "i32", "i64", "u8"]:
		var decoded: Variant = _decode(json, depth, state)
		var found := type_string(typeof(decoded))
		if not state.error and found != kind:
			return _fail(state, "Expected %s element, found %s" % [kind, found])
		return decoded
	if not _visit(state, depth):
		return null
	var integer := kind != "float"
	var number: Variant
	if json is int or json is float:
		number = _decode_number(json, integer, state)
	elif (
		json is Dictionary
		and json.size() == 1
		and json.get("$variant") is Dictionary
		and json["$variant"].size() == 2
		and json["$variant"].has("value")
		and json["$variant"].get("type") == ("int" if integer else "float")
	):
		var value: Variant = json["$variant"].value
		number = _decode_tagged_int(value, state) if integer else _decode_tagged_float(value, state)
	else:
		return _fail(state, "Expected a %s component" % ("int" if integer else "float"))
	if state.error:
		return null
	if kind == "float":
		return float(number)
	if (kind == "i32" and (number < I32_MIN or number > I32_MAX)) or (
		kind == "u8" and (number < 0 or number > 255)
	):
		return _fail(state, "Component %d is out of %s range" % [number, kind])
	return number


static func _decode_tagged_int(value: Variant, state: Dictionary) -> Variant:
	if not value is String or not _canonical_int(value):
		return _fail(
			state, "Tagged int value must be a canonical decimal string in the int64 range"
		)
	return value.to_int()


## Optional '-', no leading zeros (and no "-0"), digits only, within int64.
static func _canonical_int(text: String) -> bool:
	var negative := text.begins_with("-")
	var digits := text.trim_prefix("-")
	if digits.is_empty() or digits.length() > 19:
		return false
	if digits.begins_with("0") and (digits.length() > 1 or negative):
		return false
	for index in range(digits.length()):
		var code := digits.unicode_at(index)
		if code < 48 or code > 57:
			return false
	if digits.length() == 19:
		return digits <= ("9223372036854775808" if negative else "9223372036854775807")
	return true


static func _decode_tagged_float(value: Variant, state: Dictionary) -> Variant:
	if value is String:
		match value:
			"nan":
				return NAN
			"inf":
				return INF
			"-inf":
				return -INF
			"-0.0":
				# GDScript merges equal constants within a function, and 0.0 == -0.0;
				# keep this the function's only zero literal.
				return -0.0
		return _fail(state, "Tagged float strings are nan, inf, -inf and -0.0")
	if (value is float or value is int) and is_finite(value):
		return float(value)
	return _fail(state, "Tagged float value must be a finite number or nan, inf, -inf, -0.0")


static func _build_tuple(type: int, value: Array) -> Variant:
	match type:
		TYPE_VECTOR2:
			return Vector2(value[0], value[1])
		TYPE_VECTOR2I:
			return Vector2i(value[0], value[1])
		TYPE_VECTOR3:
			return Vector3(value[0], value[1], value[2])
		TYPE_VECTOR3I:
			return Vector3i(value[0], value[1], value[2])
		TYPE_VECTOR4:
			return Vector4(value[0], value[1], value[2], value[3])
		TYPE_VECTOR4I:
			return Vector4i(value[0], value[1], value[2], value[3])
		TYPE_QUATERNION:
			return Quaternion(value[0], value[1], value[2], value[3])
		TYPE_COLOR:
			return Color(value[0], value[1], value[2], value[3])
		TYPE_RECT2:
			return Rect2(value[0], value[1], value[2], value[3])
		TYPE_RECT2I:
			return Rect2i(value[0], value[1], value[2], value[3])
		TYPE_PLANE:
			return Plane(Vector3(value[0], value[1], value[2]), value[3])
		TYPE_AABB:
			return AABB(
				Vector3(value[0], value[1], value[2]), Vector3(value[3], value[4], value[5])
			)
		TYPE_TRANSFORM2D:
			return Transform2D(
				Vector2(value[0], value[1]),
				Vector2(value[2], value[3]),
				Vector2(value[4], value[5])
			)
		TYPE_BASIS:
			return _decode_basis(value)
		TYPE_TRANSFORM3D:
			return Transform3D(_decode_basis(value), Vector3(value[9], value[10], value[11]))
	return Projection(
		Vector4(value[0], value[1], value[2], value[3]),
		Vector4(value[4], value[5], value[6], value[7]),
		Vector4(value[8], value[9], value[10], value[11]),
		Vector4(value[12], value[13], value[14], value[15])
	)


static func _decode_basis(value: Array) -> Basis:
	return Basis(
		Vector3(value[0], value[1], value[2]),
		Vector3(value[3], value[4], value[5]),
		Vector3(value[6], value[7], value[8])
	)


static func type_name(type: int) -> String:
	if type < 0 or type >= TYPE_NAMES.size():
		push_error("Unknown Variant type code: %d" % type)
		return ""
	return TYPE_NAMES[type]


static func type_from_name(name: String) -> int:
	return TYPE_NAMES.find(name)
