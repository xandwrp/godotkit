## Runs as the game's SceneTree via --script for `gdkit run`. Environment:
##   GDKIT_PROBE_READY_FILE  path to write {"port","token","protocol"} once listening
##   GDKIT_PROBE_TOKEN       shared secret
##   GDKIT_PROBE_SCENE       scene to instantiate (else the project main scene)
##   GDKIT_PROBE_ADAPTER     optional checkpoint adapter res:// path
## Listens on 127.0.0.1:0. One JSON line per request/response:
##   {"token","id","kind":"status"|"checkpoints"|"network"} -> {"id","ok","payload"|"error"}
## Counts process frames; observes MultiplayerAPI via `node_added`, never per-frame walks.
## Adapter instances that are not RefCounted are freed after each call.
## Never attaches a debugger; a script error in the game does not block the probe.
extends SceneTree
const Protocol := preload("protocol.gd")


func _initialize() -> void:
	pass
