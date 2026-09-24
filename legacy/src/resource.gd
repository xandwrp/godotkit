extends SceneTree

const PREFIX := "GDKIT_RESOURCE_RESULT:"
const MAX_DEPTH := 16
const VARIANT_TYPES := {
	"int": TYPE_INT, "StringName": TYPE_STRING_NAME, "NodePath": TYPE_NODE_PATH,
	"Vector2": TYPE_VECTOR2, "Vector3": TYPE_VECTOR3, "Vector4": TYPE_VECTOR4,
	"Vector2i": TYPE_VECTOR2I, "Vector3i": TYPE_VECTOR3I, "Vector4i": TYPE_VECTOR4I,
	"Color": TYPE_COLOR,
	"Array": TYPE_ARRAY, "Dictionary": TYPE_DICTIONARY,
	"PackedByteArray": TYPE_PACKED_BYTE_ARRAY, "PackedInt32Array": TYPE_PACKED_INT32_ARRAY, "PackedInt64Array": TYPE_PACKED_INT64_ARRAY,
	"PackedFloat32Array": TYPE_PACKED_FLOAT32_ARRAY, "PackedFloat64Array": TYPE_PACKED_FLOAT64_ARRAY, "PackedStringArray": TYPE_PACKED_STRING_ARRAY,
	"PackedVector2Array": TYPE_PACKED_VECTOR2_ARRAY, "PackedVector3Array": TYPE_PACKED_VECTOR3_ARRAY, "PackedVector4Array": TYPE_PACKED_VECTOR4_ARRAY, "PackedColorArray": TYPE_PACKED_COLOR_ARRAY,
	"Rect2": TYPE_RECT2, "Rect2i": TYPE_RECT2I,
	"Transform2D": TYPE_TRANSFORM2D, "Transform3D": TYPE_TRANSFORM3D,
	"Quaternion": TYPE_QUATERNION, "Basis": TYPE_BASIS, "Plane": TYPE_PLANE, "AABB": TYPE_AABB,
}
const VARIANT_COMPONENTS := {
	TYPE_VECTOR2: ["x", "y"], TYPE_VECTOR3: ["x", "y", "z"], TYPE_VECTOR4: ["x", "y", "z", "w"],
	TYPE_VECTOR2I: ["x", "y"], TYPE_VECTOR3I: ["x", "y", "z"], TYPE_VECTOR4I: ["x", "y", "z", "w"],
	TYPE_COLOR: ["r", "g", "b", "a"],
	TYPE_RECT2: ["position.x", "position.y", "size.x", "size.y"],
	TYPE_RECT2I: ["position.x", "position.y", "size.x", "size.y"],
	TYPE_TRANSFORM2D: ["x.x", "x.y", "y.x", "y.y", "origin.x", "origin.y"],
	TYPE_BASIS: ["x.x", "x.y", "x.z", "y.x", "y.y", "y.z", "z.x", "z.y", "z.z"],
	TYPE_TRANSFORM3D: ["basis.x.x", "basis.x.y", "basis.x.z", "basis.y.x", "basis.y.y", "basis.y.z", "basis.z.x", "basis.z.y", "basis.z.z", "origin.x", "origin.y", "origin.z"],
	TYPE_QUATERNION: ["x", "y", "z", "w"],
	TYPE_PLANE: ["normal.x", "normal.y", "normal.z", "d"],
	TYPE_AABB: ["position.x", "position.y", "position.z", "size.x", "size.y", "size.z"],
}

const SCALAR_TYPES := { "bool": TYPE_BOOL, "float": TYPE_FLOAT, "String": TYPE_STRING }
const PACKED_ELEMENTS := {
	TYPE_PACKED_BYTE_ARRAY: TYPE_INT, TYPE_PACKED_INT32_ARRAY: TYPE_INT, TYPE_PACKED_INT64_ARRAY: TYPE_INT,
	TYPE_PACKED_FLOAT32_ARRAY: TYPE_FLOAT, TYPE_PACKED_FLOAT64_ARRAY: TYPE_FLOAT, TYPE_PACKED_STRING_ARRAY: TYPE_STRING,
	TYPE_PACKED_VECTOR2_ARRAY: TYPE_VECTOR2, TYPE_PACKED_VECTOR3_ARRAY: TYPE_VECTOR3, TYPE_PACKED_VECTOR4_ARRAY: TYPE_VECTOR4, TYPE_PACKED_COLOR_ARRAY: TYPE_COLOR,
}

var failed := false


func fail(stage: String, message: String, field: String = "") -> void:
	if failed: return
	failed = true
	print(PREFIX + JSON.stringify({ "status": "error", "stage": stage, "field": field, "message": message }))
	quit(1)


