## User arg 0: project script (must extend SceneTree). Loads it, verifies the base
## class before set_script, then re-enters _initialize. A non-SceneTree script is
## an error envelope, not a spin.
extends SceneTree
const Protocol := preload("protocol.gd")


func _initialize() -> void:
	pass
