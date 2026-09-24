extends SceneTree


func _initialize() -> void:
	var version := Engine.get_version_info()
	var directory := DirAccess.open("res://")
	var resource := ResourceLoader.load("res://probe.tres", "", ResourceLoader.CACHE_MODE_IGNORE)
	var compatible := int(version.get("major", 0)) == 4 and OS.has_feature("editor")
	compatible = compatible and directory != null and resource != null
	if directory != null:
		directory.get_directories()
		directory.get_files()
	compatible = compatible and FileAccess.file_exists("res://project.godot")
	print("GDKIT_PROBE_RESULT:" + JSON.stringify({ "compatible": compatible, "version": version.get("string", "unknown") }))
	quit(0 if compatible else 1)