func construct(spec: Dictionary, field: String = "") -> Resource:
	var resource: Resource
	if spec.has("class") and spec["class"] != null:
		var class_id := str(spec["class"])
		if not ClassDB.class_exists(class_id) or not ClassDB.can_instantiate(class_id) or not ClassDB.is_parent_class(class_id, "Resource"):
			fail("construct", "Class must be an instantiable Resource", field)
			return null
		resource = ClassDB.instantiate(class_id)
	else:
		var script := load(str(spec.script)) as Script
		if script == null or not script.can_instantiate() or not ClassDB.is_parent_class(script.get_instance_base_type(), "Resource"):
			fail("construct", "Script must instantiate a Resource without constructor arguments", field)
			return null
		var constructor_script := script
		while constructor_script != null:
			for method: Dictionary in constructor_script.get_script_method_list():
				if method.name == "_init" and method.args.size() > method.default_args.size():
					fail("construct", "Script constructor requires arguments", field)
					return null
			constructor_script = constructor_script.get_base_script()
		resource = script.new() as Resource
	if resource == null:
		fail("construct", "Could not instantiate Resource", field)
	return resource


func unsupported_reason(property: Dictionary) -> String:
	var usage := int(property.usage)
	if usage & PROPERTY_USAGE_STORAGE == 0 or usage & PROPERTY_USAGE_READ_ONLY != 0 or property.name == "script":
		return "Property must be writable and serialized"
	if int(property.type) == TYPE_OBJECT:
		if resource_constraints(property).is_empty():
			return "Object field must declare a Resource type"
	elif int(property.type) == TYPE_ARRAY:
		if not property.has("element") or (resource_constraints(property.element).is_empty() and not scalar_kind(int(property.element.type))):
			return "Array must declare a supported scalar or Resource element type"
	elif int(property.type) == TYPE_DICTIONARY:
		if not property.has("key_type") or not scalar_kind(property.key_type) or not scalar_kind(property.value_type):
			return "Dictionary must declare supported scalar key and value types"
	elif int(property.type) not in [TYPE_BOOL, TYPE_INT, TYPE_FLOAT, TYPE_STRING] and variant_contract(int(property.type)).is_empty():
		return "Only scalar and Resource fields are supported"
	return ""


func resource_constraints(property: Dictionary) -> Array:
	var constraints := []
	if int(property.type) != TYPE_OBJECT: return constraints
	if property.has("element_script"):
		var element_script: Script = property.element_script
		if ClassDB.is_parent_class(element_script.get_instance_base_type(), "Resource"):
			constraints.append({ "class": element_script.get_instance_base_type(), "script": element_script.resource_path })
		return constraints
	var names := str(property.hint_string) if int(property.hint) == PROPERTY_HINT_RESOURCE_TYPE else str(property.class_name)
	for class_id: String in names.split(",", false):
		class_id = class_id.strip_edges()
		if ClassDB.class_exists(class_id) and ClassDB.is_parent_class(class_id, "Resource"):
			constraints.append({ "class": class_id, "script": null })
		else:
			for entry: Dictionary in ProjectSettings.get_global_class_list():
				if entry.class == class_id:
					var script := load(str(entry.path)) as Script
					if script != null and ClassDB.is_parent_class(script.get_instance_base_type(), "Resource"):
						constraints.append({ "class": class_id, "script": str(entry.path) })
	return constraints


func accepts_resource(property: Dictionary, value: Resource) -> bool:
	if value == null: return true
	for constraint: Dictionary in resource_constraints(property):
		if constraint.script != null:
			if is_instance_of(value, load(str(constraint.script))): return true
		elif value.is_class(str(constraint.class)):
			return true
	return false


func property_metadata(resource: Resource) -> Dictionary:
	var metadata := {}
	for property: Dictionary in resource.get_property_list():
		if int(property.usage) & (PROPERTY_USAGE_GROUP | PROPERTY_USAGE_SUBGROUP | PROPERTY_USAGE_CATEGORY) == 0:
			if int(property.type) == TYPE_ARRAY:
				var current: Variant = resource.get(property.name)
				if current is Array and current.is_typed():
					var element_script := current.get_typed_script() as Script
					property["element"] = { "type": current.get_typed_builtin(), "hint": PROPERTY_HINT_NONE, "hint_string": "", "class_name": current.get_typed_class_name() }
					if element_script != null:
						property.element["element_script"] = element_script
			elif int(property.type) == TYPE_DICTIONARY:
				var current: Variant = resource.get(property.name)
				if current is Dictionary:
					property["key_type"] = current.get_typed_key_builtin()
					property["value_type"] = current.get_typed_value_builtin()
			metadata[property.name] = property
	return metadata


func scalar_kind(kind: int) -> bool:
	return kind in SCALAR_TYPES.values() or (kind in VARIANT_TYPES.values() and kind not in [TYPE_ARRAY, TYPE_DICTIONARY] and not PACKED_ELEMENTS.has(kind))


func scalar_name(kind: int) -> String:
	for table: Dictionary in [SCALAR_TYPES, VARIANT_TYPES]:
		for name: String in table:
			if table[name] == kind: return name
	return ""


