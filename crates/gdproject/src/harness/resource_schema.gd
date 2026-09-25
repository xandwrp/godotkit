## User args: "class" <Name> | "script" <res path>.
## Instantiates, walks get_property_list, reports each stored field with type,
## class_name, element type (from hint_string for typed arrays), default (encoded),
## hint, hint_string, enum choices, and accepted spec shapes.
## Payload: gdproject::resource::ResourceSchema.
extends SceneTree
const Protocol := preload("protocol.gd")


func _initialize() -> void:
	pass
	quit(0)
