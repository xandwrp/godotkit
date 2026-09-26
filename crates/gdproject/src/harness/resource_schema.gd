## User args: "class" <Name> | "script" <res path>.
## Instantiates the target and reports every property-list entry raw, with the
## fresh instance's value encoded; gdview::property derives the field schema.
## Payload: {"class": native class, "script_class": global name or null,
## "properties": [{name, type, class_name, hint, hint_string, usage, default,
## default_error}]}.
extends SceneTree
const Protocol := preload("protocol.gd")
const HARNESS := "resource_schema"


func _initialize() -> void:
	var args := OS.get_cmdline_user_args()
	if args.size() != 2 or args[0] not in ["class", "script"]:
		_fail("arguments", "Expected `class <Name>` or `script <res://path>`", "", 2)
		return
	var instance: Resource = null
	var script_class: Variant = null
	if args[0] == "class":
		instance = _instantiate_class(args[1])
	else:
		var script := _load_script(args[1])
		if script != null:
			instance = script.new() as Resource
			var global_name := script.get_global_name()
			if global_name:
				script_class = String(global_name)
	if instance == null:
		return
	var properties: Array = []
	for property in instance.get_property_list():
		var entry := {
			"name": property.name,
			"type": property.type,
			"class_name": String(property["class_name"]),
			"hint": property.hint,
			"hint_string": property.hint_string,
			"usage": property.usage,
			"default": null,
			"default_error": null,
		}
		if property.usage & PROPERTY_USAGE_STORAGE and property.name != "script":
			var encoded := Protocol.try_encode(instance.get(property.name), true)
			if encoded.ok:
				entry.default = encoded.value
			else:
				entry.default_error = encoded.error
		properties.append(entry)
	Protocol.emit_ok(
		HARNESS,
		{"class": instance.get_class(), "script_class": script_class, "properties": properties}
	)
	quit(0)


func _instantiate_class(name: String) -> Resource:
	if not ClassDB.class_exists(name):
		for global_class in ProjectSettings.get_global_class_list():
			if global_class["class"] == name:
				_fail(
					"target",
					"%s is a script class; use --script %s" % [name, global_class.path],
					"class"
				)
				return null
		_fail("target", "Unknown class %s" % name, "class")
		return null
	if not ClassDB.is_parent_class(name, "Resource"):
		_fail("target", "%s is not a Resource" % name, "class")
		return null
	if not ClassDB.can_instantiate(name):
		_fail("target", "%s is abstract and cannot be instantiated" % name, "class")
		return null
	return ClassDB.instantiate(name) as Resource


func _load_script(path: String) -> Script:
	if not Protocol.transportable_res_path(path):
		_fail("target", "Script must be a saved res:// path: %s" % path, "script")
		return null
	if not ResourceLoader.exists(path):
		_fail("target", "No script at %s" % path, "script")
		return null
	var script := ResourceLoader.load(path) as Script
	if script == null:
		_fail("target", "%s is not a script" % path, "script")
		return null
	if not script.can_instantiate():
		_fail("target", "%s cannot be instantiated (abstract, or it failed to compile)" % path, "script")
		return null
	if not ClassDB.is_parent_class(script.get_instance_base_type(), "Resource"):
		_fail(
			"target",
			"%s extends %s, not Resource" % [path, script.get_instance_base_type()],
			"script"
		)
		return null
	return script


func _fail(stage: String, message: String, field: String = "", code: int = 1) -> void:
	Protocol.emit_error(HARNESS, stage, message, field)
	quit(code)
