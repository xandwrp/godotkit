# Production harness contracts and real-engine evidence

Production scope: only `crates/gdproject/src/harness/{protocol,probe,import_scan,check,script_bootstrap}.gd`. No Rust changes or commits. Original spike scripts and README remain unchanged. This document is the handoff to the check/runner orchestrator.

## Invocation and wire contract

Place the selected harness alongside `protocol.gd` and invoke it with `--headless --path PROJECT --script HARNESS -- USER_ARGS`. ImportScan additionally requires `--editor`. Keep the preliminary `--editor --import` stage; the direct-cold experiments do not justify removing it.

All envelopes are single stdout lines beginning exactly `GDKIT_RESULT:` followed by JSON:

```json
{"protocol":1,"harness":"check","ok":true,"payload":{"counts":{"scripts":1,"scenes":1,"resources":1},"failures":[],"recognized_extensions":["gd","res","tres","tscn"]},"error":null}
```

The extension list above is illustrative, not a hardcoded engine list.

Errors have `ok:false`, `payload:null`, and `error:{stage,message,field}`. `field` is null when unspecified. Payloads are ordinary JSON, not implicitly Variant-encoded by `emit_ok`; Variant-valued fields must explicitly call `Protocol.encode`.

| Harness name | User arguments | Success payload | Exit behavior |
| --- | --- | --- | --- |
| `probe` | none used | `{version:String,major:int,editor:bool}` | 0 after directory/resource checks; 1 with `resource` error envelope on failure |
| `import_scan` | arg0: manifest **file path** | `{scanned:int,recognized_extensions:[String]}` | 0 on completed scan/load work; 2 with operational error envelope |
| `check` | arg0: manifest **file path**; arg1: exactly `strict-methods` or `project-policy`; arg2: **editor extensions JSON file path (required)** | `{counts:{scripts:int,scenes:int,resources:int},failures:[res paths],recognized_extensions:[String]}` | 0 if failures empty, 1 otherwise; `ok:true` in both cases. Bad arguments/manifest: 2 with error envelope |
| `script_bootstrap` | exactly one project-script path | **No success envelope** | user owns quit/code after startup; argument errors exit 2; load/base errors exit 1 |

### Shared manifest shape

The file contains the **full project file inventory**, without a Rust-side resource extension allowlist, as a JSON array of strings, e.g.:

```json
["res://base.gd", "res://child.gd", "res://level.tscn", "res://settings.tres", "res://pixel.BMP", "res://shared.gdshaderinc", "res://notes.md", "res://project.godot"]
```

No wrapper object. Both harnesses use the same validator. Reject non-arrays, non-string entries, non-`res://` paths, empty/`.`/`..`/`.godot` components, backslashes, and control characters. Empty arrays are valid. Entries are not deduplicated; counts reflect attempted entries. The manifest file itself may be an absolute filesystem path or a path understood by FileAccess.

### ImportScan → Check typed handoff (final)

ImportScan still takes exactly **one** user argument. Its success payload is now:

```text
{scanned: integer, recognized_extensions: string[]}
```

`recognized_extensions` is the sorted, deduplicated, lowercase result of the public `ResourceLoader.get_recognized_extensions_for_type("")` API **in the initialized editor**, where importers are registered. It includes loader and importer extensions, without leading dots; it is capability metadata, not a list of successfully imported files. The real 4.7.2 editor returns `bmp`, `png`, `svg`, audio/model/font source extensions, `gdshaderinc`, and others. No private importer API or sidecar-success heuristic is needed.

The orchestrator serializes **only that array**, unchanged, to an artifact JSON file, e.g. `editor-extensions.json`. It then invokes Check with exactly **three** user arguments:

```text
--script check.gd -- /absolute/files.json strict-methods /absolute/editor-extensions.json
```

Check arg0 remains the full-file inventory JSON array; arg1 is the policy; **arg2 is the required file path, not inline JSON or an envelope**. No new ImportScan argument, output-file side effect, environment variable or per-file metadata is required. A minimal fake handoff can be `["bmp","gd","gdshaderinc","res","scn","tres","tscn"]`; it need not imitate the entire engine registry, but must be consistent with that fake's expected inventory.

Check validates arg2 as a JSON array of nonempty extension strings, rejecting dots, slash/backslash, colon, whitespace/control characters. It normalizes case and deduplicates entries. Missing/unreadable file, bad JSON, wrong shape or invalid entries produce `ok:false`, `error.stage:"extensions"`, `error.field:arg2`, and exit **2**. Missing arg2 is an `arguments` error, exit 2. An explicit empty array is accepted for focused runtime-only callers, but the full check pipeline must supply the real ImportScan list; do not silently default a missing handoff to `[]`.

