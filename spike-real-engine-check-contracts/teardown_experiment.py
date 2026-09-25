"""Compare real editor teardown paths without filtering any diagnostics."""
import json, os, pathlib, signal, subprocess, time
ROOT = pathlib.Path(__file__).resolve().parent
OUT = ROOT / 'artifacts' / ('teardown-' + time.strftime('%Y%m%d-%H%M%S'))
OUT.mkdir(parents=True)
source = (ROOT / 'import_scan.gd').read_text()
results = []
for name, ending in {
    'placeholder-static': 'quit.call_deferred(0)',
    'root-close-signal': 'root.close_requested.emit()',
        'root-close-deferred': 'root.emit_signal.call_deferred("close_requested")',
        'auto-accept-off-close': 'auto_accept_quit = false\n\troot.close_requested.emit()',
        'immediate': 'quit(0)',
    'deferred': 'quit.call_deferred(0)',
    'frame-deferred': 'await process_frame\n\tquit.call_deferred(0)',
    'editor-notification': 'EditorInterface.get_base_control().get_parent().notification(Node.NOTIFICATION_WM_CLOSE_REQUEST)',
    'root-notification': 'root.propagate_notification(Node.NOTIFICATION_WM_CLOSE_REQUEST)',
    'editor-free': 'EditorInterface.get_base_control().get_parent().queue_free()\n\tawait process_frame\n\tquit.call_deferred(0)',
    'ten-frames': 'for i in range(10):\n\t\tawait process_frame\n\tquit.call_deferred(0)',
        'editor-deferred-close': 'EditorInterface.get_base_control().get_parent().notification.call_deferred(Node.NOTIFICATION_WM_CLOSE_REQUEST)',
        'editor-free-delayed': 'EditorInterface.get_base_control().get_parent().queue_free()\n\tfor i in range(10):\n\t\tawait process_frame\n\tquit.call_deferred(0)',
}.items():
    if name not in ['placeholder-static', 'immediate']:
        continue
    project = OUT / name
    project.mkdir()
    (project / 'project.godot').write_text('config_version=5\n[application]\nconfig/name="Teardown"\n[rendering]\nrenderer/rendering_method="gl_compatibility"\n')
    (project / 'sample.gd').write_text('class_name TeardownSample\nextends RefCounted\n')
    (project / 'manifest.json').write_text('["res://sample.gd"]')
    harness = OUT / (name + '.gd')
    candidate = source.replace('quit(0)', ending)
    if name == 'placeholder-static':
        candidate += '''\n\nstatic func _static_init() -> void:
	for id in Node.get_orphan_node_ids():
		var node = instance_from_id(id)
		if node is Window and node.name == &"root":
			for connection in node.get_signal_connection_list("close_requested"):
				var target = connection.callable.get_object()
				if target is SceneTree and target.root == node:
					print("PLACEHOLDER:", target)
					target.free()
					return
'''
    harness.write_text(candidate)
    env = dict(os.environ, HOME=str(project), XDG_CONFIG_HOME=str(project / 'config'), XDG_DATA_HOME=str(project / 'data'), XDG_CACHE_HOME=str(project / 'cache'))
    cmd = ['/usr/bin/godot', '--headless', '--path', str(project), '--editor', '--script', str(harness), '--', str(project / 'manifest.json')]
    proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env, start_new_session=True)
    timeout = False
    try:
        stdout, stderr = proc.communicate(timeout=12)
    except subprocess.TimeoutExpired:
        timeout = True
        os.killpg(proc.pid, signal.SIGKILL)
        stdout, stderr = proc.communicate(timeout=3)
    (project / 'stdout.log').write_bytes(stdout)
    (project / 'stderr.log').write_bytes(stderr)
    record = dict(name=name, argv=cmd, exit=proc.returncode, timeout=timeout, stderr=stderr.decode(errors='replace'), errors=[line for line in (stdout + stderr).decode(errors='replace').splitlines() if 'ERROR:' in line])
    results.append(record)
    print(json.dumps(record), flush=True)
(OUT / 'results.json').write_text(json.dumps(results, indent=2))
print(OUT)
