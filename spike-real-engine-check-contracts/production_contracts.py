"""Real-engine production harness checks, with full streams and bounded process groups."""
import json, os, pathlib, re, signal, struct, subprocess, time
ROOT = pathlib.Path(__file__).resolve().parent
HARNESS = ROOT.parent / 'crates/gdproject/src/harness'
OUT = ROOT / 'artifacts' / ('production-' + time.strftime('%Y%m%d-%H%M%S'))
OUT.mkdir(parents=True)
results = []
checks = []
def write(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
def run(name, project, harness, args=(), editor=False, deadline=12):
    # Runtime-only fixtures explicitly use an empty editor capability list.
    if harness == HARNESS / 'check.gd' and len(args) == 2:
        args = [*args, OUT / 'empty-extensions.json']
    cmd = ['/usr/bin/godot', '--headless', '--path', str(project)]
    if editor: cmd += ['--editor']
    if harness is None:
        cmd += ['--import']
    else:
        cmd += ['--script', str(harness), '--', *map(str, args)]
    env = dict(os.environ, HOME=str(OUT), XDG_CONFIG_HOME=str(OUT / 'config'), XDG_DATA_HOME=str(OUT / 'data'), XDG_CACHE_HOME=str(OUT / 'cache'))
    proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env, start_new_session=True)
    timeout = False
    try: stdout, stderr = proc.communicate(timeout=deadline)
    except subprocess.TimeoutExpired:
        timeout = True
        os.killpg(proc.pid, signal.SIGKILL)
        stdout, stderr = proc.communicate(timeout=3)
    stdout, stderr = stdout.decode(errors='replace'), stderr.decode(errors='replace')
    write(OUT / (name + '.stdout.log'), stdout)
    write(OUT / (name + '.stderr.log'), stderr)
    envelopes = [json.loads(line.removeprefix('GDKIT_RESULT:')) for line in stdout.splitlines() if line.startswith('GDKIT_RESULT:')]
    record = dict(name=name, argv=cmd, exit=proc.returncode, timeout=timeout, envelopes=envelopes,
                  errors=re.findall(r'(?m)^\s*(?:SCRIPT ERROR:|ERROR:|USER ERROR:)[^\n]*', stdout+'\n'+stderr), marker=stdout.splitlines().count('GDKIT_SCRIPT_STARTED'))
    results.append(record)
    print(json.dumps(record), flush=True)
    return record, stdout

def check(name, condition):
    checks.append(dict(name=name, passed=bool(condition)))
    print(('PASS ' if condition else 'FAIL ') + name, flush=True)

write(OUT / 'empty-extensions.json', '[]')
p = OUT / 'project'
write(p / 'project.godot', 'config_version=5\n[application]\nconfig/name="Production contracts"\n[autoload]\nSingleton="*res://singleton.gd"\n[rendering]\nrenderer/rendering_method="gl_compatibility"\n')
write(p / 'singleton.gd', 'extends Node\nvar ready_seen := false\nfunc _ready():\n\tready_seen = true\n')
write(p / 'probe.tres', '[gd_resource type="Resource" format=3]\n[resource]\nresource_name="probe"\n')
write(p / 'good.gd', 'extends Node\nfunc _init():\n\tpush_error("CHECK_INSTANTIATED_SCRIPT")\n')
write(p / 'scene.tscn', '[gd_scene load_steps=2 format=3]\n[ext_resource type="Script" path="res://good.gd" id="1"]\n[node name="Test" type="Node"]\nscript = ExtResource("1")\n')
write(p / 'manifest.json', '["res://good.gd","res://scene.tscn","res://probe.tres"]')
r, _ = run('probe', p, HARNESS / 'probe.gd')
check('probe capability payload', r['exit'] == 0 and not r['errors'] and r['envelopes'][0]['payload']['major'] == 4 and r['envelopes'][0]['payload']['editor'])
r, _ = run('check-valid', p, HARNESS / 'check.gd', [p / 'manifest.json', 'project-policy'])
payload = r['envelopes'][0]['payload']
check('check counts without instantiation', r['exit'] == 0 and not r['errors'] and payload['counts'] == dict(scripts=1, scenes=1, resources=1) and payload['failures'] == [])
check('recognized extensions normalized and unique', payload['recognized_extensions'] == sorted(set(ext.lower() for ext in payload['recognized_extensions'])))
write(p / 'unsafe.gd', 'extends Node\nfunc test(value: Node):\n\tvalue.not_known()\n')
write(p / 'unsafe.json', '["res://unsafe.gd"]')
for policy in ['project-policy', 'strict-methods']:
    r, _ = run('check-'+policy, p, HARNESS / 'check.gd', [p / 'unsafe.json', policy])
    check('policy '+policy, bool(r['errors']) == (policy == 'strict-methods'))
