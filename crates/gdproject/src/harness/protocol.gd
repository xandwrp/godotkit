## Shared wire protocol. Envelope payloads are ordinary JSON; callers explicitly
## encode Variant-valued fields. ScriptBootstrap emits only errors, never success.
## Process-local Object/RID/Callable/Signal values are not transportable.
## Non-string dictionaries use a tagged array of [key, value] pairs. Dictionary
## keys matching reserved tags are escaped using that same representation.
@tool
extends RefCounted

const PROTOCOL_VERSION := 1
const RESULT_PREFIX := "GDKIT_RESULT:"
const MAX_DEPTH := 32
const MAX_ENTRIES := 100000
const SAFE_INTEGER := 9007199254740992
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
	print(
		(
			RESULT_PREFIX
			+ (
				JSON
				. stringify(
					{
						"protocol": PROTOCOL_VERSION,
						"harness": harness,
						"ok": true,
						"payload": payload,
						"error": null,
					},
					"",
					false,
					true
				)
			)
		)
	)


static func emit_error(harness: String, stage: String, message: String, field: String = "") -> void:
	print(
		(
			RESULT_PREFIX
			+ (
				JSON
				. stringify(
					{
						"protocol": PROTOCOL_VERSION,
						"harness": harness,
						"ok": false,
						"payload": null,
						"error":
						{
							"stage": stage,
							"message": message,
							"field": null if field.is_empty() else field
						},
					},
					"",
					false,
					true
				)
			)
		)
	)


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


static func _tag(type: int, value: Variant) -> Dictionary:
	return {"$variant": {"type": type_name(type), "value": value}}


## Errors are engine diagnostics, not silently accepted null values. Callers must
## reject a run with any diagnostic even if it also printed an envelope.
static func encode(value: Variant, depth: int = 0) -> Variant:
	if depth > MAX_DEPTH:
		push_error("Variant nesting exceeds %d (possibly a cycle)" % MAX_DEPTH)
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
			return value
		TYPE_STRING_NAME, TYPE_NODE_PATH:
			return _tag(type, str(value))
		TYPE_VECTOR2, TYPE_VECTOR2I:
			return _tag(type, _encode_items([value.x, value.y], depth))
		TYPE_VECTOR3, TYPE_VECTOR3I:
			return _tag(type, _encode_items([value.x, value.y, value.z], depth))
		TYPE_VECTOR4, TYPE_VECTOR4I, TYPE_QUATERNION:
			return _tag(type, _encode_items([value.x, value.y, value.z, value.w], depth))
		TYPE_COLOR:
			return _tag(type, _encode_items([value.r, value.g, value.b, value.a], depth))
		TYPE_RECT2, TYPE_RECT2I:
			return _tag(
				type,
				_encode_items(
					[value.position.x, value.position.y, value.size.x, value.size.y], depth
				)
			)
		TYPE_PLANE:
			return _tag(
				type,
				_encode_items([value.normal.x, value.normal.y, value.normal.z, value.d], depth)
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
					depth
				)
			)
		TYPE_TRANSFORM2D:
			return _tag(
				type,
				_encode_items(
					[value.x.x, value.x.y, value.y.x, value.y.y, value.origin.x, value.origin.y],
					depth
				)
			)
		TYPE_BASIS:
			return _tag(type, _encode_items(_basis_items(value), depth))
		TYPE_TRANSFORM3D:
			return _tag(
				type,
				_encode_items(
					_basis_items(value.basis) + [value.origin.x, value.origin.y, value.origin.z],
					depth
				)
			)
		TYPE_PROJECTION:
			var items: Array = []
			for column in [value.x, value.y, value.z, value.w]:
				items.append_array([column.x, column.y, column.z, column.w])
			return _tag(type, _encode_items(items, depth))
		TYPE_ARRAY:
			var items := _encode_items(value, depth)
			if value.is_typed():
				if value.get_typed_builtin() == TYPE_OBJECT:
					push_error(
						"Object-typed arrays require class/script metadata not defined by this grammar"
					)
					return null
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
			if value.size() > MAX_ENTRIES:
				push_error("Variant dictionary exceeds entry limit")
				return null
			var plain := true
			for key in value:
				if not key is String or key in ["$variant", "$ref", "$resource"]:
					plain = false
			if plain:
				var result := {}
				for key in value:
					result[key] = encode(value[key], depth + 1)
				return result
			var pairs: Array = []
			for key in value:
				pairs.append([encode(key, depth + 1), encode(value[key], depth + 1)])
			return _tag(type, pairs)
		TYPE_OBJECT:
			if value == null:
				return null
			if value is Resource:
				if valid_res_path(value.resource_path) and "::" not in value.resource_path:
					return {"$ref": value.resource_path}
				var spec := {"properties": {}}
				var script: Script = value.get_script()
				if script != null:
					if not valid_res_path(script.resource_path):
						push_error("Inline resource script must have a res:// path")
						return null
					spec["script"] = script.resource_path
				else:
					spec["class"] = value.get_class()
				for property in value.get_property_list():
					if (
						property.usage & PROPERTY_USAGE_STORAGE
						and property.name not in ["script", "resource_path"]
					):
						spec.properties[property.name] = encode(value.get(property.name), depth + 1)
				return {"$resource": spec}
		_:
			if type >= TYPE_PACKED_BYTE_ARRAY and type <= TYPE_PACKED_VECTOR4_ARRAY:
				return _tag(type, _encode_items(value, depth))
	push_error("Variant type %s is not transportable" % type_name(type))
	return null


