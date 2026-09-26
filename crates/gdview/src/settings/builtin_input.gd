# Regenerates builtin_input.godot, Godot's built-in ui_* input actions, in
# project.godot's [input] format. From an empty project (project.godot holding
# only `config_version=5`), with the engine gdkit targets:
#   godot --headless --path <empty> --script <this file> > builtin_input.godot
# `.macos` entries are feature overrides, not actions, so they are left out.
extends SceneTree


func _initialize() -> void:
	var lines := PackedStringArray()
	lines.append("; Godot %s built-in input actions. Regenerate with builtin_input.gd." % Engine.get_version_info().string)
	lines.append("[input]")
	var names: Array[String] = []
	for action in InputMap.get_actions():
		if str(action).begins_with("ui_") and not str(action).contains("."):
			names.append(str(action))
	names.sort()
	for action in names:
		var events := PackedStringArray()
		for event in InputMap.action_get_events(action):
			events.append(var_to_str(event).replace("\n", ""))
		lines.append("%s={\n\"deadzone\": %s,\n\"events\": [%s]\n}" % [
			action, var_to_str(InputMap.action_get_deadzone(action)), ",\n".join(events)])
	print("\n".join(lines))
	quit(0)
