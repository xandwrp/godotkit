#!/usr/bin/env python3
"""Real Godot contract experiments; all generated files stay beside this driver."""
import argparse
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import time

ROOT = Path(__file__).resolve().parent
ERROR = re.compile(r"(?m)^\s*(?:SCRIPT ERROR:|ERROR:|USER ERROR:)[^\n]*")
CLEANUP = re.compile(r"^ERROR: \d+ RID allocations of type '.+' were leaked at exit\.$")
START = "GDKIT_SCRIPT_STARTED"


def write(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--engine", default="/usr/bin/godot")
    parser.add_argument("--repeats", type=int, default=3)
    args = parser.parse_args()
    if not 1 <= args.repeats <= 10:
        parser.error("repeats must be 1..10")
    run = ROOT / "artifacts" / time.strftime("%Y%m%d-%H%M%S")
    run.mkdir(parents=True, exist_ok=False)
    # Isolate Godot's user data and editor configuration as well as project caches.
    env = dict(os.environ, NO_COLOR="1", HOME=str(run / "home"),
               XDG_DATA_HOME=str(run / "data"), XDG_CONFIG_HOME=str(run / "config"),
               XDG_CACHE_HOME=str(run / "cache"))
    for key in ("HOME", "XDG_DATA_HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME"):
        Path(env[key]).mkdir()
    results = []
    checks = []

    def check(name, condition):
        checks.append({"name": name, "passed": bool(condition)})
        print(("PASS " if condition else "FAIL ") + name, flush=True)

    def invoke(name, project=None, extra=(), deadline=15):
        command = [args.engine]
        if project is not None:
            command += ["--headless", "--no-header", "--path", str(project)]
        command += list(extra)
        before = time.monotonic()
        child = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                 env=env, start_new_session=True)
        timed_out = False
        try:
            stdout, stderr = child.communicate(timeout=deadline)
        except subprocess.TimeoutExpired:
            timed_out = True
            os.killpg(child.pid, signal.SIGKILL)
            stdout, stderr = child.communicate(timeout=3)
        stdout, stderr = stdout.decode(errors="replace"), stderr.decode(errors="replace")
        write(run / (name + ".stdout.log"), stdout)
        write(run / (name + ".stderr.log"), stderr)
        result = {"name": name, "argv": command, "exit": child.returncode,
                  "timed_out": timed_out, "elapsed_seconds": round(time.monotonic() - before, 3),
                  "engine_errors": ERROR.findall(stdout + "\n" + stderr),
                  "marker_count": stdout.splitlines().count(START)}
        result["script_success"] = (not timed_out and child.returncode == 0
                                     and result["marker_count"] == 1 and not result["engine_errors"])
        results.append(result)
        return result, stdout, stderr

    version, out, _ = invoke("version", extra=["--version"], deadline=5)
    check("engine version", version["exit"] == 0 and "4.7.2" in out)
    for pipeline in (False, True):
        for broken in (False, True):
            for repeat in range(args.repeats):
                name = f"import-{'pipeline' if pipeline else 'direct'}-{'broken' if broken else 'valid'}-{repeat + 1}"
                project = run / name
                write(project / "project.godot", 'config_version=5\n[application]\nconfig/name="Import spike"\n[rendering]\nrenderer/rendering_method="gl_compatibility"\n')
                write(project / "base.gd", 'class_name SpikeBase\nextends RefCounted\nvar answer := 42\n')
                write(project / "child.gd", 'class_name SpikeChild\nextends SpikeBase\n')
                write(project / "consumer.gd", 'extends Node\nvar value: SpikeChild = SpikeChild.new()\nvar icon = preload("res://icon.svg")\n')
                write(project / "icon.svg", '<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="red"/></svg>\n')
                paths = ["res://base.gd", "res://child.gd", "res://consumer.gd"]
                if broken:
                    write(project / "broken.gd", 'class_name SpikeBroken\nextends RefCounted\nfunc broken( -> void:\n\tpass\n')
                    paths.append("res://broken.gd")
                manifest = run / (name + ".manifest.json")
                write(manifest, json.dumps(paths))
                check(name + " starts cold", not (project / ".godot").exists())
                if pipeline:
                    pre, _, _ = invoke(name + "-preimport", project, ["--editor", "--import"])
                    check(name + " preimport exits without validating broken script", not pre["timed_out"] and pre["exit"] == 0 and not pre["engine_errors"])
                result, stdout, _ = invoke(name, project, ["--editor", "--script", str(ROOT / "import_scan.gd"), "--", str(manifest)])
                envelopes = [json.loads(line.removeprefix("GDKIT_RESULT:")) for line in stdout.splitlines() if line.startswith("GDKIT_RESULT:")]
                payload = envelopes[0]["payload"] if len(envelopes) == 1 else {}
                result["payload"] = payload
                cache = project / ".godot/global_script_class_cache.cfg"
                text = cache.read_text() if cache.exists() else ""
                write(run / (name + ".class-cache.cfg"), text)
                result["cache_after_exit"] = text
                check(name + " settled and completed", not result["timed_out"] and result["exit"] == 0 and payload.get("scanned") == len(paths) and payload.get("scanning") is False and payload.get("importing") is False)
                check(name + " cache persisted", all(token in text for token in ('&"SpikeBase"', '&"SpikeChild"', '"res://base.gd"', '"res://child.gd"')))
                check(name + " asset imported", bool(list((project / ".godot/imported").glob("*.ctex"))))
                result["cleanup_errors"] = [e for e in result["engine_errors"] if CLEANUP.fullmatch(e.strip())]
                result["validation_errors"] = [e for e in result["engine_errors"] if not CLEANUP.fullmatch(e.strip())]
                check(name + " failure distinguished from completion", bool(result["validation_errors"]) == broken and (not broken or any("Parse Error:" in e for e in result["validation_errors"])))
                # Verify the cache in a new runtime, not merely that its file exists.
                write(project / "verify_cache.gd", 'extends SceneTree\nfunc _initialize() -> void:\n\tvar item := SpikeChild.new()\n\tassert(item.answer == 42)\n\tassert(load("res://icon.svg") is Texture2D)\n\tprint("CACHE_USABLE")\n\tquit(0)\n')
                verify, output, _ = invoke(name + "-verify-cache", project, ["--script", "res://verify_cache.gd"])
                check(name + " cache usable in fresh runtime", not verify["timed_out"] and verify["exit"] == 0 and not verify["engine_errors"] and "CACHE_USABLE" in output)

    project = run / "scripts"
    write(project / "project.godot", 'config_version=5\n[autoload]\nSettings="*res://settings.gd"\n')
    write(project / "settings.gd", 'extends Node\nvar enabled := false\nfunc _ready() -> void:\n\tenabled = true\n\tprint("AUTOLOAD_READY")\n')
    fixtures = {
        "normal": 'extends SceneTree\nfunc _initialize() -> void:\n\tprint("USER_INITIALIZE")\n\tquit(0)\n',
        "bad_base": 'extends Node\nfunc _initialize() -> void:\n\tprint("MUST_NOT_RUN")\n',
        "runtime_error": 'extends SceneTree\nfunc _initialize() -> void:\n\tprint("USER_INITIALIZE")\n\tcall_deferred("finish")\n\tvar missing: Object = null\n\tmissing.call("explode")\nfunc finish() -> void:\n\tquit(0)\n',
        "never_quit": 'extends SceneTree\nfunc _initialize() -> void:\n\tprint("USER_INITIALIZE")\n',
        "autoload": 'extends SceneTree\nfunc _initialize() -> void:\n\tprint("USER_INITIALIZE")\n\tassert(Settings.enabled)\n\tassert(root.get_node("Settings") == Settings)\n\tprint("AUTOLOAD_CONFIRMED")\n\tquit(0)\n',
        "nonzero": 'extends SceneTree\nfunc _initialize() -> void:\n\tprint("USER_INITIALIZE")\n\tquit(7)\n',
        "parse_error": 'extends SceneTree\nfunc broken( -> void:\n\tpass\n',
        "runtime_error_no_quit": 'extends SceneTree\nfunc _initialize() -> void:\n\tprint("USER_INITIALIZE")\n\tvar missing: Object = null\n\tmissing.call("explode")\n',
        "inherited": 'extends "res://normal.gd"\n',
        "init_order": 'extends SceneTree\nfunc _init() -> void:\n\tprint("USER_INIT")\nfunc _initialize() -> void:\n\tprint("USER_INITIALIZE")\n\tquit(0)\n',
    }
    for name, source in fixtures.items():
        write(project / (name + ".gd"), source)
    for name in fixtures:
        result, stdout, _ = invoke("script-" + name, project, ["--script", str(ROOT / "script_bootstrap.gd"), "--", "res://" + name + ".gd"], deadline=3)
        if name in ("bad_base", "parse_error"):
            check(name + " rejected before handoff", result["exit"] == 1 and not result["timed_out"] and result["marker_count"] == 0 and '"ok":false' in stdout and "MUST_NOT_RUN" not in stdout)
        else:
            check(name + " marker before initialize", result["marker_count"] == 1 and "USER_INITIALIZE" in stdout and stdout.index(START) < stdout.index("USER_INITIALIZE"))
        check(name + " success contract", result["script_success"] == (name in ("normal", "autoload", "inherited", "init_order")))
        if name in ("never_quit", "runtime_error_no_quit"):
            check(name + " deadline enforced", result["timed_out"])
        if name == "runtime_error":
            check("runtime error with exit zero fails", result["exit"] == 0 and bool(result["engine_errors"]))
        if name == "runtime_error_no_quit":
            check("runtime error does not quit engine", bool(result["engine_errors"]) and result["timed_out"])
        if name == "nonzero":
            check("user owns exit code", result["exit"] == 7)
        if name == "autoload":
            check("autoload ready before handoff", "AUTOLOAD_CONFIRMED" in stdout and stdout.index("AUTOLOAD_READY") < stdout.index(START))
        if name == "init_order":
            check("marker before set_script init", "USER_INIT\n" in stdout and stdout.index(START) < stdout.index("USER_INIT\n") < stdout.index("USER_INITIALIZE"))
    summary = {"engine_version": out.strip(), "results": results, "checks": checks,
               "passed": sum(c["passed"] for c in checks), "failed": sum(not c["passed"] for c in checks)}
    write(run / "summary.json", json.dumps(summary, indent=2) + "\n")
    print(f"Artifacts: {run}\nChecks: {summary['passed']} passed, {summary['failed']} failed", flush=True)
    return int(summary["failed"] != 0)


if __name__ == "__main__":
    raise SystemExit(main())