static func _encode_items(items: Variant, depth: int) -> Array:
	var result: Array = []
	if items.size() > MAX_ENTRIES:
		push_error("Variant array exceeds entry limit")
		return result
	for item in items:
		result.append(encode(item, depth + 1))
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


## Decode trusted, grammar-validated specs. Invalid/unsupported tags emit errors.
## Resource specs can execute resource scripts; they are not an untrusted sandbox.
static func decode(json: Variant, depth: int = 0) -> Variant:
	if depth > MAX_DEPTH:
		push_error("Variant nesting exceeds depth limit")
		return null
	if json is Array:
		var items: Array = []
		if json.size() > MAX_ENTRIES:
			push_error("Variant array exceeds entry limit")
			return null
		for item in json:
			items.append(decode(item, depth + 1))
		return items
	if not json is Dictionary:
		# Godot's JSON parser returns float for every JSON number.
		if json is float and is_finite(json) and absf(json) <= SAFE_INTEGER and json == floor(json):
			return int(json)
		return json
	if json.has("$ref"):
		if json.size() != 1 or not json["$ref"] is String or not valid_res_path(json["$ref"]):
			push_error("Invalid resource reference")
			return null
		return ResourceLoader.load(json["$ref"])
	if json.has("$resource"):
		var spec: Dictionary = json["$resource"]
		if spec.has("class") == spec.has("script"):
			push_error("Resource requires exactly one of class or script")
			return null
		var resource: Resource
		if spec.has("script"):
			var script: Script = decode({"$ref": spec.script}, depth + 1) as Script
			if (
				script == null
				or not script.can_instantiate()
				or not ClassDB.is_parent_class(script.get_instance_base_type(), "Resource")
			):
				push_error("Invalid resource script")
				return null
			resource = script.new() as Resource
		else:
			if (
				not ClassDB.can_instantiate(spec["class"])
				or not ClassDB.is_parent_class(spec["class"], "Resource")
			):
				push_error("Invalid resource class")
				return null
			resource = ClassDB.instantiate(spec["class"]) as Resource
		for property in spec.get("properties", {}):
			resource.set(property, decode(spec.properties[property], depth + 1))
		return resource
	if json.has("$variant"):
		var tag: Dictionary = json["$variant"]
		var type := type_from_name(tag.get("type", ""))
		var value: Variant = tag.get("value")
		if type == TYPE_INT:
			return int(value)
		if type == TYPE_FLOAT:
			match value:
				"nan":
					return NAN
				"inf":
					return INF
				"-inf":
					return -INF
			return float(value)
		if type == TYPE_STRING_NAME:
			return StringName(value)
		if type == TYPE_NODE_PATH:
			return NodePath(value)
		if type == TYPE_DICTIONARY:
			var pairs := {}
			for pair in value:
				pairs[decode(pair[0], depth + 1)] = decode(pair[1], depth + 1)
			return pairs
		value = decode(value, depth + 1)
		if type == TYPE_ARRAY:
			var element := type_from_name(tag.get("element", ""))
			if element < 0 or element == TYPE_OBJECT:
				push_error("Unsupported typed array element")
				return null
			return Array(value, element, &"", null)
		if type >= TYPE_PACKED_BYTE_ARRAY and type <= TYPE_PACKED_VECTOR4_ARRAY:
			return type_convert(value, type)
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
			TYPE_PROJECTION:
				return Projection(
					Vector4(value[0], value[1], value[2], value[3]),
					Vector4(value[4], value[5], value[6], value[7]),
					Vector4(value[8], value[9], value[10], value[11]),
					Vector4(value[12], value[13], value[14], value[15])
				)
		push_error("Unknown or unsupported Variant tag: %s" % tag.get("type", ""))
		return null
	var result := {}
	if json.size() > MAX_ENTRIES:
		push_error("Variant dictionary exceeds entry limit")
		return null
	for key in json:
		result[key] = decode(json[key], depth + 1)
	return result


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