write(p / 'bad.json', '["res://../escape.gd"]')
r, _ = run('bad-manifest', p, HARNESS / 'check.gd', [p / 'bad.json', 'project-policy'])
check('invalid manifest error envelope', r['exit'] == 2 and len(r['envelopes']) == 1 and not r['envelopes'][0]['ok'])
write(p / 'broken.gd', 'extends Node\nfunc broken( -> void:\n\tpass\n')
write(p / 'broken.json', '["res://broken.gd","res://missing.tres"]')
r, _ = run('check-broken', p, HARNESS / 'check.gd', [p / 'broken.json', 'project-policy'])
check('failed load paths', r['exit'] == 1 and r['envelopes'][0]['payload']['failures'] == ['res://broken.gd','res://missing.tres'])
for name, script in {
    'normal': 'extends SceneTree\nfunc _init():\n\tprint("USER_INIT")\nfunc _initialize():\n\tassert(Singleton.ready_seen)\n\tquit(0)\n',
    'node': 'extends Node\n',
    'parse': 'extends SceneTree\nfunc nope( ->:\n',
    'seven': 'extends SceneTree\nfunc _initialize():\n\tquit(7)\n',
    'error': 'extends SceneTree\nfunc _initialize():\n\tquit.call_deferred(0)\n\tvar x = null\n\tx.nope()\n',
    'timeout': 'extends SceneTree\nfunc _initialize():\n\tpass\n',
}.items():
    write(p / (name+'.gd'), script)
    r, stdout = run('script-'+name, p, HARNESS / 'script_bootstrap.gd', ['res://'+name+'.gd'], deadline=2)
    if name == 'normal': check('marker before user init and autoload ready', r['exit'] == 0 and not r['errors'] and r['marker'] == 1 and not r['envelopes'] and stdout.index('GDKIT_SCRIPT_STARTED') < stdout.index('USER_INIT'))
    elif name in ['node','parse']: check('script rejects '+name, r['exit'] == 1 and r['marker'] == 0 and len(r['envelopes']) == 1 and not r['envelopes'][0]['ok'])
    elif name == 'seven': check('user exit preserved', r['exit'] == 7 and r['marker'] == 1)
    elif name == 'error': check('runtime error not hidden by zero exit', r['exit'] == 0 and r['marker'] == 1 and r['errors'])
    else: check('user owns quit', r['timeout'] and r['marker'] == 1)
write(p / 'abstract.gd', '@abstract\nextends Node\n@abstract func work() -> void\n')
write(p / 'abstract.json', '["res://abstract.gd"]')
r, _ = run('check-abstract', p, HARNESS / 'check.gd', [p / 'abstract.json', 'strict-methods'])
check('abstract script is valid without instantiation', r['exit'] == 0 and not r['errors'] and r['envelopes'][0]['payload']['failures'] == [])
write(p / 'singleton.gd', 'extends Node\nfunc _init():\n\tprint("AUTOLOAD_POLICY:", ProjectSettings.get_setting("debug/gdscript/warnings/unsafe_method_access"))\nfunc unsafe(value: Node):\n\tvalue.not_known()\n')
for policy in ['project-policy', 'strict-methods']:
    r, stdout = run('autoload-'+policy, p, HARNESS / 'check.gd', [p / 'abstract.json', policy])
    check('policy applied before autoload '+policy, bool(r['errors']) == (policy == 'strict-methods'))
