## Arg 0 is a full-file inventory JSON array of res:// paths; arg 1 is
## strict-methods | project-policy; arg 2 is a JSON-array file containing ImportScan's
## recognized_extensions. Counts include attempted loads, not instances.
## A successful envelope means completed work; diagnostics and failures still fail validation.
extends SceneTree
const Protocol := preload("protocol.gd")


func _init() -> void:
	var args := OS.get_cmdline_user_args()
	if args.size() == 3 and args[1] == "strict-methods":
		ProjectSettings.set_setting("debug/gdscript/warnings/enable", true)
		ProjectSettings.set_setting("debug/gdscript/warnings/unsafe_method_access", 2)
		ProjectSettings.emit_signal("settings_changed")


func _initialize() -> void:
	_check.call_deferred()


func _check() -> void:
	var args := OS.get_cmdline_user_args()
	if args.size() != 3 or args[1] not in ["strict-methods", "project-policy"]:
		Protocol.emit_error(
			"check",
			"arguments",
			"Expected manifest path, strict-methods | project-policy, and editor extensions file"
		)
		quit(2)
		return
	var manifest := Protocol.read_manifest(args[0])
	if not manifest.ok:
		Protocol.emit_error("check", "manifest", manifest.message, args[0])
		quit(2)
		return
	var editor_extensions := _read_extensions(args[2])
	if not editor_extensions.ok:
		Protocol.emit_error("check", "extensions", editor_extensions.message, args[2])
		quit(2)
		return
	# Union editor importers with loaders registered during runtime autoload startup.
	var recognized_extensions: Array[String] = editor_extensions.extensions
	for extension in ResourceLoader.get_recognized_extensions_for_type(""):
		var normalized := extension.to_lower()
		if normalized not in recognized_extensions:
			recognized_extensions.append(normalized)
	recognized_extensions.sort()
	var paths: Array = manifest.paths
	paths.sort_custom(
		func(a: String, b: String) -> bool:
			var a_script := a.get_extension().to_lower() == "gd"
			var b_script := b.get_extension().to_lower() == "gd"
			return a_script if a_script != b_script else a < b
	)
	var counts := {"scripts": 0, "scenes": 0, "resources": 0}
	var failures: Array = []
	var loaded: Array[Resource] = []
	for path: String in paths:
		var extension := path.get_extension().to_lower()
		if extension not in recognized_extensions:
			continue
		var kind := (
			"scripts"
			if extension == "gd"
			else ("scenes" if extension in ["tscn", "scn"] else "resources")
		)
		counts[kind] += 1
		var resource := ResourceLoader.load(path, "", ResourceLoader.CACHE_MODE_REPLACE)
		# Abstract scripts are valid check inputs even though they cannot instantiate.
		if (
			resource == null
			or (
				resource is Script and not resource.can_instantiate() and not resource.is_abstract()
			)
		):
			failures.append(path)
		# Loading a Shader only stores its code; the server compiles it on first RID
		# use. Headless 4.7's dummy renderer still runs ShaderLanguage there, so this
		# reports SHADER ERROR lines for shaders no material references. Failure is
		# visible only as engine diagnostics, which already fail validation.
		if resource is Shader:
			resource.get_rid()
		if resource != null:
			loaded.append(resource)
	loaded.clear()
	(
		Protocol
		. emit_ok(
			"check",
			{
				"counts": counts,
				"failures": failures,
				"recognized_extensions": recognized_extensions,
			}
		)
	)
	quit(0 if failures.is_empty() else 1)


func _read_extensions(path: String) -> Dictionary:
	var file := FileAccess.open(path, FileAccess.READ)
	if file == null:
		return {"ok": false, "message": "Cannot open editor extensions file"}
	var parser := JSON.new()
	if parser.parse(file.get_as_text()) != OK or not parser.data is Array:
		return {
			"ok": false, "message": "Editor extensions must be a JSON array of extension strings"
		}
	var extensions: Array[String] = []
	for entry: Variant in parser.data:
		if not entry is String or entry.is_empty():
			return {"ok": false, "message": "Editor extensions must contain nonempty strings"}
		for index in range(entry.length()):
			if entry.unicode_at(index) <= 32 or entry[index] in [".", "/", "\\", ":"]:
				return {
					"ok": false,
					"message":
					"Editor extensions must be suffixes without dots, paths or whitespace"
				}
		var normalized: String = entry.to_lower()
		if normalized not in extensions:
			extensions.append(normalized)
	return {"ok": true, "extensions": extensions}
