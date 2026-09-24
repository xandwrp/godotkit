## User arg 0: manifest of .gd paths. For each script (and inner classes), instantiates
## nothing; reads the compiled script's get_rpc_config() to report effective RPC
## configuration. Payload: gdview::net::EngineFacts as JSON.
extends SceneTree
const Protocol := preload("protocol.gd")


func _initialize() -> void:
	pass
	quit(0)
