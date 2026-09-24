## Runs as the session's SceneTree via --script. Environment:
##   GDKIT_PROBE_READY_FILE  path to touch with {"port","token","protocol"} once listening
##   GDKIT_PROBE_TOKEN       shared secret
##   GDKIT_PROBE_SCENE       optional scene to instantiate instead of the main scene
## Listens on 127.0.0.1:0 (OS-selected port). One JSON line per request/response:
##   {"token","id","kind":"network"|"checkpoints","adapter"?} -> {"id","ok","payload"|"error"}
## Observes MultiplayerAPI signals (peer_connected, etc.) via `node_added`, never per-frame walks.
## Adapter instances that are not RefCounted are freed after each call.
## Never attaches a debugger; script errors in the game do not block the probe.
extends SceneTree
const Protocol := preload("protocol.gd")


func _initialize() -> void:
	pass