func scalar_input(value: Variant, kind: int, location: String) -> Variant:
	if value is Dictionary:
		return decode_variant(value, kind, location)
	if kind == TYPE_INT and value is float and is_finite(value) and value == floor(value) and abs(value) <= 9007199254740991.0:
		value = int(value)
	elif kind == TYPE_FLOAT and typeof(value) == TYPE_INT:
		value = float(value)
	if typeof(value) != kind or (kind == TYPE_FLOAT and not is_finite(value)):
		fail("validate", "Expected %s scalar" % scalar_name(kind), location)
		return null
	return value


func scalar_output(value: Variant) -> Variant:
	if typeof(value) == TYPE_INT and value >= -9007199254740991 and value <= 9007199254740991: return value
	return encode_variant(value) if typeof(value) in VARIANT_TYPES.values() else value


func scalar_contract(kind: int) -> Dictionary:
	var contract := variant_contract(kind)
	if contract.is_empty(): contract = { "type": scalar_name(kind) }
	contract["accepted_inputs"] = ["scalar", "$variant"] if kind == TYPE_INT else (["$variant"] if kind in VARIANT_TYPES.values() else ["scalar"])
	return contract


func container_contract(property: Dictionary) -> Dictionary:
	if int(property.type) == TYPE_ARRAY and (not property.has("element") or not scalar_kind(int(property.element.type))): return {}
	if int(property.type) == TYPE_DICTIONARY and (not property.has("key_type") or not scalar_kind(property.key_type) or not scalar_kind(property.value_type)): return {}
	var contract := variant_contract(int(property.type))
	if int(property.type) == TYPE_ARRAY and property.has("element") and scalar_kind(int(property.element.type)):
		contract["element_type"] = scalar_name(int(property.element.type))
		contract["element_contract"] = scalar_contract(int(property.element.type))
	elif int(property.type) == TYPE_DICTIONARY and property.has("key_type"):
		contract["key_type"] = scalar_name(property.key_type)
		contract["value_type"] = scalar_name(property.value_type)
		contract["key_contract"] = scalar_contract(property.key_type)
		contract["value_contract"] = scalar_contract(property.value_type)
	return contract


func variant_contract(kind: int) -> Dictionary:
	for tag: String in VARIANT_TYPES:
		if VARIANT_TYPES[tag] == kind:
			var contract := { "tag": "$variant", "type": tag, "value_encoding": "string" }
			if kind in [TYPE_ARRAY, TYPE_DICTIONARY]:
				contract["value_encoding"] = "object"
				contract["required_fields"] = ["element_type", "items"] if kind == TYPE_ARRAY else ["key_type", "value_type", "entries"]
			elif PACKED_ELEMENTS.has(kind):
				contract["value_encoding"] = "array"
				contract["element_type"] = scalar_name(PACKED_ELEMENTS[kind])
				contract["element_contract"] = scalar_contract(PACKED_ELEMENTS[kind])
				contract["exact_elements"] = true
				if kind in [TYPE_PACKED_BYTE_ARRAY, TYPE_PACKED_INT32_ARRAY]:
					contract["element_minimum"] = 0 if kind == TYPE_PACKED_BYTE_ARRAY else -2147483648
					contract["element_maximum"] = 255 if kind == TYPE_PACKED_BYTE_ARRAY else 2147483647
			if VARIANT_COMPONENTS.has(kind):
				contract["value_encoding"] = "array"
				contract["components"] = VARIANT_COMPONENTS[kind]
				contract["length"] = VARIANT_COMPONENTS[kind].size()
				contract["component_type"] = "integer" if kind in [TYPE_VECTOR2I, TYPE_VECTOR3I, TYPE_VECTOR4I, TYPE_RECT2I] else "finite number"
				contract["exact_components"] = true
				if kind in [TYPE_VECTOR2I, TYPE_VECTOR3I, TYPE_VECTOR4I, TYPE_RECT2I]:
					contract["component_minimum"] = -2147483648
					contract["component_maximum"] = 2147483647
			if kind == TYPE_INT:
				contract["minimum"] = "-9223372036854775808"
				contract["maximum"] = "9223372036854775807"
				contract["format"] = "canonical signed decimal"
			return contract
	return {}


func component_values(value: Variant) -> Array:
	var components := []
	for component: String in VARIANT_COMPONENTS[typeof(value)]:
		var current: Variant = value
		for member: String in component.split("."):
			current = current[member]
		components.append(current)
	return components


func encode_variant(value: Variant) -> Dictionary:
	var kind := typeof(value)
	var payload: Variant = str(value)
	if VARIANT_COMPONENTS.has(kind):
		payload = component_values(value)
	elif kind == TYPE_ARRAY:
		var items := []
		for item: Variant in value: items.append(scalar_output(item))
		payload = { "element_type": scalar_name(value.get_typed_builtin()), "items": items }
	elif kind == TYPE_DICTIONARY:
		var entries := []
		for key: Variant in value: entries.append([scalar_output(key), scalar_output(value[key])])
		payload = { "key_type": scalar_name(value.get_typed_key_builtin()), "value_type": scalar_name(value.get_typed_value_builtin()), "entries": entries }
	elif PACKED_ELEMENTS.has(kind):
		payload = []
		for item: Variant in value: payload.append(scalar_output(item))
	return { "$variant": { "type": variant_contract(kind).type, "value": payload } }


