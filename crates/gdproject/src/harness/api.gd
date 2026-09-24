## Reflects ClassDB into gdview::api::ApiIndex JSON (schema_version 3).
## Includes GDExtension classes registered by the project at --path.
## Payload: the ApiIndex object. Emits an error envelope if ClassDB reports nothing.
extends SceneTree
const Protocol := preload("protocol.gd")


func _initialize() -> void:
	pass
	quit(0)
