## User args: <spec json path> <staged res:// path>.
## Builds the resource graph from the spec, reading each value as its declared
## property type: typed arrays and dictionaries take the typing of the fresh
## instance's value, ints widen to floats, strings become StringName/NodePath.
## Each assignment is read back, the root is saved to the staged path and
## reloaded with CACHE_MODE_IGNORE, and every spec'd property is compared with
## what was assigned and echoed back encoded. Any divergence -> error envelope
## with stage "arguments"|"spec"|"target"|"assign"|"save"|"reload"|"verify" and
## the field path (`properties.ammo.properties.count`, `properties.tags[2]`).
## Nested `$resource` specs are built here, not by Protocol.decode, so they get
## the same typing and checks; every other leaf goes through Protocol.
## Payload: {"echo": {field: encoded}}. gdproject::resource re-verifies it.
extends SceneTree
const Protocol := preload("protocol.gd")
const HARNESS := "resource_create"

var failed := false


func _initialize() -> void:
	var args := OS.get_cmdline_user_args()
	if args.size() != 2:
		_fail("arguments", "Expected spec path and staged res:// path", "", 2)
		return
	var text := FileAccess.get_file_as_string(args[0])
	var spec: Variant = JSON.parse_string(text) if text else null
	if not spec is Dictionary:
		_fail("spec", "Cannot read the spec as a JSON object", args[0])
		return
	var staged: String = args[1]
	var root := _build(spec, "")
	if failed:
		return
	var error := ResourceSaver.save(root, staged)
	if error != OK:
		_fail("save", "Cannot save %s: %s" % [staged, error_string(error)], "")
		return
	var reloaded := ResourceLoader.load(staged, "", ResourceLoader.CACHE_MODE_IGNORE) as Resource
	if reloaded == null:
		_fail("reload", "Cannot load the saved resource back from %s" % staged, "")
		return
	var echo := {}
	for name in spec.get("properties", {}):
		var field := "properties.%s" % name
		var assigned: Variant = root.get(name)
		var stored: Variant = reloaded.get(name)
		if not _same(assigned, stored):
			var message := (
				"Reloading changed the value: assigned %s, reloaded %s"
				% [_show(assigned), _show(stored)]
			)
			var equal: bool = typeof(assigned) == typeof(stored) and assigned == stored
			if equal or _show(assigned) == _show(stored):
				# Godot 4.7.2 writes -0.0 as 0 in .tres text, in every float slot.
				message += " (they differ in bits .tres text does not keep, such as the sign of -0.0)"
			_fail("verify", message, field)
			return
		var encoded := Protocol.try_encode(stored, true)
		if not encoded.ok:
			_fail("verify", encoded.error, field)
			return
		echo[name] = encoded.value
	Protocol.emit_ok(HARNESS, {"echo": echo})
	quit(0)


## Instantiates a `$resource`-shaped spec and assigns its properties.
## `at` is the field path of this resource ("" for the root).
func _build(spec: Dictionary, at: String) -> Resource:
	var resource := _instantiate(spec, at)
	if resource == null:
		return null
	var properties: Variant = spec.get("properties", {})
	if not properties is Dictionary:
		_fail("spec", "Resource properties must be an object", at)
		return null
	var infos := {}
	for info in resource.get_property_list():
		if info.usage & PROPERTY_USAGE_STORAGE:
			infos[info.name] = info
	for name in properties:
		var field := _join(at, "properties.%s" % name)
		if name == "script" or not infos.has(name):
			_fail("assign", "%s has no stored property `%s`" % [_describe(resource), name], field)
			return null
		var value: Variant = _read(properties[name], _declared(resource.get(name), infos[name]), field)
		if failed:
			return null
		resource.set(name, value)
		var stored: Variant = resource.get(name)
		if not _same(value, stored):
			_fail(
				"assign",
				(
					"The property kept %s after assigning %s (a setter or type rejected it)"
					% [_show(stored), _show(value)]
				),
				field
			)
			return null
	return resource


func _instantiate(spec: Dictionary, at: String) -> Resource:
	for key in spec:
		if key not in ["class", "script", "properties"]:
			_fail("spec", "Unknown resource spec field %s" % key, at)
			return null
	if spec.has("class") == spec.has("script"):
		_fail("spec", "Resource requires exactly one of class or script", at)
		return null
	if spec.has("class"):
		var class_field := _join(at, "class")
		var name: Variant = spec["class"]
		if not name is String or not ClassDB.class_exists(name):
			_fail("target", "Unknown class %s" % [name], class_field)
			return null
		if not ClassDB.is_parent_class(name, "Resource"):
			_fail("target", "%s is not a Resource" % name, class_field)
			return null
		if not ClassDB.can_instantiate(name):
			_fail("target", "%s is abstract and cannot be instantiated" % name, class_field)
			return null
		return ClassDB.instantiate(name) as Resource
	var field := _join(at, "script")
	var path: Variant = spec["script"]
	if (
		not path is String
		or not Protocol.transportable_res_path(path)
		or not ResourceLoader.exists(path)
	):
		_fail("target", "No script at %s" % [path], field)
		return null
	var script := ResourceLoader.load(path) as Script
	if script == null or not script.can_instantiate():
		_fail("target", "%s cannot be instantiated (abstract, or it failed to compile)" % path, field)
		return null
	if not ClassDB.is_parent_class(script.get_instance_base_type(), "Resource"):
		_fail(
			"target", "%s extends %s, not Resource" % [path, script.get_instance_base_type()], field
		)
		return null
	return script.new() as Resource