func decode_variant(value: Dictionary, kind: int, location: String, property: Dictionary = {}) -> Variant:
	var tagged: Variant = value.get("$variant")
	if value.size() != 1 or not tagged is Dictionary or tagged.size() != 2 or not tagged.has("type") or not tagged.has("value"):
		fail("validate", "Expected $variant with exactly type and value", location)
		return null
	if not tagged.type is String or not VARIANT_TYPES.has(tagged.type) or VARIANT_TYPES[tagged.type] != kind:
		fail("validate", "Variant tag must match the declared property type", location)
		return null
	if kind in [TYPE_ARRAY, TYPE_DICTIONARY] or PACKED_ELEMENTS.has(kind):
		return decode_container(tagged.value, kind, location + ".$variant.value", property)
	if VARIANT_COMPONENTS.has(kind):
		return decode_components(tagged.value, kind, location)
	if not tagged.value is String:
		fail("validate", "Tagged scalar value must be a string", location)
		return null
	var text: String = tagged.value
	match kind:
		TYPE_INT:
			var integer := text.to_int()
			if str(integer) != text:
				fail("validate", "Expected canonical signed 64-bit decimal integer", location)
				return null
			return integer
		TYPE_STRING_NAME:
			var name_value := StringName(text)
			if str(name_value) == text: return name_value
		TYPE_NODE_PATH:
			var path_value := NodePath(text)
			if str(path_value) == text: return path_value
	fail("validate", "Tagged value cannot be represented exactly", location)
	return null


func decode_container(payload: Variant, kind: int, location: String, property: Dictionary) -> Variant:
	var result: Variant
	if PACKED_ELEMENTS.has(kind):
		if not payload is Array:
			fail("validate", "Expected packed array entries", location)
			return null
		match kind:
			TYPE_PACKED_BYTE_ARRAY: result = PackedByteArray()
			TYPE_PACKED_INT32_ARRAY: result = PackedInt32Array()
			TYPE_PACKED_INT64_ARRAY: result = PackedInt64Array()
			TYPE_PACKED_FLOAT32_ARRAY: result = PackedFloat32Array()
			TYPE_PACKED_FLOAT64_ARRAY: result = PackedFloat64Array()
			TYPE_PACKED_STRING_ARRAY: result = PackedStringArray()
			TYPE_PACKED_VECTOR2_ARRAY: result = PackedVector2Array()
			TYPE_PACKED_VECTOR3_ARRAY: result = PackedVector3Array()
			TYPE_PACKED_VECTOR4_ARRAY: result = PackedVector4Array()
			TYPE_PACKED_COLOR_ARRAY: result = PackedColorArray()
		for index in payload.size():
			var entry_path := location + "[%d]" % index
			var entry: Variant = scalar_input(payload[index], PACKED_ELEMENTS[kind], entry_path)
			if failed: return null
			var contract := variant_contract(kind)
			if contract.has("element_minimum") and (entry < contract.element_minimum or entry > contract.element_maximum):
				fail("validate", "Packed integer entry is outside its storage range", entry_path)
				return null
			result.append(entry)
			if result[index] != entry:
				fail("validate", "Packed entry cannot be represented exactly", entry_path)
				return null
		return result
	if not payload is Dictionary:
		fail("validate", "Expected typed container payload", location)
		return null
	if kind == TYPE_ARRAY:
		if payload.size() != 2 or not payload.has("element_type") or not payload.has("items") or not payload.items is Array or not property.has("element") or payload.element_type != scalar_name(int(property.element.type)):
			fail("validate", "Array payload must declare the property's scalar element_type and items", location)
			return null
		result = Array([], int(property.element.type), &"", null)
		for index in payload.items.size():
			var item: Variant = scalar_input(payload.items[index], int(property.element.type), location + ".items[%d]" % index)
			if failed: return null
			result.append(item)
	else:
		if payload.size() != 3 or not payload.has("key_type") or not payload.has("value_type") or not payload.has("entries") or not payload.entries is Array or not property.has("key_type") or payload.key_type != scalar_name(property.key_type) or payload.value_type != scalar_name(property.value_type):
			fail("validate", "Dictionary payload must declare the property's scalar key_type, value_type, and entries", location)
			return null
		result = Dictionary({}, property.key_type, &"", null, property.value_type, &"", null)
		for index in payload.entries.size():
			var entry: Variant = payload.entries[index]
			var entry_path := location + ".entries[%d]" % index
			if not entry is Array or entry.size() != 2:
				fail("validate", "Expected a dictionary [key, value] pair", entry_path)
				return null
			var key: Variant = scalar_input(entry[0], property.key_type, entry_path + "[0]")
			if failed: return null
			var item: Variant = scalar_input(entry[1], property.value_type, entry_path + "[1]")
			if failed: return null
			if result.has(key):
				fail("validate", "Duplicate dictionary key", entry_path + "[0]")
				return null
			result[key] = item
	return result