After runtime autoload initialization, Check unions the handoff with its runtime `ResourceLoader.get_recognized_extensions_for_type("")`. Its existing `recognized_extensions` payload field now means that **sorted, unique, lowercase union**. This preserves editor importer eligibility and adds custom loaders registered by runtime autoloads. Rust filters the original full inventory by this returned union and validates counts/failure membership. Missing/corrupt `.bmp` paths remain eligible without consulting `exists()` or import/load success.

Require clean preliminary-import diagnostics before advancing the normal pipeline. Corrupt BMP was observed to produce `ERROR: Error loading image: 'res://broken.bmp'.` and `ERROR: Error importing 'res://broken.bmp'.` with **exit 0**. That is an import failure verdict, not successful import. The experiment deliberately continued afterward to prove the failed source remains in the capability list and fails a fresh Check load.

### Probe

Requires `res://project.godot` and a loadable `res://probe.tres`. Opens/lists `res://` via DirAccess and loads the resource with `CACHE_MODE_IGNORE`. `editor` is `OS.has_feature("editor")`, meaning editor-capable executable, **not** whether this process was launched with `--editor`. The caller decides version compatibility from the payload; reporting a different major is not hidden as a generic load failure.

### Check

- Applies strict warning policy in `_init`, before autoload parsing/instantiation: `debug/gdscript/warnings/enable=true`, `debug/gdscript/warnings/unsafe_method_access=2`, then emits `settings_changed`.
- `project-policy` leaves those settings unchanged.
- Defers manifest processing until autoload initialization.
- Requires the editor extension handoff in arg2 and unions it with `ResourceLoader.get_recognized_extensions_for_type("")` in the deferred check after autoload initialization, so custom loaders registered by autoload `_ready` participate. Returns the union lowercased, deduplicated and lexically sorted as `recognized_extensions` (extensions have no leading dot).
- Validates the entire manifest, sorts `.gd` entries first then lexically, and attempts only entries whose lowercase final extension occurs in `recognized_extensions`. It does **not** use `ResourceLoader.exists`, `FileAccess.file_exists`, or load success to decide eligibility. Missing or corrupt recognized files must still count and can fail.
- Classification of eligible entries is unchanged: `.gd` = scripts, `.tscn`/`.scn` = scenes, everything else = resources. Counts include attempted eligible entries, not successful loads. Unrecognized files are ignored; duplicate manifest entries remain duplicate attempts.
- Rust must derive the same eligible inventory using the returned extension list, apply the same classification, compare all counts, and require each failure path to belong to that eligible inventory. Never keep a separate hardcoded extension allowlist or filter by resource existence.

**Resolved engine-registry limitation (4.7.2):** the runtime-only extension query omits imported source extensions such as `bmp`. The editor query includes them, so the mandatory arg2 handoff supplies that capability metadata without consulting import success. `json` is also recognized by this engine and must not be assumed to be a non-resource extension. No Rust code was changed here; callers must adopt the three-argument contract.
- Uses `CACHE_MODE_REPLACE` in a fresh runtime process. Retains loaded resources through the pass; clears references before reporting.
- Does not explicitly instantiate scripts or PackedScenes. Autoloads still run normally. Resource loading itself can execute resource/static initialization; this is not a sandbox or a promise of zero user-code execution.
- Failures include null resources and non-instantiable, non-abstract Scripts. Valid abstract scripts are accepted. Dependencies can fail while their parent resource remains non-null, so failures are not a complete validation verdict.
- Stages: `arguments`, `manifest`, `extensions`. Manifest errors put arg0 in `field`; handoff errors put arg2 in `field`.

### ImportScan

- Deferred entry, yield one frame and wait while filesystem **scanning OR importing**, explicit `scan()`, then yield/wait again.
- Validates the whole shared manifest; loads only `.gd` entries using `CACHE_MODE_REPLACE`. Non-script entries are ignored and not counted.
- Reports the public editor extension registry as `recognized_extensions`, independently of whether source files imported successfully. A broken imported source cannot disappear from eligibility because its importer failed.
- Never uses `can_instantiate()` as an editor-mode validity test.
- Null Script loads are operational `load` error envelopes (paths in message), exit 2. Malformed scripts can still return non-null, emit parse errors and produce a successful completion envelope/exit 0.
- Stages: `arguments`, `manifest`, `editor`, `load`.
- Error shutdown also waits for initial scanning/importing to finish before deferred quit; no fixed sleeps, no `--quit-after` completion inference.
- Require a valid completion, normal exit 0, no timeout, **no errors in either complete stream**, and post-exit class-cache audit. Do not ignore diagnostics after the envelope. Fresh runtime loading remains necessary.

### ScriptBootstrap

