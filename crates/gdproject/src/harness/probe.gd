## Requires res://project.godot and res://probe.tres in the probe workspace.
## Payload: {version: String, major: int, editor: bool}.
extends SceneTree
const Protocol := preload("protocol.gd")


func _initialize() -> void:
	var version := Engine.get_version_info()
	var directory := DirAccess.open("res://")
	var resource := ResourceLoader.load("res://probe.tres", "", ResourceLoader.CACHE_MODE_IGNORE)
	if directory == null or resource == null or not FileAccess.file_exists("res://project.godot"):
		Protocol.emit_error(
			"probe", "resource", "Cannot read probe project directory or load res://probe.tres"
		)
		quit(1)
		return
	directory.get_directories()
	directory.get_files()
	(
		Protocol
		. emit_ok(
			"probe",
			{
				"version": str(version.get("string", "unknown")),
				"major": int(version.get("major", 0)),
				"editor": OS.has_feature("editor"),
			}
		)
	)
	quit(0)