## What a value must become, {type, object_class, script} plus `element` for a
## typed Array, `key`/`value` for a typed Dictionary and `enum` for an enum int.
## Container typing comes from the fresh instance's current value.
func _declared(current: Variant, info: Dictionary) -> Dictionary:
	var object_class: StringName = info["class_name"] if info.type == TYPE_OBJECT else &""
	if info.type == TYPE_OBJECT and not object_class and info.hint == PROPERTY_HINT_RESOURCE_TYPE:
		object_class = StringName(info.hint_string)
	var declared := _type(info.type, object_class, null)
	if info.hint == PROPERTY_HINT_ENUM and info.type == TYPE_INT:
		declared["enum"] = info.hint_string
	if current is Array and current.is_typed():
		declared["element"] = _type(
			current.get_typed_builtin(), current.get_typed_class_name(), current.get_typed_script()
		)
	if current is Dictionary and current.is_typed():
		declared["key"] = _type(
			current.get_typed_key_builtin(),
			current.get_typed_key_class_name(),
			current.get_typed_key_script()
		)
		declared["value"] = _type(
			current.get_typed_value_builtin(),
			current.get_typed_value_class_name(),
			current.get_typed_value_script()
		)
	return declared


func _type(type: int, object_class: StringName, script: Variant) -> Dictionary:
	return {"type": type, "object_class": object_class, "script": script}


const ANY := {"type": TYPE_NIL, "object_class": &"", "script": null}


## Decodes `json` and reads it as `declared` (TYPE_NIL: any Variant).
func _read(json: Variant, declared: Dictionary, at: String) -> Variant:
	var value: Variant = _decode(json, declared, at)
	if failed:
		return null
	return _coerce(value, declared, at)


## JSON -> Variant. Nested resources are built here; arrays and dictionaries
## read their items as the declaration types them.
func _decode(json: Variant, declared: Dictionary, at: String) -> Variant:
	if json is Array:
		var element: Dictionary = declared.get("element", ANY)
		var items: Array = []
		for index in json.size():
			var item: Variant = _read(json[index], element, "%s[%d]" % [at, index])
			if failed:
				return null
			items.append(item)
		return items
	if not json is Dictionary:
		return _leaf(json, at)
	if json.has("$resource"):
		if json.size() != 1 or not json["$resource"] is Dictionary:
			return _fail("spec", "Invalid resource spec", at)
		return _build(json["$resource"], at)
	if json.has("$ref"):
		return _leaf(json, at)
	if json.has("$variant"):
		var tag: Variant = json["$variant"]
		if tag is Dictionary and tag.get("type") == "Dictionary" and tag.get("value") is Array:
			return _pairs(tag.value, declared, at)
		var tagged: Variant = _leaf(json, at)
		var element: Dictionary = declared.get("element", ANY)
		if (
			not failed
			and tagged is Array
			and element.type != TYPE_NIL
			and tagged.get_typed_builtin() != element.type
		):
			return _fail(
				"assign",
				(
					"Expected Array[%s], got Array[%s]"
					% [_type_name(element), type_string(tagged.get_typed_builtin())]
				),
				at
			)
		return tagged
	var pairs: Array = []
	for key in json:
		pairs.append([key, json[key]])
	return _pairs(pairs, declared, at, true)


## `[[key, value]…]` into a Dictionary. `plain` keys are JSON object keys:
## strings, never decoded as JSON values.
func _pairs(pairs: Array, declared: Dictionary, at: String, plain := false) -> Variant:
	var result := {}
	var key_type: Dictionary = declared.get("key", ANY)
	var value_type: Dictionary = declared.get("value", ANY)
	for pair in pairs:
		if not pair is Array or pair.size() != 2:
			return _fail("spec", "Dictionary value must be an array of [key, value] pairs", at)
		var location := "%s[%s]" % [at, JSON.stringify(pair[0])]
		var key: Variant = (
			_coerce(pair[0], key_type, location) if plain else _read(pair[0], key_type, location)
		)
		if failed:
			return null
		if result.has(key):
			return _fail("spec", "Duplicate dictionary key", location)
		result[key] = _read(pair[1], value_type, location)
		if failed:
			return null
	return result