- Deferred load lets autoload singleton names and `_ready` state resolve.
- Requires an instantiable Script with native base `SceneTree`; inherited SceneTree scripts are permitted.
- Pre-handoff errors: `arguments`, `load`, `base`; load/base errors put the requested script path in `field`. No startup marker on these failures.
- Prints exact line `GDKIT_SCRIPT_STARTED`, then `set_script(target)`, then explicitly calls `_initialize()`.
- The marker precedes user `_init` executed by `set_script`, **not** static initializers executed while loading the resource.
- Never prints a success envelope and never schedules automatic quit after handoff.
- Require exactly one marker, normal exit 0, no timeout, no error envelope, and no engine errors from either complete stream. Marker alone is not success. Runtime errors may coexist with exit 0 or continue until timeout. Enforce process-group deadlines outside Godot.

## Editor teardown root cause and fix

`--editor --script` in the tested 4.7.2 engine allocates an initial default SceneTree and subsequently overwrites the main-loop pointer with the script's new SceneTree. The initial tree is not destroyed and remains `SceneTree::singleton` (the constructor only assigns the singleton when null). This leaks its root and directs orphan-node deletion to the wrong tree.

Evidence: two SceneTree construction traces in verbose startup; upstream `main/main.cpp` (`Main::start`) and `scene/main/scene_tree.cpp` (constructor, destructor and finalize); a controlled real-engine before/after experiment. Upstream master source inspection supports the diagnosis but is not claimed to be a source checkout of the installed package.

Immediate quit, deferred quit, extra frames, editor close notifications, and early editor-node deletion all retained the leak. Merely emitting `root.close_requested` timed out. No suppression policy was added.

The production workaround is in **ImportScan `_static_init`**, before Godot constructs the harness tree:

1. Only Godot **4.7.2** and editor hint, with no installed engine main loop.
2. Inspect orphan Window nodes named `root`, with no children.
3. Use the root's `close_requested` connection to identify its owning SceneTree; require that tree's root is exactly this node and that the tree has no script.
4. Free that unused placeholder. Its destructor clears the singleton; the subsequently constructed harness tree becomes the correct singleton.

This uses exposed APIs rather than deleting arbitrary orphan nodes or suppressing output. It skips normal editor loading of the harness when a main loop already exists. Other engine versions are **not** silently exempted from errors; this workaround needs separate verification before broadening its version guard. Do not move it into the shared protocol or runtime harnesses.

Comparison evidence: `artifacts/teardown-20260924-190128/results.json` — placeholder cleanup exits 0 with empty stderr and no engine errors; immediate quit still produces all five RID errors plus Canvas/CanvasItem/ObjectDB warnings. Earlier exploratory runs (including two syntax-error experiments) are retained, not treated as passing evidence.

## Variant grammar decisions and limits

Encoder covers plain scalars, the ±(2^53-1) integer boundary and tagged decimal i64 extremes; tagged integral floats and tagged `nan`, `inf`, `-inf`, `-0.0`; StringName/NodePath; every vector, integer vector, rect, Color, Plane, Quaternion, AABB, Basis, Transform2D/3D, Projection; every Packed*Array including PackedVector4Array; ordinary and built-in typed Arrays; dictionaries; external and inline native/scripted Resources.

- Numeric structures have flat component arrays. Matrices use column order; Transform3D is Basis columns then origin.
- Packed arrays contain recursively encoded elements; packed vectors/colors therefore contain tagged vector/color values. Components whose type the tag fixes (tuple and packed numeric components) stay bare even when integral.
- Non-string-key dictionaries are `{"$variant":{"type":"Dictionary","value":[[key,value],...]}}`. String dictionaries containing reserved keys `$variant`, `$ref`, `$resource` use that representation too, avoiding ambiguity.
- Typed Arrays use `{"$variant":{"type":"Array","element":"Vector2","value":[...]}}`.
- Native type names follow Godot spelling (`Nil`, `bool`, `int`, `float`, `AABB`, `RID`, etc.). Unknown `type_from_name` returns -1, never TYPE_NIL.
- External Resources with valid `res://` file paths use `$ref`; built-in subresources with `::` paths encode inline. Inline resources use exactly one of `class` or `script`, plus storage properties excluding `script`/`resource_path`. A built-in (`::`) script is rejected on encode, and `::` paths are rejected in `$ref` and `script` on decode.
- Non-Resource Objects, RID, Callable and Signal emit errors: these process-local identities cannot be transported losslessly. Object/class/script-typed Arrays are explicitly rejected because the documented element-name grammar does not preserve that metadata. Typed Dictionary metadata is not represented by the documented grammar.
- Cycles are caught by identity: the encoder tracks the Arrays, Dictionaries and inline Resources on the current path and fails with "cyclic reference" as soon as one recurs (`a.append(a)` fails in under a millisecond). Depth limit 32 bounds nesting, and a global budget of 100000 visited values per encode/decode call (not per container) bounds shared subgraphs, e.g. a 4-way DAG 30 levels deep. `gdview::variant::Limits` applies the same depth and budget accounting. Encoding stops at the first error; `try_encode`/`try_decode` return it instead of emitting a diagnostic.
- `decode` validates the grammar (component counts and ranges, canonical tagged ints, tag fields, duplicate keys, typed array items), but Resource construction can execute scripts, so it is for **trusted** specs, not an untrusted sandbox. Unknown/unsupported tags emit engine errors.
- Godot JSON parses every number as float, so bare numbers are classified by value on both sides: integral values within ±(2^53-1) are int, fractional values float, and bare integral values beyond that are rejected as ambiguous. To keep float and int distinct, encoders tag integral floats (`{"type":"float","value":3.0}`) and use tagged strings for `nan`, `inf`, `-inf` and `-0.0` (whose sign `JSON.stringify` drops). Non-finite floats are accepted only in that tagged string form, by both codecs. Harness output escapes the control characters `JSON.stringify` leaves raw.
- `protocol_golden.json` (byte-identical to `crates/gdproject/tests/fixtures/protocol_golden.json`) is now produced by `real_engine_protocol_gd_matches_golden_fixture` in `crates/gdproject/tests/protocol.rs` (`GDKIT_REFRESH_GOLDEN=1` rewrites both copies). It has 66 cases, including integral and signed-zero floats and control characters, plus Godot's `type_string` table. Offline, `gdview::variant` decodes every case and re-encodes it to identical JSON. The 47-case `variant_contracts.gd` driver here predates that grammar change, so the Python driver's golden comparison no longer matches the fixture.