func decode_components(payload: Variant, kind: int, location: String) -> Variant:
	var contract := variant_contract(kind)
	if not payload is Array or payload.size() != contract.length:
		fail("validate", "Expected %d tagged components" % contract.length, location)
		return null
	var components := []
	var integer_components := kind in [TYPE_VECTOR2I, TYPE_VECTOR3I, TYPE_VECTOR4I, TYPE_RECT2I]
	for index in payload.size():
		var component: Variant = payload[index]
		var component_path := location + ".$variant.value[%d]" % index
		if typeof(component) not in [TYPE_INT, TYPE_FLOAT] or not is_finite(float(component)):
			fail("validate", "Expected a finite numeric component", component_path)
			return null
		if integer_components:
			if component < -2147483648 or component > 2147483647 or component != floor(component):
				fail("validate", "Expected a signed 32-bit integer component", component_path)
				return null
			component = int(component)
		components.append(component)
	var result: Variant
	match kind:
		TYPE_VECTOR2: result = Vector2(components[0], components[1])
		TYPE_VECTOR3: result = Vector3(components[0], components[1], components[2])
		TYPE_VECTOR4: result = Vector4(components[0], components[1], components[2], components[3])
		TYPE_VECTOR2I: result = Vector2i(components[0], components[1])
		TYPE_VECTOR3I: result = Vector3i(components[0], components[1], components[2])
		TYPE_VECTOR4I: result = Vector4i(components[0], components[1], components[2], components[3])
		TYPE_COLOR: result = Color(components[0], components[1], components[2], components[3])
		TYPE_RECT2: result = Rect2(components[0], components[1], components[2], components[3])
		TYPE_RECT2I: result = Rect2i(components[0], components[1], components[2], components[3])
		TYPE_TRANSFORM2D: result = Transform2D(Vector2(components[0], components[1]), Vector2(components[2], components[3]), Vector2(components[4], components[5]))
		TYPE_BASIS: result = Basis(Vector3(components[0], components[1], components[2]), Vector3(components[3], components[4], components[5]), Vector3(components[6], components[7], components[8]))
		TYPE_TRANSFORM3D: result = Transform3D(Basis(Vector3(components[0], components[1], components[2]), Vector3(components[3], components[4], components[5]), Vector3(components[6], components[7], components[8])), Vector3(components[9], components[10], components[11]))
		TYPE_QUATERNION: result = Quaternion(components[0], components[1], components[2], components[3])
		TYPE_PLANE: result = Plane(Vector3(components[0], components[1], components[2]), components[3])
		TYPE_AABB: result = AABB(Vector3(components[0], components[1], components[2]), Vector3(components[3], components[4], components[5]))
	var observed := component_values(result)
	for index in components.size():
		if observed[index] != components[index]:
			fail("validate", "Component cannot be represented exactly by the engine", location + ".$variant.value[%d]" % index)
			return null
	return result


func default_value(value: Variant) -> Dictionary:
	var kind := typeof(value)
	if kind in [TYPE_NIL, TYPE_OBJECT] and value == null:
		return { "encoding": "json", "value": null }
	if kind in [TYPE_NIL, TYPE_BOOL, TYPE_STRING] or (kind == TYPE_INT and value >= -9007199254740991 and value <= 9007199254740991) or (kind == TYPE_FLOAT and is_finite(value)):
		return { "encoding": "json", "value": value }
	if kind == TYPE_ARRAY and (not value.is_typed() or not scalar_kind(value.get_typed_builtin())):
		return { "encoding": "godot", "value": var_to_str(value) }
	if kind == TYPE_DICTIONARY and (not scalar_kind(value.get_typed_key_builtin()) or not scalar_kind(value.get_typed_value_builtin())):
		return { "encoding": "godot", "value": var_to_str(value) }
	if kind == TYPE_ARRAY or PACKED_ELEMENTS.has(kind):
		for item: Variant in value:
			if default_value(item).encoding == "godot": return { "encoding": "godot", "value": var_to_str(value) }
	elif kind == TYPE_DICTIONARY:
		for key: Variant in value:
			if default_value(key).encoding == "godot" or default_value(value[key]).encoding == "godot": return { "encoding": "godot", "value": var_to_str(value) }
	if not variant_contract(kind).is_empty():
		if VARIANT_COMPONENTS.has(kind):
			for component: Variant in component_values(value):
				if not is_finite(float(component)):
					return { "encoding": "godot", "value": var_to_str(value) }
		return { "encoding": "tagged", "value": encode_variant(value) }
	if value is Resource:
		var script := value.get_script() as Script
		return { "encoding": "resource", "value": { "path": value.resource_path, "type": value.get_class(), "script": script.resource_path if script != null else null } }
	return { "encoding": "godot", "value": var_to_str(value) }


