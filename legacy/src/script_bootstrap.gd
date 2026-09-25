extends SceneTree


func _initialize() -> void:
	_load_script.call_deferred()


func _load_script() -> void:
	var arguments := OS.get_cmdline_user_args()
	if arguments.size() != 1:
		printerr("gdkit script bootstrap expected one project script argument")
		quit(2)
		return
	var target_script := load(arguments[0])
	if target_script == null or not target_script.can_instantiate():
		printerr("gdkit script bootstrap could not load " + arguments[0])
		quit(1)
		return
	set_script(target_script)
	_initialize()
