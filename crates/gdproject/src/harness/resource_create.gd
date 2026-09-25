## User args: <spec json path> <staged output path>.
## Builds the resource graph from the spec (Protocol.decode), assigns each property,
## verifies the getter returns an equal value, saves, reloads with CACHE_MODE_IGNORE,
## and echoes every property back encoded. Any divergence -> error envelope with
## stage "assign"|"save"|"reload"|"verify" and the field path.
## Payload: {"echo": {field: encoded}, "warnings": [String]}.
extends SceneTree
const Protocol := preload("protocol.gd")


func _initialize() -> void:
	pass
	quit(0)