func enum_choices(property: Dictionary) -> Array:
	var choices := []
	if int(property.hint) not in [PROPERTY_HINT_ENUM, PROPERTY_HINT_ENUM_SUGGESTION]:
		return choices
	var enum_value := 0
	for option: String in str(property.hint_string).split(",", int(property.type) != TYPE_STRING):
		if int(property.type) == TYPE_STRING:
			choices.append({ "name": option, "value": option })
		elif int(property.type) == TYPE_INT:
			var parts := option.split(":")
			if parts.size() > 1:
				enum_value = parts[1].to_int()
			choices.append({ "name": parts[0], "value": encode_variant(enum_value) if enum_value < -9007199254740991 or enum_value > 9007199254740991 else enum_value })
			enum_value += 1
	return choices


func accepted_inputs(property: Dictionary, reason: String) -> Array:
	if not reason.is_empty(): return []
	match int(property.type):
		TYPE_OBJECT: return ["null", "$ref", "$resource"]
		TYPE_ARRAY: return ["$variant"] if scalar_kind(int(property.element.type)) else ["array"]
		TYPE_INT: return ["scalar", "$variant"]
	if not variant_contract(int(property.type)).is_empty(): return ["$variant"]
	return ["scalar"]


func schema(resource: Resource, spec: Dictionary, metadata: Dictionary) -> void:
	var fields := []
	for field: String in metadata:
		var property: Dictionary = metadata[field]
		var reason := unsupported_reason(property)
		fields.append({
			"name": field,
			"type": type_string(int(property.type)),
			"type_id": int(property.type),
			"class_name": str(property.class_name),
			"default": default_value(resource.get(field)),
			"hint": int(property.hint),
			"hint_string": str(property.hint_string),
			"enum_choices": enum_choices(property),
			"usage": int(property.usage),
			"storage": int(property.usage) & PROPERTY_USAGE_STORAGE != 0,
			"read_only": int(property.usage) & PROPERTY_USAGE_READ_ONLY != 0,
			"editor_visible": int(property.usage) & PROPERTY_USAGE_EDITOR != 0,
			"create_supported": reason.is_empty(),
			"unsupported_reason": reason,
			"variant_contract": container_contract(property),
			"resource_constraints": resource_constraints(property),
			"element_constraints": resource_constraints(property.element) if property.has("element") else [],
			"element_accepted_inputs": ["null", "$ref", "$resource"] if reason.is_empty() and int(property.type) == TYPE_ARRAY and int(property.element.type) == TYPE_OBJECT else [],
			"accepted_inputs": accepted_inputs(property, reason),
		})
	print(PREFIX + JSON.stringify({ "status": "schema", "type": resource.get_class(), "script": spec.get("script"), "executes_constructors_and_getters": true, "fields": fields, "integer_min": -9007199254740991, "integer_max": 9007199254740991, "max_resource_depth": MAX_DEPTH, "variant_codec_version": 1 }, "", true, true))
	quit()


func field_path(prefix: String, field: String) -> String:
	return field if prefix.is_empty() else prefix + ".properties." + field


func snapshot(value: Variant, path: String, ancestors: Array = [], depth: int = 0) -> Variant:
	if typeof(value) in [TYPE_NIL, TYPE_OBJECT] and value == null:
		return { "kind": "null" }
	if depth > MAX_DEPTH and (value is Resource or value is Array or value is Dictionary):
		fail("validate", "Serialized graph nesting exceeds limit of 16", path)
		return null
	if value is Script:
		return { "kind": "script", "path": value.resource_path }
	if value is Resource:
		var identity: int = value.get_instance_id()
		if identity in ancestors:
			fail("validate", "Cyclic Resource graph is unsupported", path)
			return null
		var chain := ancestors.duplicate()
		chain.append(identity)
		var fields := {}
		for property: Dictionary in value.get_property_list():
			if int(property.usage) & PROPERTY_USAGE_STORAGE != 0 and property.name != "script":
				fields[property.name] = snapshot(value.get(property.name), field_path(path, property.name), chain, depth + 1)
				if failed: return null
		var script := value.get_script() as Script
		return { "kind": "resource", "type": value.get_class(), "script": script.resource_path if script != null else null, "properties": fields }
	if value is Array:
		var items := []
		for index in value.size():
			items.append(snapshot(value[index], path + "[%d]" % index, ancestors, depth + 1))
			if failed: return null
		var typed_script := value.get_typed_script() as Script
		return { "kind": "array", "builtin": value.get_typed_builtin(), "class": str(value.get_typed_class_name()), "script": typed_script.resource_path if typed_script != null else null, "items": items }
	if value is Dictionary:
		var items := {}
		for key: Variant in value:
			if typeof(key) in [TYPE_OBJECT, TYPE_ARRAY, TYPE_DICTIONARY]:
				fail("validate", "Compound serialized dictionary keys are unsupported", path)
				return null
			items[key] = snapshot(value[key], path + "[%s]" % var_to_str(key), ancestors, depth + 1)
			if failed: return null
		return { "kind": "dictionary", "key_builtin": value.get_typed_key_builtin(), "value_builtin": value.get_typed_value_builtin(), "key_class": str(value.get_typed_key_class_name()), "value_class": str(value.get_typed_value_class_name()), "key_script": value.get_typed_key_script().resource_path if value.get_typed_key_script() != null else null, "value_script": value.get_typed_value_script().resource_path if value.get_typed_value_script() != null else null, "items": items }
	if PACKED_ELEMENTS.has(typeof(value)):
		var items := []
		for index in value.size(): items.append(snapshot(value[index], path + "[%d]" % index, ancestors, depth + 1))
		return { "kind": typeof(value), "items": items }
	if typeof(value) == TYPE_OBJECT:
		fail("validate", "Serialized non-Resource objects are unsupported", path)
		return null
	return { "kind": typeof(value), "value": value }


