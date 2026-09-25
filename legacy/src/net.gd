extends SceneTree

const RESULT_PREFIX := "GDKIT_NET_RESULT:"


func _initialize() -> void:
	var arguments := OS.get_cmdline_user_args()
	if arguments.size() != 1:
		printerr("gdkit net reflection requires one script manifest")
		quit(2)
		return
	var parsed: Variant = JSON.parse_string(FileAccess.get_file_as_string(arguments[0]))
	if not parsed is Array:
		printerr("gdkit net could not read its script manifest")
		quit(2)
		return
	var scripts: Array = []
	var errors: Array[String] = []
	for path: Variant in parsed:
		var resource_path := str(path)
		var resource := ResourceLoader.load(resource_path, "Script", ResourceLoader.CACHE_MODE_IGNORE)
		if not resource is Script:
			errors.append("%s did not load as a Script" % resource_path)
			continue
		scripts.append({
			"path": resource_path,
			"rpc_config": resource.get_rpc_config(),
		})
	var resolved_paths: Dictionary = {}
	for property: Dictionary in ProjectSettings.get_property_list():
		var setting_name := str(property.get("name", ""))
		if not setting_name.begins_with("autoload/"): continue
		var configured_path := str(ProjectSettings.get_setting(setting_name, ""))
		if configured_path.begins_with("*"): configured_path = configured_path.substr(1)
		if not configured_path.begins_with("uid://"): continue
		var resolved := ResourceUID.get_id_path(ResourceUID.text_to_id(configured_path))
		if not resolved.is_empty(): resolved_paths[configured_path] = resolved
	var version := Engine.get_version_info()
	print(RESULT_PREFIX + JSON.stringify({
		"version": str(version.get("string", "unknown")),
		"scripts": scripts,
		"errors": errors,
		"resolved_paths": resolved_paths,
	}))
	quit()
