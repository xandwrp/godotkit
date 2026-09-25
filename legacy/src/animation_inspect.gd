extends SceneTree

const RESULT_PREFIX := "GDKIT_ANIMATION_RESULT:"


func finding(severity: String, code: String, message: String, path: String = "") -> Dictionary:
	return {"severity": severity, "code": code, "message": message, "path": path}


func node_path_from(root: Node, node: Node) -> String:
	if root == node:
		return "."
	return str(root.get_path_to(node))


func find_animation_trees(node: Node, result: Array[AnimationTree]) -> void:
	if node is AnimationTree:
		result.append(node as AnimationTree)
	for child in node.get_children():
		find_animation_trees(child, result)


func resource_identity(resource: Resource) -> Dictionary:
	return {
		"type": resource.get_class(),
		"resource_path": resource.resource_path,
		"resource_name": resource.resource_name,
	}


func inspect_transition(machine: AnimationNodeStateMachine, index: int) -> Dictionary:
	var transition := machine.get_transition(index)
	return {
		"from": str(machine.get_transition_from(index)),
		"to": str(machine.get_transition_to(index)),
		"advance_mode": transition.advance_mode,
		"advance_condition": str(transition.advance_condition),
		"advance_expression": str(transition.advance_expression),
		"switch_mode": transition.switch_mode,
		"xfade_time": transition.xfade_time,
		"priority": transition.priority,
		"reset": transition.reset,
		"break_loop_at_end": transition.break_loop_at_end,
	}


func inspect_graph_node(
	node: AnimationNode,
	path: String,
	player: AnimationPlayer,
	findings: Array[Dictionary]
) -> Dictionary:
	var result := resource_identity(node)
	result["path"] = path
	if node is AnimationNodeAnimation:
		var animation_node := node as AnimationNodeAnimation
		var animation_name := str(animation_node.animation)
		result["animation"] = animation_name
		result["animation_found"] = player != null and player.has_animation(animation_name)
		if not result["animation_found"]:
			findings.append(finding(
				"error",
				"missing_animation",
				"Animation node references missing animation '%s'" % animation_name,
				path
			))
	elif node is AnimationNodeStateMachine:
		var machine := node as AnimationNodeStateMachine
		var children: Array[Dictionary] = []
		for child_name_value in machine.get_node_list():
			var child_name := str(child_name_value)
			children.append(inspect_graph_node(
				machine.get_node(child_name_value),
				path + "/" + child_name,
				player,
				findings
			))
		var transitions: Array[Dictionary] = []
		for index in machine.get_transition_count():
			transitions.append(inspect_transition(machine, index))
		result["children"] = children
		result["transitions"] = transitions
		result["graph_analysis"] = analyze_state_machine(children, transitions, path, findings)
	elif node is AnimationNodeBlendTree:
		var blend_tree := node as AnimationNodeBlendTree
		var children: Array[Dictionary] = []
		for child_name_value in blend_tree.get_node_list():
			var child_name := str(child_name_value)
			children.append(inspect_graph_node(
				blend_tree.get_node(child_name_value),
				path + "/" + child_name,
				player,
				findings
			))
		var connections: Array[Dictionary] = []
		var serialized_connections: Array = blend_tree.get("node_connections")
		for index in range(0, serialized_connections.size(), 3):
			if index + 2 >= serialized_connections.size():
				break
			connections.append({
				"input_node": str(serialized_connections[index]),
				"input_index": int(serialized_connections[index + 1]),
				"output_node": str(serialized_connections[index + 2]),
			})
		result["children"] = children
		result["connections"] = connections
	elif node is AnimationNodeBlendSpace1D:
		var blend_space := node as AnimationNodeBlendSpace1D
		var children: Array[Dictionary] = []
		for index in blend_space.get_blend_point_count():
			var child := inspect_graph_node(
				blend_space.get_blend_point_node(index),
				path + "/point_%d" % index,
				player,
				findings
			)
			child["blend_position"] = blend_space.get_blend_point_position(index)
			children.append(child)
		result["children"] = children
	elif node is AnimationNodeBlendSpace2D:
		var blend_space := node as AnimationNodeBlendSpace2D
		var children: Array[Dictionary] = []
		for index in blend_space.get_blend_point_count():
			var child := inspect_graph_node(
				blend_space.get_blend_point_node(index),
				path + "/point_%d" % index,
				player,
				findings
			)
			var position := blend_space.get_blend_point_position(index)
			child["blend_position"] = {"x": position.x, "y": position.y}
			children.append(child)
		result["children"] = children
	return result