write(p / 'singleton.gd', 'extends Node\n')
write(p / 'scripted_resource.gd', 'extends Resource\n@export var answer: int = 42\n')
r, _ = run('variant-roundtrips', p, ROOT / 'variant_contracts.gd')
check('47 Variant JSON roundtrips', r['exit'] == 0 and not r['errors'] and r['envelopes'][0]['payload']['cases'] == 47)
actual_golden = r['envelopes'][0]
write(OUT / 'protocol_actual.json', json.dumps(actual_golden, indent=2, ensure_ascii=False)+'\n')
expected_golden = json.loads((ROOT / 'protocol_golden.json').read_text())
check('protocol matches frozen engine golden without Rust decoder', json.dumps(actual_golden, sort_keys=True) == json.dumps(expected_golden, sort_keys=True))
error_script = 'extends SceneTree\nconst Protocol = preload('+json.dumps(str(HARNESS / 'protocol.gd'))+')\nfunc _initialize():\n\tProtocol.encode(RID())\n\tProtocol.decode({"$variant": {"type": "FutureType", "value": null}})\n\tvar cycle: Array = []\n\tcycle.append(cycle)\n\tProtocol.encode(cycle)\n\tcycle.clear()\n\tquit()\n'
write(p / 'variant_errors.gd', error_script)
r, _ = run('variant-errors', p, p / 'variant_errors.gd')
check('Variant unsupported, unknown and cyclic values diagnosed', r['exit'] == 0 and len(r['errors']) == 3)
for name, text in [('object', '{}'), ('nonstring', '[42]'), ('malformed', '['), ('cache', '["res://.godot/cache.gd"]'), ('absolute', '["/tmp/x.gd"]')]:
    write(p / 'invalid.json', text)
    r, _ = run('manifest-'+name, p, HARNESS / 'check.gd', [p / 'invalid.json', 'project-policy'])
    check('reject manifest '+name, r['exit'] == 2 and not r['errors'] and r['envelopes'][0]['error']['stage'] == 'manifest')
r, _ = run('check-invalid-policy', p, HARNESS / 'check.gd', [p / 'manifest.json', 'unknown'])
check('reject invalid policy', r['exit'] == 2 and r['envelopes'][0]['error']['stage'] == 'arguments')
# Full file inventory: built-in importers, shader includes, and runtime custom loaders.
q = OUT / 'full-inventory'
write(q / 'project.godot', 'config_version=5\n[application]\nconfig/name="Full inventory"\n[autoload]\nLoader="*res://autoload.gd"\n')
write(q / 'autoload.gd', 'extends Node\nvar loader = preload("res://custom_loader.gd").new()\nfunc _ready():\n\tResourceLoader.add_resource_format_loader(loader, true)\nfunc _exit_tree():\n\tResourceLoader.remove_resource_format_loader(loader)\n')
write(q / 'custom_loader.gd', 'extends ResourceFormatLoader\nfunc _get_recognized_extensions() -> PackedStringArray:\n\treturn PackedStringArray(["gdkcustom"])\nfunc _handles_type(type: StringName) -> bool:\n\treturn type == &"Resource"\nfunc _get_resource_type(path: String) -> String:\n\treturn "Resource" if path.get_extension().to_lower() == "gdkcustom" else ""\nfunc _exists(_path: String) -> bool:\n\treturn false\nfunc _load(path: String, _original_path: String, _use_sub_threads: bool, _cache_mode: int):\n\tif not FileAccess.file_exists(path):\n\t\treturn ERR_FILE_NOT_FOUND\n\tvar resource := Resource.new()\n\tresource.resource_name = "custom"\n\treturn resource\n')
# One red pixel, 24-bit BMP with four-byte row alignment.
bmp = struct.pack('<2sIHHI', b'BM', 58, 0, 0, 54) + struct.pack('<IiiHHIIiiII', 40, 1, 1, 1, 24, 0, 4, 2835, 2835, 0, 0) + bytes([0, 0, 255, 0])
(q / 'pixel.BMP').write_bytes(bmp)
write(q / 'shared.gdshaderinc', 'const float TEST_VALUE = 1.0;\n')
write(q / 'asset.GDKCUSTOM', 'custom resource fixture\n')
write(q / 'notes.md', 'not a Godot resource\n')
write(q / 'data.json', '{"engine_recognizes_json":true}\n')
inventory = sorted('res://'+str(path.relative_to(q)) for path in q.rglob('*') if path.is_file())
manifest = OUT / 'full-inventory.json'
write(manifest, json.dumps(inventory))
r, _ = run('inventory-preimport', q, None, editor=True)
check('BMP fixture imported', r['exit'] == 0 and not r['errors'] and bool(list((q / '.godot/imported').glob('*.ctex'))))
r, _ = run('inventory-import-scan', q, HARNESS / 'import_scan.gd', [manifest], editor=True)
editor_extensions = r['envelopes'][0]['payload']['recognized_extensions']
check('editor public API recognizes BMP independent of runtime loaders', r['exit'] == 0 and not r['errors'] and 'bmp' in editor_extensions and 'gdkcustom' not in editor_extensions)
check('editor extensions canonical', editor_extensions == sorted(set(ext.lower() for ext in editor_extensions)))
handoff = OUT / 'editor-extensions.json'
write(handoff, json.dumps(editor_extensions))
for policy in ['project-policy', 'strict-methods']:
    r, _ = run('inventory-'+policy, q, HARNESS / 'check.gd', [manifest, policy, handoff])
    payload = r['envelopes'][0]['payload']
    recognized = payload['recognized_extensions']
    eligible = [path for path in inventory if path.rsplit('.', 1)[-1].lower() in recognized]
    check('engine inventory includes shaderinc custom and JSON '+policy, all(ext in recognized for ext in ['gdshaderinc', 'gdkcustom', 'json']))
    check('editor handoff makes BMP eligible '+policy, 'bmp' in recognized and 'res://pixel.BMP' in eligible)
    check('editor capabilities preserved in runtime union '+policy, set(editor_extensions).issubset(recognized))
    check('full inventory counts and skips nonresources '+policy, r['exit'] == 0 and not r['errors'] and payload['counts'] == dict(scripts=2, scenes=0, resources=4) and payload['failures'] == [] and len(eligible) == 6)
    check('custom loader exists false does not exclude load '+policy, 'res://asset.GDKCUSTOM' in eligible)
