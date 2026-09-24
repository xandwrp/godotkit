extends SceneTree

var method_argument_names := true
var method_default_values := true


func _type(info: Dictionary) -> Dictionary:
	return {
		"type": int(info.get("type", TYPE_NIL)),
		"class_name": str(info.get("class_name", "")),
		"usage": int(info.get("usage", 0)),
	}


func _arguments(values: Array) -> Array:
	var result: Array = []
	for value: Dictionary in values:
		if str(value.get("name", "")).is_empty():
			method_argument_names = false
		result.append({
			"name": str(value.get("name", "")),
			"type": _type(value),
		})
	return result


func _methods(class_id: StringName) -> Array:
	var result: Array = []
	for method: Dictionary in ClassDB.class_get_method_list(class_id, true):
		if not method.has("default_args"):
			method_default_values = false
		var defaults: Array = []
		for value: Variant in method.get("default_args", []):
			defaults.append(var_to_str(value))
		result.append({
			"name": str(method.get("name", "")),
			"return_type": _type(method.get("return", {})),
			"arguments": _arguments(method.get("args", [])),
			"defaults": defaults,
			"flags": int(method.get("flags", 0)),
		})
	return result


func _properties(class_id: StringName) -> Array:
	var result: Array = []
	for property: Dictionary in ClassDB.class_get_property_list(class_id, true):
		if int(property.get("usage", 0)) & (PROPERTY_USAGE_GROUP | PROPERTY_USAGE_SUBGROUP | PROPERTY_USAGE_CATEGORY) != 0:
			continue
		result.append({
			"name": str(property.get("name", "")),
			"type": _type(property),
		})
	return result


func _signals(class_id: StringName) -> Array:
	var result: Array = []
	for signal_info: Dictionary in ClassDB.class_get_signal_list(class_id, true):
		result.append({
			"name": str(signal_info.get("name", "")),
			"arguments": _arguments(signal_info.get("args", [])),
		})
	return result


func _enums(class_id: StringName) -> Array:
	var result: Array = []
	for enum_name: StringName in ClassDB.class_get_enum_list(class_id, true):
		var values: Array = []
		for constant_name: StringName in ClassDB.class_get_enum_constants(class_id, enum_name, true):
			values.append({
				"name": str(constant_name),
				"value": ClassDB.class_get_integer_constant(class_id, constant_name),
			})
		result.append({ "name": str(enum_name), "values": values })
	return result


func _constants(class_id: StringName) -> Array:
	var result: Array = []
	for constant_name: StringName in ClassDB.class_get_integer_constant_list(class_id, true):
		if ClassDB.class_get_integer_constant_enum(class_id, constant_name, true).is_empty():
			result.append({
				"name": str(constant_name),
				"value": ClassDB.class_get_integer_constant(class_id, constant_name),
			})
	return result


func _initialize() -> void:
	var classes: Array = []
	for class_id: StringName in ClassDB.get_class_list():
		classes.append({
			"name": str(class_id),
			"parent": str(ClassDB.get_parent_class(class_id)),
			"methods": _methods(class_id),
			"properties": _properties(class_id),
			"signals": _signals(class_id),
			"enums": _enums(class_id),
			"constants": _constants(class_id),
		})
	var version := Engine.get_version_info()
	print("GDKIT_API_RESULT:" + JSON.stringify({
		"version": str(version.get("string", "unknown")),
		"classes": classes,
		"method_argument_names": method_argument_names,
		"method_default_values": method_default_values,
	}))
	quit()
