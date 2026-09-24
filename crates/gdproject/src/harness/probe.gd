## Compatibility probe. Payload: {"version": String, "major": int, "editor": bool}.
## Also loads probe.tres and lists res:// to prove resource loading and DirAccess work.
extends SceneTree
const Protocol := preload("protocol.gd")


func _initialize() -> void:
	pass
	quit(0)
