## User args: <scene res path> [tree node path].
## Instantiates the scene off-tree (constructors run, _ready does not) and reports
## AnimationTree graphs, generated parameters, AnimationPlayer inventories, track
## targets, and structural findings. Trees are identified by full node path.
## Payload: gdproject::animation::AnimationInspection.
extends SceneTree
const Protocol := preload("protocol.gd")


func _initialize() -> void:
	pass
	quit(0)
