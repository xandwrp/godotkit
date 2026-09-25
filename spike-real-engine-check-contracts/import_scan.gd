@tool
extends SceneTree


func _initialize() -> void:
	scan_project.call_deferred()


func settle(filesystem: EditorFileSystem) -> void:
	await process_frame
	while filesystem.is_scanning() or filesystem.is_importing():
		await process_frame


func scan_project() -> void:
	var args := OS.get_cmdline_user_args()
	if args.size() != 1:
		push_error("import_scan requires a manifest")
		quit(2)
		return
	var filesystem := EditorInterface.get_resource_filesystem()
	await settle(filesystem)
	filesystem.scan()
	await settle(filesystem)
	var file := FileAccess.open(args[0], FileAccess.READ)
	if file == null:
		push_error("cannot open manifest")
		quit(2)
		return
	var paths = JSON.parse_string(file.get_as_text())
	if not paths is Array:
		push_error("manifest must be an array")
		quit(2)
		return
	var failed: Array[String] = []
	var scanned := 0
	for path in paths:
		if not path is String or not path.ends_with(".gd"):
			push_error("manifest must contain only script paths")
			quit(2)
			return
		scanned += 1
		var script := ResourceLoader.load(path, "", ResourceLoader.CACHE_MODE_REPLACE) as Script
		if script == null:
			failed.append(path)
	print("GDKIT_RESULT:" + JSON.stringify({"protocol": 1, "harness": "import_scan", "ok": true, "payload": {"scanned": scanned, "null_loads": failed, "scanning": filesystem.is_scanning(), "importing": filesystem.is_importing(), "cache_exists_before_quit": FileAccess.file_exists("res://.godot/global_script_class_cache.cfg")}}))
	quit(0)
