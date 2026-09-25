## Arg 0: SceneTree project script. The marker precedes the set_script handoff,
## including its _init callback. Loading can execute static initializers first.
## There is no success envelope; the user owns quit.
extends SceneTree
const Protocol := preload("protocol.gd")


func _initialize() -> void:
	_load_script.call_deferred()


func _load_script() -> void:
	var args := OS.get_cmdline_user_args()
	if args.size() != 1:
		Protocol.emit_error("script_bootstrap", "arguments", "Expected one SceneTree script path")
		quit(2)
		return
	var target := ResourceLoader.load(args[0], "", ResourceLoader.CACHE_MODE_REPLACE) as Script
	if target == null or not target.can_instantiate():
		Protocol.emit_error(
			"script_bootstrap", "load", "Cannot load an instantiable script", args[0]
		)
		quit(1)
		return
	if target.get_instance_base_type() != &"SceneTree":
		Protocol.emit_error(
			"script_bootstrap", "base", "Project script must extend SceneTree", args[0]
		)
		quit(1)
		return
	print("GDKIT_SCRIPT_STARTED")
	set_script(target)
	_initialize()