func compare_snapshot(actual: Variant, expected: Variant, path: String, stage: String) -> bool:
	if actual == expected: return true
	if actual is Dictionary and expected is Dictionary and str(actual.get("kind")) == str(expected.get("kind")):
		var key := "properties" if str(expected.get("kind")) == "resource" else "items"
		if expected.has(key) and actual.has(key):
			var left: Variant = actual[key]
			var right: Variant = expected[key]
			if left is Dictionary and right is Dictionary and left.size() == right.size():
				for field: Variant in right:
					var location := field_path(path, str(field)) if str(expected.kind) == "resource" else path + "[%s]" % var_to_str(field)
					if left.has(field) and not compare_snapshot(left[field], right[field], location, stage): return false
			elif left is Array and right is Array and left.size() == right.size():
				for index in right.size():
					if not compare_snapshot(left[index], right[index], path + "[%d]" % index, stage): return false
	fail(stage, "Serialized value differs from requested graph", path)
	return false


func check_requested(resource: Resource, expected: Dictionary, path: String, stage: String, identity: bool) -> bool:
	for field: String in expected:
		var descriptor: Dictionary = expected[field]
		var actual: Variant = resource.get(field)
		var location := field_path(path, field)
		if not check_value(actual, descriptor, "properties." + location if path.is_empty() and descriptor.kind != "scalar" else location, stage, identity): return false
	return true


func check_value(actual: Variant, descriptor: Dictionary, location: String, stage: String, identity: bool) -> bool:
	if descriptor.kind == "array":
		if not actual is Array or actual.size() != descriptor.entries.size():
			fail(stage, "Array assignment changed requested entries", location)
			return false
		for index in descriptor.entries.size():
			if not check_value(actual[index], descriptor.entries[index], location + "[%d]" % index, stage, identity): return false
		return compare_snapshot(snapshot(actual, location), descriptor.snapshot, location, stage)
	if descriptor.kind == "container":
		return compare_snapshot(snapshot(actual, location), descriptor.snapshot, location, stage)
	if descriptor.kind == "scalar":
		if descriptor.value == null and actual == null: return true
		if typeof(actual) != typeof(descriptor.value) or actual != descriptor.value:
			fail(stage, "Value differs from requested value", location)
			return false
	else:
		if not actual is Resource or (identity and actual != descriptor.value):
			fail(stage, "Resource assignment changed requested object or rejected its script type", location)
			return false
		if descriptor.kind == "ref" and actual.resource_path != descriptor.path:
			fail(stage, "External Resource reference changed path", location)
			return false
		var observed: Variant = snapshot(actual, location)
		if failed or not compare_snapshot(observed, descriptor.snapshot, location, stage): return false
		if descriptor.kind == "inline" and not check_requested(actual, descriptor.expected, location, stage, identity): return false
	return true


func resource_input(value: Variant, property: Dictionary, location: String, depth: int, error_location: String = "") -> Dictionary:
	var descriptor := { "kind": "scalar", "value": value }
	if value == null: return descriptor
	if error_location.is_empty(): error_location = location
	if not value is Dictionary or value.size() != 1 or (not value.has("$ref") and not value.has("$resource")):
		fail("validate", "Expected null, $ref, or $resource", error_location)
		return {}
	var nested_path := location
	if value.has("$ref"):
		var reference := ResourceLoader.load(str(value["$ref"]), "", ResourceLoader.CACHE_MODE_IGNORE_DEEP)
		if reference == null:
			fail("load", "Could not load referenced Resource", error_location)
			return {}
		descriptor = { "kind": "ref", "value": reference, "path": reference.resource_path, "snapshot": snapshot(reference, nested_path) }
	else:
		var child := build(value["$resource"], nested_path, depth + 1)
		if failed: return {}
		descriptor = { "kind": "inline", "value": child.resource, "expected": child.expected, "snapshot": snapshot(child.resource, nested_path) }
	if failed: return {}
	value = descriptor.value
	if not accepts_resource(property, value):
		fail("validate", "Resource does not satisfy declared class or script constraints: %s" % resource_constraints(property), error_location)
		return {}
	return descriptor