func analyze_state_machine(
	children: Array[Dictionary],
	transitions: Array[Dictionary],
	path: String,
	findings: Array[Dictionary]
) -> Dictionary:
	var state_names: Array[String] = []
	for child in children:
		var state_name := str(child["path"]).get_file()
		if state_name not in ["Start", "End"]:
			state_names.append(state_name)
	var reachable := {"Start": true}
	var changed := true
	while changed:
		changed = false
		for transition in transitions:
			var from := str(transition["from"])
			var to := str(transition["to"])
			if reachable.has(from) and not reachable.has(to):
				reachable[to] = true
				changed = true
	var unreachable: Array[String] = []
	var terminal: Array[String] = []
	for state_name in state_names:
		if not reachable.has(state_name):
			unreachable.append(state_name)
		var has_outgoing := false
		for transition in transitions:
			if transition["from"] == state_name:
				has_outgoing = true
				break
		if not has_outgoing:
			terminal.append(state_name)
	for state_name in unreachable:
		findings.append(finding(
			"warning",
			"unreachable_state",
			"State '%s' is not reachable from Start" % state_name,
			path + "/" + state_name
		))
	var unconditional_auto: Array[String] = []
	for transition in transitions:
		if (
			transition["advance_mode"] == AnimationNodeStateMachineTransition.ADVANCE_MODE_AUTO
			and transition["advance_condition"] == ""
			and transition["advance_expression"] == ""
		):
			unconditional_auto.append("%s -> %s" % [transition["from"], transition["to"]])
	if not unconditional_auto.is_empty():
		findings.append(finding(
			"warning",
			"unconditional_auto_transition",
			"Automatic transitions have no condition: %s" % ", ".join(unconditional_auto),
			path
		))
	return {
		"reachable_from_start": reachable.keys(),
		"unreachable_states": unreachable,
		"terminal_states": terminal,
		"unconditional_auto_transitions": unconditional_auto,
	}


func track_target(animation_root: Node, animation: Animation, index: int) -> Dictionary:
	var track_path := animation.track_get_path(index)
	var names := str(track_path.get_concatenated_names())
	var target: Node = animation_root if names.is_empty() else animation_root.get_node_or_null(NodePath(names))
	var bone := ""
	var bone_found = null
	var track_type := animation.track_get_type(index)
	if (
		target is Skeleton3D
		and track_path.get_subname_count() > 0
		and track_type in [Animation.TYPE_POSITION_3D, Animation.TYPE_ROTATION_3D, Animation.TYPE_SCALE_3D]
	):
		bone = str(track_path.get_subname(0))
		bone_found = (target as Skeleton3D).find_bone(bone) >= 0
	return {
		"index": index,
		"path": str(track_path),
		"type": track_type,
		"enabled": animation.track_is_enabled(index),
		"target_found": target != null,
		"bone": bone,
		"bone_found": bone_found,
	}


func inspect_animations(player: AnimationPlayer, findings: Array[Dictionary]) -> Array[Dictionary]:
	var result: Array[Dictionary] = []
	var animation_root := player.get_node_or_null(player.root_node)
	if animation_root == null:
		findings.append(finding(
			"error",
			"missing_animation_root",
			"AnimationPlayer root_node '%s' does not resolve" % player.root_node
		))
		return result
	for name_value in player.get_animation_list():
		var name := str(name_value)
		var animation := player.get_animation(name_value)
		var tracks: Array[Dictionary] = []
		var invalid_targets := 0
		var invalid_bones := 0
		for index in animation.get_track_count():
			var track := track_target(animation_root, animation, index)
			tracks.append(track)
			if not track["target_found"]:
				invalid_targets += 1
			elif track["bone_found"] == false:
				invalid_bones += 1
		if invalid_targets > 0:
			findings.append(finding(
				"error",
				"missing_track_target",
				"Animation '%s' has %d unresolved track target(s)" % [name, invalid_targets],
				name
			))
		if invalid_bones > 0:
			findings.append(finding(
				"error",
				"missing_track_bone",
				"Animation '%s' has %d unresolved skeleton bone track(s)" % [name, invalid_bones],
				name
			))
		result.append({
			"name": name,
			"length": animation.length,
			"loop_mode": animation.loop_mode,
			"tracks": tracks,
			"track_count": tracks.size(),
			"invalid_track_targets": invalid_targets,
			"invalid_bones": invalid_bones,
		})
	return result