func _leaf(json: Variant, at: String) -> Variant:
	var decoded := Protocol.try_decode(json)
	if not decoded.ok:
		return _fail("assign", decoded.error, at)
	return decoded.value


## The lossless readings of a decoded value as its declared type; a value of
## any other type fails here, before set() could drop it silently.
func _coerce(value: Variant, declared: Dictionary, at: String) -> Variant:
	var type: int = declared.type
	if type == TYPE_NIL:
		return value
	if type == TYPE_FLOAT and value is int:
		return float(value)
	if type == TYPE_STRING_NAME and value is String:
		return StringName(value)
	if type == TYPE_NODE_PATH and value is String:
		return NodePath(value)
	if type == TYPE_OBJECT:
		if value == null:
			return null
		if not value is Object:
			return _fail(
				"assign",
				"Expected %s, got %s" % [_type_name(declared), type_string(typeof(value))],
				at
			)
		if not _is_a(value, declared):
			return _fail("assign", "Expected %s, got %s" % [_type_name(declared), _describe(value)], at)
		return value
	if typeof(value) != type:
		var message := "Expected %s, got %s" % [_type_name(declared), type_string(typeof(value))]
		if declared.has("enum") and value is String:
			message += "; write the enum's value (%s), not its name" % declared.enum
		return _fail("assign", message, at)
	if type == TYPE_ARRAY and declared.has("element"):
		var element: Dictionary = declared.element
		return Array(value, element.type, element.object_class, element.script)
	if type == TYPE_DICTIONARY and declared.has("key"):
		var key: Dictionary = declared.key
		var item: Dictionary = declared.value
		return Dictionary(
			value, key.type, key.object_class, key.script, item.type, item.object_class, item.script
		)
	return value


func _is_a(value: Object, declared: Dictionary) -> bool:
	var script: Variant = declared.script
	var name: StringName = declared.object_class
	if script == null and name and not ClassDB.class_exists(name):
		for global_class in ProjectSettings.get_global_class_list():
			if global_class["class"] == name:
				script = load(global_class.path)
	if script != null:
		var own: Script = value.get_script()
		while own != null:
			if own == script:
				return true
			own = own.get_base_script()
		return false
	return not name or value.is_class(name)


## Deep equality that sees through reloading: external resources by path,
## inline ones by class, script and stored properties; containers by typing
## and items; everything else by value or bits.
func _same(a: Variant, b: Variant) -> bool:
	if typeof(a) != typeof(b):
		return false
	match typeof(a):
		TYPE_OBJECT:
			if is_same(a, b):
				return true
			if not (a is Resource and b is Resource):
				return false
			if (
				Protocol.transportable_res_path(a.resource_path)
				or Protocol.transportable_res_path(b.resource_path)
			):
				return a.resource_path == b.resource_path
			if a.get_class() != b.get_class() or _script_path(a) != _script_path(b):
				return false
			for info in a.get_property_list():
				if info.usage & PROPERTY_USAGE_STORAGE and info.name not in ["script", "resource_path"]:
					if not _same(a.get(info.name), b.get(info.name)):
						return false
			return true
		TYPE_ARRAY:
			if a.size() != b.size() or not a.is_same_typed(b):
				return false
			for index in a.size():
				if not _same(a[index], b[index]):
					return false
			return true
		TYPE_DICTIONARY:
			if a.size() != b.size() or not a.is_same_typed(b):
				return false
			for key in a:
				if not b.has(key) or not _same(a[key], b[key]):
					return false
			return true
	# Bit-exact: NaN equals itself (Vector2(0, nan)) and -0.0 is not 0.0.
	return var_to_bytes(a) == var_to_bytes(b)


func _script_path(resource: Resource) -> String:
	var script: Script = resource.get_script()
	return script.resource_path if script != null else ""


func _type_name(declared: Dictionary) -> String:
	if declared.type == TYPE_OBJECT and declared.object_class:
		if declared.script != null and declared.script.get_global_name():
			return declared.script.get_global_name()
		return declared.object_class
	return type_string(declared.type)


func _describe(value: Object) -> String:
	var script: Script = value.get_script()
	if script != null:
		var name := script.get_global_name()
		return "%s (%s)" % [name, script.resource_path] if name else script.resource_path
	return value.get_class()


func _show(value: Variant) -> String:
	if value == null:
		return "null"
	if value is Resource and value.resource_path:
		return value.resource_path
	if value is Object:
		return _describe(value)
	return var_to_str(value)


func _join(at: String, field: String) -> String:
	return "%s.%s" % [at, field] if at else field


func _fail(stage: String, message: String, field: String, code: int = 1) -> Variant:
	if not failed:
		failed = true
		Protocol.emit_error(HARNESS, stage, message, field)
		quit(code)
	return null
