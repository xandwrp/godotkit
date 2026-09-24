## Runs under --editor. Waits for the editor filesystem scan and import to settle,
## then loads every .gd in the manifest with CACHE_MODE_REPLACE so the global
## script class cache is written. Payload: {"scanned": int}.
@tool
extends SceneTree
const Protocol := preload("protocol.gd")


func _initialize() -> void:
	pass