func parameter_value(value: Variant) -> Variant:
	match typeof(value):
		TYPE_NIL, TYPE_BOOL, TYPE_INT, TYPE_FLOAT, TYPE_STRING, TYPE_STRING_NAME:
			return value
		TYPE_VECTOR2:
			return {"x": value.x, "y": value.y}
		TYPE_VECTOR3:
			return {"x": value.x, "y": value.y, "z": value.z}
		TYPE_VECTOR4:
			return {"x": value.x, "y": value.y, "z": value.z, "w": value.w}
		TYPE_OBJECT:
			return {"class": value.get_class()} if value != null else null
		_:
			return str(value)


func inspect_parameters(tree: AnimationTree) -> Array[Dictionary]:
	var result: Array[Dictionary] = []
	for property in tree.get_property_list():
		var name := str(property["name"])
		if name.begins_with("parameters/"):
			var value: Variant = tree.get(name)
			result.append({
				"name": name,
				"type": int(property["type"]),
				"class_name": str(property.get("class_name", "")),
				"value": parameter_value(value),
			})
	return result


func inspect_tree(scene_root: Node, tree: AnimationTree) -> Dictionary:
	var findings: Array[Dictionary] = []
	var player_node := tree.get_node_or_null(tree.anim_player)
	var player := player_node as AnimationPlayer
	var root := tree.get_node_or_null(tree.root_node)
	if not tree.active:
		findings.append(finding(
			"warning", "inactive_tree", "AnimationTree is not active", node_path_from(scene_root, tree)
		))
	if root == null:
		findings.append(finding(
			"error",
			"missing_tree_root",
			"AnimationTree root_node '%s' does not resolve" % tree.root_node,
			node_path_from(scene_root, tree)
		))
	if player == null:
		findings.append(finding(
			"error",
			"missing_animation_player",
			"AnimationTree anim_player '%s' does not resolve to AnimationPlayer" % tree.anim_player,
			node_path_from(scene_root, tree)
		))
	var graph = null
	if tree.tree_root == null:
		findings.append(finding(
			"error", "missing_graph_root", "AnimationTree has no tree_root", node_path_from(scene_root, tree)
		))
	elif player != null:
		graph = inspect_graph_node(tree.tree_root, "root", player, findings)
	var animations: Array[Dictionary] = []
	if player != null:
		animations = inspect_animations(player, findings)
	return {
		"path": node_path_from(scene_root, tree),
		"active": tree.active,
		"process_callback": tree.process_callback,
		"root_node": {"path": str(tree.root_node), "resolved": root != null},
		"animation_player": {
			"path": str(tree.anim_player),
			"resolved": player != null,
			"node_path": node_path_from(scene_root, player) if player != null else "",
		},
		"graph": graph,
		"parameters": inspect_parameters(tree),
		"animations": animations,
		"findings": findings,
	}


func finish(result: Dictionary, code: int) -> void:
	print(RESULT_PREFIX + JSON.stringify(result))
	quit(code)


func _initialize() -> void:
	var arguments := OS.get_cmdline_user_args()
	if arguments.is_empty():
		finish({"error": "missing scene path"}, 2)
		return
	var scene_path := arguments[0]
	var selector := arguments[1] if arguments.size() > 1 else ""
	var packed := ResourceLoader.load(scene_path, "PackedScene", ResourceLoader.CACHE_MODE_IGNORE) as PackedScene
	if packed == null:
		finish({"error": "could not load scene", "scene": scene_path}, 2)
		return
	var scene_root := packed.instantiate(PackedScene.GEN_EDIT_STATE_DISABLED)
	if scene_root == null:
		finish({"error": "could not instantiate scene", "scene": scene_path}, 2)
		return
	var trees: Array[AnimationTree] = []
	find_animation_trees(scene_root, trees)
	if not selector.is_empty():
		trees = trees.filter(func(tree: AnimationTree) -> bool:
			return node_path_from(scene_root, tree) == selector or tree.name == selector
		)
	var inspected: Array[Dictionary] = []
	for tree in trees:
		inspected.append(inspect_tree(scene_root, tree))
	var result := {
		"schema_version": 1,
		"scene": scene_path,
		"selector": selector,
		"execution": {"entered_scene_tree": false, "scene_scripts": "constructors_only"},
		"trees": inspected,
	}
	if trees.is_empty():
		result["findings"] = [finding(
			"error",
			"animation_tree_not_found",
			"No matching AnimationTree was found",
			selector
		)]
	else:
		result["findings"] = []
	trees.clear()
	scene_root.free()
	packed = null
	finish(result, 0)