missing = ['res://missing.BMP', 'res://missing.gdshaderinc', 'res://missing.GDKCUSTOM', 'res://missing.md']
write(manifest, json.dumps(inventory + missing))
r, _ = run('inventory-missing', q, HARNESS / 'check.gd', [manifest, 'strict-methods', handoff])
payload = r['envelopes'][0]['payload']
check('recognized missing files attempted rather than exists filtered', r['exit'] == 1 and payload['counts'] == dict(scripts=2, scenes=0, resources=7) and payload['failures'] == sorted(missing[:3]))
write(OUT / 'bmp_runtime.gd', 'extends SceneTree\nfunc _initialize():\n\tassert(load("res://pixel.BMP") is Texture2D)\n\tassert("bmp" not in ResourceLoader.get_recognized_extensions_for_type(""))\n\tprint("BMP_LOADABLE_BUT_NOT_ENUMERATED")\n\tquit()\n')
r, stdout = run('bmp-runtime-registry-gap', q, OUT / 'bmp_runtime.gd')
check('BMP loads despite missing runtime extension registration', r['exit'] == 0 and not r['errors'] and 'BMP_LOADABLE_BUT_NOT_ENUMERATED' in stdout)
# Broken imported sources must remain classified even when import fails.
bad_import = OUT / 'invalid-import'
write(bad_import / 'project.godot', 'config_version=5\n[application]\nconfig/name="Invalid BMP"\n')
write(bad_import / 'broken.bmp', 'not a BMP file\n')
bad_manifest = OUT / 'invalid-import.json'
write(bad_manifest, '["res://broken.bmp", "res://project.godot"]')
r, _ = run('invalid-bmp-preimport', bad_import, None, editor=True)
check('invalid BMP fails import verdict', not r['timeout'] and bool(r['errors']) and any('broken.bmp' in error for error in r['errors']))
r, _ = run('invalid-bmp-import-scan', bad_import, HARNESS / 'import_scan.gd', [bad_manifest], editor=True)
check('failed BMP still in editor capability list', not r['timeout'] and len(r['envelopes']) == 1 and r['envelopes'][0]['payload']['scanned'] == 0 and 'bmp' in r['envelopes'][0]['payload']['recognized_extensions'])
bad_handoff = OUT / 'invalid-import-extensions.json'
write(bad_handoff, json.dumps(r['envelopes'][0]['payload']['recognized_extensions']))
r, _ = run('invalid-bmp-check', bad_import, HARNESS / 'check.gd', [bad_manifest, 'strict-methods', bad_handoff])
check('failed BMP counted and reported in fresh check', r['exit'] == 1 and r['envelopes'][0]['payload']['counts'] == dict(scripts=0, scenes=0, resources=1) and r['envelopes'][0]['payload']['failures'] == ['res://broken.bmp'])
for name, contents in [('object', '{}'), ('malformed', '['), ('number', '[42]'), ('empty', '[""]'), ('dot', '[".bmp"]'), ('path', '["../bmp"]'), ('space', '["bmp "]')]:
    write(OUT / 'bad-extensions.json', contents)
    r, _ = run('extensions-'+name, p, HARNESS / 'check.gd', [p / 'manifest.json', 'project-policy', OUT / 'bad-extensions.json'])
    check('reject invalid editor extensions '+name, r['exit'] == 2 and not r['errors'] and r['envelopes'][0]['error']['stage'] == 'extensions')
