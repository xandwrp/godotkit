extends Node2D

@rpc
func ping() -> void:
	pass

@rpc("any_peer", "call_local", "reliable", 3)
func fire() -> void:
	pass

func send() -> void:
	ping.rpc()
	rpc_id(1, &"fire")
	Callable(self, "fire").rpc_id(1)
	multiplayer.rpc(1, self, &"ping")
