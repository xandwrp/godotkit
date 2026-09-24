@tool
extends SceneTree


func _initialize() -> void:
	call_deferred("scan_project")


func scan_project() -> void:
	var arguments := OS.get_cmdline_user_args()
	if arguments.size() != 2:
		push_error("gdkit import scan requires a project manifest")
		quit(2)
		return
	var filesystem := EditorInterface.get_resource_filesystem()
	await process_frame
	while filesystem.is_scanning() or filesystem.is_importing():
		await process_frame
	filesystem.scan()
	await process_frame
	while filesystem.is_scanning() or filesystem.is_importing():
		await process_frame
	var manifest := FileAccess.open(arguments[0], FileAccess.READ)
	if manifest == null:
		push_error("gdkit import scan could not open its project manifest")
		quit(2)
		return
	var paths = JSON.parse_string(manifest.get_as_text())
	if not paths is Array:
		push_error("gdkit import scan received an invalid project manifest")
		quit(2)
		return
	for path in paths:
		if path is String and path.ends_with(".gd"):
			ResourceLoader.load(path, "", ResourceLoader.CACHE_MODE_REPLACE)
	var completion := FileAccess.open(arguments[1], FileAccess.WRITE)
	if completion == null:
		push_error("gdkit import scan could not record completion")
		quit(2)
		return
	completion.store_string("GDKIT_IMPORT_SCAN_COMPLETE")
	completion.close()
	push_warning("GDKIT_IMPORT_SCAN_COMPLETE")
	quit()
