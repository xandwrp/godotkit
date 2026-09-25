extends SceneTree


func _initialize() -> void:
	_load_script.call_deferred()


func fail(stage: String, message: String) -> void:
	print("GDKIT_RESULT:" + JSON.stringify({"protocol": 1, "harness": "script_bootstrap", "ok": false, "error": {"stage": stage, "message": message}}))
	quit(1)


func _load_script() -> void:
	var args := OS.get_cmdline_user_args()
	if args.size() != 1:
		fail("arguments", "expected one project script")
		return
	var target := load(args[0]) as Script
	if target == null or not target.can_instantiate():
		fail("load", "cannot instantiate " + args[0])
		return
	if target.get_instance_base_type() != &"SceneTree":
		fail("base", "script must extend SceneTree, got " + target.get_instance_base_type())
		return
	print("GDKIT_SCRIPT_STARTED")
	set_script(target)
	_initialize()
