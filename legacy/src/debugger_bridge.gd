extends SceneTree

const SCHEMA_VERSION := 1
const MAX_EVENTS := 128
const MAX_REQUEST_BYTES := 16384

var debugger_server := TCPServer.new()
var query_server := TCPServer.new()
var debugger_stream: StreamPeerTCP
var debugger_packets: PacketPeerStream
var query_peer: StreamPeerTCP
var query_buffer := PackedByteArray()
var generation := ""
var token := ""
var events: Array[Dictionary] = []


func _initialize() -> void:
	generation = OS.get_environment("GDKIT_DEBUGGER_GENERATION")
	token = OS.get_environment("GDKIT_DEBUGGER_TOKEN")
	var ready_path := OS.get_environment("GDKIT_DEBUGGER_READY")
	if generation.is_empty() or token.is_empty() or ready_path.is_empty():
		quit(2)
		return
	if debugger_server.listen(0, "127.0.0.1") != OK or query_server.listen(0, "127.0.0.1") != OK:
		quit(2)
		return
	var ready := FileAccess.open(ready_path, FileAccess.WRITE)
	if ready == null:
		quit(2)
		return
	ready.store_string(JSON.stringify({
		"schema_version": SCHEMA_VERSION,
		"generation": generation,
		"debugger_port": debugger_server.get_local_port(),
		"query_port": query_server.get_local_port(),
	}))
	ready.close()


func _process(_delta: float) -> bool:
	poll_debugger()
	poll_query()
	return false


func poll_debugger() -> void:
	if debugger_stream == null and debugger_server.is_connection_available():
		debugger_stream = debugger_server.take_connection()
		debugger_stream.set_no_delay(true)
		debugger_packets = PacketPeerStream.new()
		debugger_packets.stream_peer = debugger_stream
		debugger_packets.input_buffer_max_size = 8388612
		debugger_packets.output_buffer_max_size = 8388612
		debugger_packets.put_var(["profiler:multiplayer:rpc", 1, [true, []]])
	if debugger_stream == null:
		return
	debugger_stream.poll()
	if debugger_stream.get_status() != StreamPeerTCP.STATUS_CONNECTED:
		quit()
		return
	while debugger_packets.get_available_packet_count() > 0:
		var message = debugger_packets.get_var(false)
		if message is Array and message.size() == 3 and message[0] == "multiplayer:rpc":
			capture_rpc_frame(message[2])


func capture_rpc_frame(frame: Variant) -> void:
	if not frame is Array or frame.is_empty() or int(frame[0]) + 1 != frame.size() or int(frame[0]) % 6 != 0:
		return
	var index := 1
	while index < frame.size():
		append_rpc("in", str(frame[index + 1]), int(frame[index + 2]), int(frame[index + 3]))
		append_rpc("out", str(frame[index + 1]), int(frame[index + 4]), int(frame[index + 5]))
		index += 6


func append_rpc(direction: String, node_path: String, count: int, bytes: int) -> void:
	if count == 0:
		return
	events.append({
		"kind": "rpc",
		"collected_at_unix_ms": int(Time.get_unix_time_from_system() * 1000.0),
		"process_tick": 0,
		"api_instance_id": 0,
		"peer_id": null,
		"direction": direction,
		"node_path": node_path,
		"bytes": bytes,
		"count": count,
	})
	if events.size() > MAX_EVENTS:
		events.pop_front()


func poll_query() -> void:
	if query_peer == null and query_server.is_connection_available():
		query_peer = query_server.take_connection()
		query_peer.set_no_delay(true)
		query_buffer.clear()
	if query_peer == null:
		return
	query_peer.poll()
	if query_peer.get_status() != StreamPeerTCP.STATUS_CONNECTED:
		query_peer = null
		return
	var available := query_peer.get_available_bytes()
	if available == 0:
		return
	var data := query_peer.get_data(available)
	if data[0] != OK:
		query_peer.disconnect_from_host()
		query_peer = null
		return
	query_buffer.append_array(data[1])
	if query_buffer.size() > MAX_REQUEST_BYTES:
		query_peer.disconnect_from_host()
		query_peer = null
		return
	if query_buffer.has(10):
		var request = JSON.parse_string(query_buffer.get_string_from_utf8())
		if not valid_request(request):
			query_peer.disconnect_from_host()
			query_peer = null
			return
		var response := (JSON.stringify({
			"schema_version": SCHEMA_VERSION,
			"request_id": request.request_id,
			"generation": generation,
			"recent_events": events,
		}) + "\n").to_utf8_buffer()
		if response.size() <= int(request.max_response_bytes):
			query_peer.put_data(response)
		query_peer.disconnect_from_host()
		query_peer = null
		query_buffer.clear()


func valid_request(request: Variant) -> bool:
	if not request is Dictionary or request.size() != 7:
		return false
	return request.get("schema_version") == SCHEMA_VERSION \
		and request.get("request_id") is String \
		and not request.request_id.is_empty() \
		and request.get("generation") == generation \
		and request.get("token") == token \
		and request.get("kind") == "rpc_events" \
		and request.get("deadline_unix_ms") is float \
		and request.deadline_unix_ms >= Time.get_unix_time_from_system() * 1000.0 \
		and request.get("max_response_bytes") is float \
		and request.max_response_bytes >= 4096.0 \
		and request.max_response_bytes <= 8388608.0