func build(spec: Dictionary, path: String = "", depth: int = 0) -> Dictionary:
	if depth > MAX_DEPTH:
		fail("validate", "Resource nesting exceeds limit of 16", path)
		return {}
	var resource := construct(spec, path)
	if failed or resource == null: return {}
	var metadata := property_metadata(resource)
	var expected := {}
	for field: String in spec.properties:
		var location := field_path(path, field)
		if not metadata.has(field):
			fail("validate", "Unknown property", location)
			return {}
		var property: Dictionary = metadata[field]
		var reason := unsupported_reason(property)
		if not reason.is_empty():
			fail("validate", reason, location)
			return {}
		var value: Variant = spec.properties[field]
		var kind := int(property.type)
		var descriptor := { "kind": "scalar", "value": value }
		if kind == TYPE_DICTIONARY or PACKED_ELEMENTS.has(kind) or (kind == TYPE_ARRAY and scalar_kind(int(property.element.type))):
			if not value is Dictionary:
				fail("validate", "Expected an explicit $variant container", location)
				return {}
			value = decode_variant(value, kind, location, property)
			if failed: return {}
			descriptor = { "kind": "container", "value": value, "snapshot": snapshot(value, location) }
		elif kind == TYPE_ARRAY:
			if not value is Array:
				fail("validate", "Expected an array of Resources", location)
				return {}
			var entries := []
			var array_path := "properties." + location if path.is_empty() else location
			var template: Array = resource.get(field)
			var typed := Array([], template.get_typed_builtin(), template.get_typed_class_name(), template.get_typed_script())
			for index in value.size():
				var entry := resource_input(value[index], property.element, array_path + "[%d]" % index, depth)
				if failed: return {}
				entries.append(entry)
				typed.append(entry.value)
			descriptor = { "kind": "array", "value": typed, "entries": entries, "snapshot": snapshot(typed, array_path) }
		elif kind == TYPE_OBJECT and value != null:
			descriptor = resource_input(value, property, "properties." + location if path.is_empty() else location, depth, location)
			if failed: return {}
			value = descriptor.value
		elif kind != TYPE_OBJECT:
			if value is Dictionary:
				value = decode_variant(value, kind, location)
				if failed: return {}
			elif kind != TYPE_INT and not variant_contract(kind).is_empty():
				fail("validate", "Expected an explicit $variant tag", location)
				return {}
			if kind == TYPE_INT and value is float and is_finite(value) and value == floor(value) and abs(value) <= 9007199254740991.0:
				value = int(value)
			if typeof(value) != kind:
				fail("validate", "Expected %s; received %s" % [type_string(kind), type_string(typeof(value))], location)
				return {}
			descriptor.value = value
		expected[field] = descriptor
	for field: String in expected:
		resource.set(field, expected[field].value)
		if expected[field].kind in ["scalar", "container"]:
			var requested: Variant = spec.properties[field]
			spec.properties[field] = encode_variant(expected[field].value) if requested is Dictionary and requested.has("$variant") else expected[field].value
	if not check_requested(resource, expected, path, "assign", true): return {}
	return { "resource": resource, "expected": expected }


func _initialize() -> void:
	var arguments := OS.get_cmdline_user_args()
	var spec: Dictionary = JSON.parse_string(FileAccess.get_file_as_string(arguments[0]))
	if arguments[1] == "schema":
		var instance := construct(spec)
		if failed or instance == null: return
		schema(instance, spec, property_metadata(instance))
		return
	var graph := build(spec)
	if failed: return
	var resource: Resource = graph.resource
	var baseline: Variant = snapshot(resource, "")
	if failed: return
	var error := ResourceSaver.save(resource, arguments[2])
	if error != OK:
		fail("save", error_string(error))
		return
	var saved_observation: Variant = snapshot(resource, "")
	if failed or not compare_snapshot(saved_observation, baseline, "", "save"): return
	var reloaded := ResourceLoader.load(arguments[2], "", ResourceLoader.CACHE_MODE_IGNORE_DEEP) as Resource
	if reloaded == null:
		fail("verify", "Saved Resource did not reload")
		return
	if not check_requested(reloaded, graph.expected, "", "verify", false): return
	var reloaded_observation: Variant = snapshot(reloaded, "")
	if failed or not compare_snapshot(reloaded_observation, baseline, "", "verify"): return
	print(PREFIX + JSON.stringify({ "status": "created", "type": resource.get_class(), "script": spec.get("script"), "properties": spec.properties }, "", true, true))
	quit()
