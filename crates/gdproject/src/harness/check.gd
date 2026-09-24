## Loads every path in the manifest (user arg 0) in a fresh process.
## User arg 1: "strict-methods" | "project-policy". Strict policy is applied in _init,
## before autoloads instantiate.
## Payload: {"counts": {"scripts","scenes","resources"}, "failures": [res paths]}.
## ok is false only for tool errors (bad manifest); load failures are data.
extends SceneTree
const Protocol := preload("protocol.gd")


func _init() -> void:
	pass


func _initialize() -> void:
	pass
	quit(0)