r, _ = run('extensions-missing-file', p, HARNESS / 'check.gd', [p / 'manifest.json', 'project-policy', OUT / 'absent-extensions.json'])
check('missing handoff file is tool error', r['exit'] == 2 and r['envelopes'][0]['error']['stage'] == 'extensions')
write(OUT / 'mixed-extensions.json', '["BMP","bmp"]')
r, _ = run('extensions-normalized', bad_import, HARNESS / 'check.gd', [bad_manifest, 'strict-methods', OUT / 'mixed-extensions.json'])
check('handoff extensions normalized and deduplicated', r['exit'] == 1 and r['envelopes'][0]['payload']['recognized_extensions'].count('bmp') == 1 and r['envelopes'][0]['payload']['failures'] == ['res://broken.bmp'])
# Dedicated cold projects avoid unrelated malformed scripts and ensure no warm cache.
for broken in [False, True]:
    for repeat in range(6):
        q = OUT / ('import-'+str(broken)+'-'+str(repeat))
        write(q / 'project.godot', 'config_version=5\n[application]\nconfig/name="Scan"\n[rendering]\nrenderer/rendering_method="gl_compatibility"\n')
        write(q / 'base.gd', 'class_name ContractBase\nextends RefCounted\n')
        write(q / 'child.gd', 'class_name ContractChild\nextends ContractBase\nvar icon = preload("res://icon.svg")\n')
        write(q / 'icon.svg', '<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="red"/></svg>')
        paths = ['res://base.gd', 'res://child.gd', 'res://icon.svg']
        if broken:
            write(q / 'broken.gd', 'extends Node\nfunc broken( ->:\n')
            paths += ['res://broken.gd']
        write(q / 'manifest.json', json.dumps(paths))
        if repeat >= 3:
            r, _ = run(q.name+'-preimport', q, None, editor=True)
            check(q.name+' preliminary import', r['exit'] == 0 and not r['errors'])
        r, _ = run(q.name, q, HARNESS / 'import_scan.gd', [q / 'manifest.json'], editor=True)
        check(q.name+' completed', r['exit'] == 0 and len(r['envelopes']) == 1 and r['envelopes'][0]['payload']['scanned'] == (3 if broken else 2) and 'svg' in r['envelopes'][0]['payload']['recognized_extensions'])
        check(q.name+' no cleanup errors', not any('RID allocations' in e for e in r['errors']))
        check(q.name+' validation errors', any('Parse Error' in e for e in r['errors']) == broken)
        cache = q / '.godot/global_script_class_cache.cfg'
        check(q.name+' cache persisted', cache.exists() and 'ContractChild' in cache.read_text())
        write(q / 'verify.gd', 'extends SceneTree\nfunc _initialize():\n\tvar child := ContractChild.new()\n\tassert(child.icon is Texture2D)\n\tprint("FRESH_CACHE_OK")\n\tquit()\n')
        r, stdout = run(q.name+'-fresh-cache', q, q / 'verify.gd')
        check(q.name+' fresh cache usable', r['exit'] == 0 and not r['errors'] and 'FRESH_CACHE_OK' in stdout)
        if not broken and repeat == 0:
            r, _ = run('import-invalid-manifest', q, HARNESS / 'import_scan.gd', [p / 'bad.json'], editor=True)
            check('import invalid manifest graceful error', r['exit'] == 2 and not r['errors'] and r['envelopes'][0]['error']['stage'] == 'manifest')
            write(q / 'missing.json', '["res://absent.gd"]')
            r, _ = run('import-missing-script', q, HARNESS / 'import_scan.gd', [q / 'missing.json'], editor=True)
            check('import missing script explicit load error', r['exit'] == 2 and r['envelopes'][0]['error']['stage'] == 'load' and not any('RID allocations' in e for e in r['errors']))
write(OUT / 'summary.json', json.dumps(dict(results=results, checks=checks), indent=2))
print(OUT)
print('checks:', sum(c['passed'] for c in checks), 'passed;', sum(not c['passed'] for c in checks), 'failed')
raise SystemExit(1 if any(not c['passed'] for c in checks) else 0)
