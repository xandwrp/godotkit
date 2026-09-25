## Runs under --editor. Arg 0 is a JSON-array manifest file of res:// paths.
## Loads only .gd entries, without instantiation. Completion is NOT parse success:
## Godot can return a non-null Script after reporting a parse error.
@tool
extends SceneTree
const Protocol := preload("protocol.gd")


## Godot 4.7.2 Main::start allocates an editor SceneTree before loading --script,
## then overwrites it with the script instance. Its orphan remains the singleton,
## so even deferred editor deletion queues leak. Release ONLY that unused tree,
## during script loading, before our SceneTree is constructed. Do not do this
## when an editor merely loads this file during a scan or on untested versions.
static func _static_init() -> void:
	var version := Engine.get_version_info()
	if version.major != 4 or version.minor != 7 or version.patch != 2:
		return
	if not Engine.is_editor_hint() or Engine.get_main_loop() != null:
		return
	for id in Node.get_orphan_node_ids():
		var node := instance_from_id(id) as Window
		if node == null or node.name != &"root" or node.get_child_count() != 0:
			continue
		for connection in node.get_signal_connection_list("close_requested"):
			var tree := connection.callable.get_object() as SceneTree
			if tree != null and tree.root == node and tree.get_script() == null:
				tree.free()
				return


func _initialize() -> void:
	_scan.call_deferred()


func _settle(filesystem: EditorFileSystem) -> void:
	await process_frame
	while filesystem.is_scanning() or filesystem.is_importing():
		await process_frame


func _scan() -> void:
	var args := OS.get_cmdline_user_args()
	if args.size() != 1:
		Protocol.emit_error("import_scan", "arguments", "Expected one manifest path")
		await _finish(2)
		return
	var manifest := Protocol.read_manifest(args[0])
	if not manifest.ok:
		Protocol.emit_error("import_scan", "manifest", manifest.message, args[0])
		await _finish(2)
		return
	if not Engine.is_editor_hint():
		Protocol.emit_error("import_scan", "editor", "Import scan requires --editor")
		quit.call_deferred(2)
		return
	var filesystem := EditorInterface.get_resource_filesystem()
	await _settle(filesystem)
	filesystem.scan()
	await _settle(filesystem)
	var scanned := 0
	var failures: Array = []
	for path: String in manifest.paths:
		if path.get_extension().to_lower() != "gd":
			continue
		scanned += 1
		var script := ResourceLoader.load(path, "", ResourceLoader.CACHE_MODE_REPLACE) as Script
		if script == null:
			failures.append(path)
	if not failures.is_empty():
		Protocol.emit_error("import_scan", "load", "Could not load scripts: " + ", ".join(failures))
		await _finish(2)
		return
	# Editor startup registers importers that runtime extension enumeration lacks.
	# This is capability metadata, independent of whether any source imported well.
	var recognized_extensions: Array[String] = []
	for extension in ResourceLoader.get_recognized_extensions_for_type(""):
		var normalized := extension.to_lower()
		if normalized not in recognized_extensions:
			recognized_extensions.append(normalized)
	recognized_extensions.sort()
	(
		Protocol
		. emit_ok(
			"import_scan",
			{
				"scanned": scanned,
				"recognized_extensions": recognized_extensions,
			}
		)
	)
	await _finish(0)


func _finish(exit_code: int) -> void:
	# Even argument errors must allow the initial editor scan to finish.
	if Engine.is_editor_hint():
		await _settle(EditorInterface.get_resource_filesystem())
	quit.call_deferred(exit_code)