## Final validation

Engine: `/usr/bin/godot`, `4.7.2.stable.arch_linux.ed1daf0bf`.

```sh
python3 spike-real-engine-check-contracts/production_contracts.py
```

Final editor-to-runtime handoff run: **120 checks passed, 0 failed; 73 engine invocations**, in `artifacts/production-20260924-194843/summary.json`. BMP is now included in Check eligibility, counted, and loaded. Corrupt BMP fails the preliminary import diagnostic verdict and remains eligible/fails in a fresh Check attempt. Prior evidence remains in `production-20260924-194454` (runtime-only registry gap, 104 checks) and `production-20260924-190908` (initial production/golden baseline, 91 checks). Every invocation has exact argv, exit/timeout, parsed envelopes/errors, and complete separate stdout/stderr logs. Fixtures and caches are retained. The script uses isolated HOME/XDG paths and process-group kill/reap on deadlines, and exits nonzero on assertion failures.

Coverage includes:

- Probe resource/directory capabilities.
- Check counts without script/scene instantiation, strict vs project policy, policy applied before autoload parsing, abstract scripts, failed paths, invalid manifests/policy.
- Full-file inventory with uppercase `.BMP`, `.gdshaderinc`, uppercase custom-loader resource, `.json`, `.md`, and `project.godot`. Custom loader registered during autoload `_ready`; its `_exists` deliberately returns false while `_load` succeeds. Missing recognized files still count and fail. Both policy modes tested. Editor ImportScan capabilities flow through a JSON artifact into Check: BMP imports/loads successfully and is included; corrupt BMP fails import and is counted/reported as a failed Check load. Runtime-only BMP omission remains a separate regression fixture explaining why the handoff is necessary. Handoff validation covers malformed JSON, wrong shapes, empty/non-string entries, dots/paths/whitespace, missing file, and case normalization/deduplication.
- Frozen 47-case protocol golden comparison independent of `gdview::variant`, with actual envelope preserved as `protocol_actual.json`. No golden regeneration during tests.
- Bootstrap autoload-ready access, marker-before-init, invalid base/parse error, exit 7, runtime error with exit 0, never-quit timeout.
- **12 independent cold imports**: direct scan and preliminary import + scan, valid/broken projects, three repetitions each. Persisted global class inheritance and SVG imports verified in new runtime processes. All 12 scans have no RID errors or leak warnings. Broken scripts retain parse errors. Error-path import teardown also has no warnings in the final run.
- **47 Variant JSON encode/decode/encode round trips**, including native and scripted inline Resources, all packed arrays, matrices, reserved-key dictionaries and i64 extremes; all type-table entries round trip.
- Unsupported RID, unknown tag and cyclic input each emit a diagnostic.

`gdformat --check` on all five production scripts: **5 unchanged**. `git diff --check -- crates/gdproject/src/harness`: clean. No Cargo tests were run for this GDScript-only scope.

Earlier production runs, including the initial six teardown failures, one assertion timeout while refining scalar decoding, and one temporary indentation regression, were not rewritten as passing results. The final evidence supersedes them. The first full-inventory experiment (`production-20260924-194201`) had three failed assertions caused by the incorrect assumption that runtime registration included BMP and excluded JSON. Its logs remain intact; the follow-up explicitly tested the observed registry behavior. The final editor-to-runtime handoff resolves that eligibility gap using the public editor registry.
