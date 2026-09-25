// Acceptance tests for gdproject::protocol. Offline, except the `real_engine_`
// tests, which run harness/protocol.gd in GDKIT_TEST_GODOT.
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gdproject::process::{self, Spawn};
use gdproject::protocol::{
    Envelope, HarnessError, PROTOCOL_VERSION, ProtocolError, RESULT_PREFIX, parse_envelope,
};
use gdproject::runner::PROTOCOL_SOURCE;
use gdview::variant::{Limits, VariantJson, VariantType};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

/// Value cases in the frozen golden; see `VARIANT_CONTRACTS`.
const GOLDEN_CASES: usize = 66;

#[derive(Debug, Deserialize, PartialEq)]
struct Payload {
    count: u32,
    label: String,
}

fn parse<T: DeserializeOwned>(lines: &[&str]) -> Result<Envelope<T>, ProtocolError> {
    parse_envelope(lines.iter().map(|line| (*line).to_owned()))
}

fn result_line(value: Value) -> String {
    format!("{RESULT_PREFIX}{value}")
}

#[test]
fn parse_envelope_finds_the_single_result_line_among_noise() {
    let line = result_line(json!({
        "protocol": PROTOCOL_VERSION, "harness": "probe", "ok": true,
        "payload": {"count": 3, "label": "héllo 世界\nGDKIT_RESULT:"}
    }));
    let envelope = parse::<Payload>(&[
        "Godot Engine v4",
        "",
        "WARNING: noisy engine",
        &line,
        "engine shutting down",
    ])
    .unwrap();
    assert_eq!(
        envelope,
        Envelope {
            protocol: PROTOCOL_VERSION,
            harness: "probe".into(),
            ok: true,
            payload: Some(Payload {
                count: 3,
                label: "héllo 世界\nGDKIT_RESULT:".into()
            }),
            error: None,
        }
    );
}

#[test]
fn parse_envelope_rejects_missing_duplicate_and_malformed_results() {
    assert!(matches!(parse::<Value>(&[]), Err(ProtocolError::Missing)));
    assert!(matches!(
        parse::<Value>(&["engine output"]),
        Err(ProtocolError::Missing)
    ));
    let valid = result_line(json!({"protocol": 1, "harness": "probe", "ok": true}));
    for lines in [
        vec![valid.as_str(), valid.as_str()],
        vec!["GDKIT_RESULT:{", valid.as_str()],
        vec![valid.as_str(), "GDKIT_RESULT:"],
    ] {
        assert!(matches!(
            parse::<Value>(&lines),
            Err(ProtocolError::Multiple)
        ));
    }
    for body in [
        "",
        "{",
        "not json",
        "null",
        "[]",
        "{}",
        "true",
        "{\"protocol\":1} trailing",
    ] {
        let line = format!("{RESULT_PREFIX}{body}");
        assert!(
            matches!(parse::<Value>(&[&line]), Err(ProtocolError::Malformed(message)) if !message.is_empty()),
            "{body}"
        );
    }
}

#[test]
fn parse_envelope_rejects_protocol_version_mismatch_before_payload_decode() {
    for version in [0, PROTOCOL_VERSION + 1, u32::MAX] {
        // Both metadata and payload are incompatible with the current contract.
        let line =
            result_line(json!({"protocol": version, "harness": 99, "ok": "yes", "payload": []}));
        assert!(
            matches!(parse::<Payload>(&[&line]), Err(ProtocolError::VersionMismatch { expected: PROTOCOL_VERSION, found }) if found == version)
        );
    }
    let line = result_line(
        json!({"protocol": 2, "harness": "probe", "ok": true, "payload": {"count": "wrong"}}),
    );
    assert!(matches!(
        parse::<Payload>(&[&line]),
        Err(ProtocolError::VersionMismatch { found: 2, .. })
    ));
    let line = result_line(json!({"protocol": 2}));
    assert!(matches!(
        parse::<Payload>(&[&line]),
        Err(ProtocolError::VersionMismatch { found: 2, .. })
    ));
}

#[test]
fn error_envelopes_surface_stage_and_message() {
    for field in [None, Some("settings/name"), Some("")] {
        let line = result_line(json!({
            "protocol": 1, "harness": "resource_create", "ok": false,
            "error": {"stage": "validate", "message": "invalid value: 世界", "field": field}
        }));
        let envelope = parse::<Payload>(&[&line]).unwrap();
        assert!(!envelope.ok);
        assert_eq!(envelope.harness, "resource_create");
        assert_eq!(envelope.payload, None);
        assert_eq!(
            envelope.error,
            Some(HarnessError {
                stage: "validate".into(),
                message: "invalid value: 世界".into(),
                field: field.map(str::to_owned),
            })
        );
    }
    let line = result_line(
        json!({"protocol": 1, "harness": "check", "ok": false, "payload": null, "error": {"stage": "load", "message": "failed"}}),
    );
    assert_eq!(
        parse::<Payload>(&[&line]).unwrap().error.unwrap().field,
        None
    );
}

#[test]
fn documented_variant_json_shapes_survive_envelope_transport() {
    // Handwritten contract examples, separate from the engine golden below.
    // This tests transport, not Variant encoding or decoding.
    let payload = json!([
        null, true, false, 42, -42, 1.25, "hello", [], {},
        {"nested": [1, {"value": "text"}]},
        {"$variant": {"type": "int", "value": "9223372036854775807"}},
        {"$variant": {"type": "int", "value": "-9223372036854775808"}},
        {"$variant": {"type": "Vector3", "value": [1.0, 2.0, 3.0]}},
        {"$variant": {"type": "Color", "value": [1.0, 0.5, 0.0, 1.0]}},
        {"$variant": {"type": "Array", "element": "Vector2", "value": [
            {"$variant": {"type": "Vector2", "value": [1.0, 2.0]}}
        ]}},
        {"$variant": {"type": "PackedByteArray", "value": [0, 255]}},
        {"$variant": {"type": "float", "value": "nan"}},
        {"$variant": {"type": "float", "value": "inf"}},
        {"$variant": {"type": "float", "value": "-inf"}},
        {"$ref": "res://assets/test.tres"},
        {"$resource": {"class": "Resource", "properties": {"name": "test"}}},
        {"$resource": {"script": "res://item.gd", "properties": {}}}
    ]);
    let line = result_line(
        json!({"protocol": 1, "harness": "resource_schema", "ok": true, "payload": payload}),
    );
    assert_eq!(parse::<Value>(&[&line]).unwrap().payload, Some(payload));
}

#[test]
fn engine_golden_variant_payload_survives_envelope_transport() {
    // Engine output frozen by real_engine_protocol_gd_matches_golden_fixture;
    // spike-real-engine-check-contracts/protocol_golden.json is a byte-identical
    // copy. The local copy keeps this offline test independent of the spike and
    // a Godot executable. These are JSON transport assertions; the Rust decoder
    // is checked against the same fixture below.
    let fixture = include_str!("fixtures/protocol_golden.json");
    let golden: Value = serde_json::from_str(fixture).unwrap();
    // Remove pretty-print line whitespace without reserializing the JSON values;
    // the harness wire format is a single prefixed line.
    let compact: String = fixture.lines().map(str::trim).collect();
    let line = format!("{RESULT_PREFIX}{compact}");
    assert_eq!(line.lines().count(), 1);
    let envelope =
        parse::<Value>(&["engine startup noise", &line, "engine shutdown noise"]).unwrap();

    assert_eq!(envelope.protocol, PROTOCOL_VERSION);
    assert_eq!(envelope.harness, "variant_contracts");
    assert!(envelope.ok);
    assert_eq!(envelope.error, None);
    let payload = envelope.payload.as_ref().unwrap();
    let encoded = payload["encoded"].as_array().unwrap();
    assert_eq!(payload["cases"], encoded.len());
    assert_eq!(encoded.len(), GOLDEN_CASES);
    assert_eq!(payload["failures"], json!([]));
    assert_eq!(payload, &golden["payload"]);
    assert_eq!(serde_json::to_value(&envelope).unwrap(), golden);

    // Pin precision-sensitive and nested representations from the engine output.
    for decimal in [
        "9007199254740993",
        "9007199254740992",
        "-9223372036854775808",
        "9223372036854775807",
    ] {
        assert!(encoded.contains(&json!({"$variant": {"type": "int", "value": decimal}})));
    }
    assert!(encoded.contains(&json!(9007199254740991_i64)));
    for special in ["nan", "inf", "-inf", "-0.0"] {
        assert!(encoded.contains(&json!({"$variant": {"type": "float", "value": special}})));
    }
    // Integral floats are tagged so readers do not take them for ints.
    for integral in [json!(3.0), json!(0.0), json!(1e20)] {
        assert!(encoded.contains(&json!({"$variant": {"type": "float", "value": integral}})));
    }
    assert!(encoded.contains(&json!([{"$variant": {"type": "float", "value": 2.0}}, 2])));
    assert!(encoded.contains(&json!("line\nquote\"")));
    // JSON.stringify leaves these control characters raw; protocol.gd escapes
    // them, which the real-engine test proves by parsing its output strictly.
    assert!(encoded.contains(&json!("bell\u{1}unit\u{1f}del\u{7f}")));
    assert!(encoded.contains(&json!({"key\u{2}": "\u{1b}[0m"})));
    assert!(encoded.contains(&json!({"$variant": {
        "type": "Dictionary", "value": [["$ref", "literal"]]
    }})));
    assert!(encoded.contains(&json!({"$ref": "res://probe.tres"})));
    assert!(encoded.contains(&json!({"$resource": {
        "script": "res://scripted_resource.gd",
        "properties": {"resource_local_to_scene": false, "resource_name": "", "answer": 42}
    }})));
}

#[test]
fn engine_golden_cases_decode_and_reencode_identically() {
    // The cross-implementation check: every value protocol.gd encoded is valid
    // gdview::variant input, and the Rust encoder reproduces the engine's JSON.
    let golden: Value =
        serde_json::from_str(include_str!("fixtures/protocol_golden.json")).unwrap();
    let encoded = golden["payload"]["encoded"].as_array().unwrap();
    assert_eq!(encoded.len(), GOLDEN_CASES);
    for case in encoded {
        let value = VariantJson::from_json(case, &Limits::default())
            .unwrap_or_else(|error| panic!("{case}: {error}"));
        assert_eq!(&value.to_json(), case);
    }
    // Godot's own type_string for every Variant.Type code.
    let names = golden["payload"]["type_names"].as_array().unwrap();
    assert_eq!(names.len(), VariantType::ALL.len());
    for (code, name) in names.iter().enumerate() {
        let name = name.as_str().unwrap();
        let ty = VariantType::from_name(name).unwrap_or_else(|| panic!("{name}"));
        assert_eq!((ty.code() as usize, ty.name()), (code, name));
    }
}

/// Encodes every case, checks decode/re-encode in the engine (type, value and
/// float sign included), and reports Godot's type names. Its envelope is the
/// golden fixture.
const VARIANT_CONTRACTS: &str = r##"extends SceneTree
const Protocol := preload("res://protocol.gd")


func _initialize() -> void:
	var typed: Array[Vector2] = [Vector2(1, 2)]
	var typed_floats: Array[float] = [1.0, 2.5]
	var inline := Resource.new()
	inline.resource_name = "inline"
	var scripted: Resource = load("res://scripted_resource.gd").new()
	# Literal 0.0 and -0.0 share one compiler constant, so build zeros at runtime.
	var zero := float(0)
	var mixed_keys := {1: "int"}
	mixed_keys[1.0] = "float"
	var values: Array = [
		null, true, 42, 9007199254740993, -9223372036854775808, 9223372036854775807,
		1.25, NAN, INF, -INF, "line\nquote\"", &"name", NodePath("child:property"),
		Vector2(1, 2), Vector2i(-1, 2), Vector3(1, 2, 3), Vector3i(1, 2, 3),
		Vector4(1, 2, 3, 4), Vector4i(1, 2, 3, 4), Color(0.25, 0.5, 0.75, 1),
		Rect2(1, 2, 3, 4), Rect2i(1, 2, 3, 4), Plane(Vector3.UP, 2),
		Quaternion(0, 0, 0, 1), AABB(Vector3(1, 2, 3), Vector3(4, 5, 6)),
		Basis(Vector3(1, 2, 3), Vector3(4, 5, 6), Vector3(7, 8, 9)),
		Transform2D(Vector2(1, 2), Vector2(3, 4), Vector2(5, 6)),
		Transform3D(Basis.IDENTITY, Vector3(4, 5, 6)), Projection.IDENTITY,
		[1, "two"], typed, {"key": [3]}, {Vector2i(1, 2): &"value"}, {"$ref": "literal"},
		PackedByteArray([0, 255]), PackedInt32Array([-1, 2147483647]),
		PackedInt64Array([-9223372036854775808, 9223372036854775807]),
		PackedFloat32Array([0.25]), PackedFloat64Array([1.25]), PackedStringArray(["a", "b"]),
		PackedVector2Array([Vector2.ONE]), PackedVector3Array([Vector3.ONE]),
		PackedVector4Array([Vector4.ONE]), PackedColorArray([Color.RED]), inline,
		load("res://probe.tres"), scripted,
		# Integral and signed-zero floats keep their type; bare numbers are ints.
		3.0, -zero, zero, 1e20, 0.1, [2.0, 2], mixed_keys, typed_floats,
		9007199254740991, -9007199254740991, 9007199254740992,
		Vector2(-zero, NAN), PackedFloat64Array([1.0, -zero, INF]),
		# Control characters JSON.stringify would otherwise leave raw.
		"bell\u0001unit\u001fdel\u007f", {"key\u0002": "\u001b[0m"},
		{"$variant": "literal", "$resource": 1}, [], {}, [[], {}],
	]
	var failures: Array = []
	var results: Array = []
	for value in values:
		var encoded: Variant = Protocol.encode(value)
		var wire := Protocol.stringify(encoded)
		var decoded: Variant = Protocol.decode(JSON.parse_string(wire))
		var roundtrip := Protocol.stringify(Protocol.encode(decoded))
		if wire != roundtrip:
			failures.append("wire %s != %s" % [wire, roundtrip])
		if typeof(decoded) != typeof(value):
			failures.append("type %s != %s" % [type_string(typeof(decoded)), wire])
		elif typeof(value) == TYPE_FLOAT and not is_nan(value):
			if decoded != value or 1.0 / decoded != 1.0 / value:
				failures.append("float %s != %s" % [decoded, wire])
		elif typeof(value) not in [TYPE_OBJECT, TYPE_FLOAT, TYPE_VECTOR2]:
			if decoded != value:
				failures.append("value %s != %s" % [var_to_str(decoded), wire])
		results.append(encoded)
	var type_names: Array = []
	for type in range(TYPE_MAX):
		type_names.append(type_string(type))
		if Protocol.type_name(type) != type_string(type):
			failures.append("type name %d" % type)
		if Protocol.type_from_name(type_string(type)) != type:
			failures.append("type code %d" % type)
	Protocol.emit_ok(
		"variant_contracts",
		{"cases": results.size(), "encoded": results, "type_names": type_names, "failures": failures}
	)
	quit()
"##;

/// Reports the error (and encode time) for values protocol.gd must reject,
/// then encodes a cycle through the diagnostic-emitting `encode`.
const VARIANT_ERRORS: &str = r##"extends SceneTree
const Protocol := preload("res://protocol.gd")


func _initialize() -> void:
	var cyclic: Array = []
	for _i in range(4):
		cyclic.append(cyclic)
	var cyclic_dictionary := {}
	cyclic_dictionary["self"] = {"inner": cyclic_dictionary}
	var shared: Array = [0, 0, 0, 0]
	for _i in range(30):
		shared = [shared, shared, shared, shared]
	var deep: Array = []
	for _i in range(40):
		deep = [deep]
	var objects: Array[Object] = []
	var builtin_script: Resource = load("res://builtin.tres").duplicate()
	var cyclic_resource: Resource = load("res://holder.gd").new()
	cyclic_resource.child = cyclic_resource
	var large := PackedByteArray()
	large.resize(Protocol.MAX_ENTRIES)
	var node := Node.new()
	var encode_cases := {
		"cyclic array": cyclic,
		"cyclic dictionary": cyclic_dictionary,
		"cyclic resource": cyclic_resource,
		"shared subgraphs": shared,
		"deep nesting": deep,
		"large packed array": large,
		"node": node,
		"rid": RID(),
		"object array": objects,
		"built-in script": builtin_script,
	}
	var report := {}
	for label in encode_cases:
		var started := Time.get_ticks_msec()
		var result: Dictionary = Protocol.try_encode(encode_cases[label])
		report["encode " + label] = {"error": result.error, "ms": Time.get_ticks_msec() - started}
	var decode_cases := {
		"int trailing text": '{"$variant": {"type": "int", "value": "12abc"}}',
		"int leading zero": '{"$variant": {"type": "int", "value": "007"}}',
		"int negative zero": '{"$variant": {"type": "int", "value": "-0"}}',
		"int plus sign": '{"$variant": {"type": "int", "value": "+1"}}',
		"int empty": '{"$variant": {"type": "int", "value": ""}}',
		"int overflow": '{"$variant": {"type": "int", "value": "9223372036854775808"}}',
		"int underflow": '{"$variant": {"type": "int", "value": "-9223372036854775809"}}',
		"int number": '{"$variant": {"type": "int", "value": 12}}',
		"bare unsafe integer": "9007199254740993",
		"bare huge number": "1e400",
		"float word": '{"$variant": {"type": "float", "value": "NaN"}}',
		"float null": '{"$variant": {"type": "float", "value": null}}',
		"tag extra field": '{"$variant": {"type": "int", "value": "1", "element": "int"}}',
		"tag sibling": '{"$variant": {"type": "int", "value": "1"}, "x": 1}',
		"tag missing value": '{"$variant": {"type": "StringName"}}',
		"tag serde name": '{"$variant": {"type": "Aabb", "value": [0, 0, 0, 0, 0, 0]}}',
		"tag bool": '{"$variant": {"type": "bool", "value": true}}',
		"vector count": '{"$variant": {"type": "Vector2", "value": [1, 2, 3]}}',
		"vector string": '{"$variant": {"type": "Vector2", "value": [1, "2"]}}',
		"vector2i fraction": '{"$variant": {"type": "Vector2i", "value": [1.5, 2]}}',
		"vector2i range": '{"$variant": {"type": "Vector2i", "value": [2147483648, 2]}}',
		"byte range": '{"$variant": {"type": "PackedByteArray", "value": [256]}}',
		"packed vector plain": '{"$variant": {"type": "PackedVector2Array", "value": [[1, 2]]}}',
		"typed array mismatch": '{"$variant": {"type": "Array", "element": "int", "value": [1.5]}}',
		"typed array object": '{"$variant": {"type": "Array", "element": "Object", "value": []}}',
		"dictionary pair": '{"$variant": {"type": "Dictionary", "value": [["a"]]}}',
		"dictionary duplicate":
		'{"$variant": {"type": "Dictionary", "value": [["a", 1], [{"$variant": {"type": "StringName", "value": "a"}}, 2]]}}',
		"ref built-in": '{"$ref": "res://builtin.tres::GDScript_abc"}',
		"ref dot godot": '{"$ref": "res://.godot/x.tres"}',
		"ref sibling": '{"$ref": "res://probe.tres", "x": 1}',
		"resource both": '{"$resource": {"class": "Resource", "script": "res://holder.gd"}}',
		"resource built-in script": '{"$resource": {"script": "res://builtin.tres::GDScript_abc"}}',
		"resource unknown field": '{"$resource": {"class": "Resource", "extra": 1}}',
	}
	for label in decode_cases:
		var result: Dictionary = Protocol.try_decode(JSON.parse_string(decode_cases[label]))
		report["decode " + label] = {"error": result.error, "ms": 0}
	# The diagnostic form: exactly one engine error, no hang.
	Protocol.encode(cyclic)
	node.free()
	cyclic_resource.child = null
	cyclic.clear()
	cyclic_dictionary.clear()
	Protocol.emit_ok("variant_errors", report)
	quit()
"##;

struct EngineRun {
    envelope: Envelope<Value>,
    stdout: String,
    stderr: String,
}

/// Runs `script` in a fresh project holding protocol.gd and `files`, with
/// isolated HOME/XDG directories, and strictly parses its envelope.
fn run_engine_script(script: &str, files: &[(&str, &str)]) -> EngineRun {
    let godot = std::env::var_os("GDKIT_TEST_GODOT").expect("set GDKIT_TEST_GODOT to opt in");
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    let home = dir.path().join("home");
    fs::create_dir_all(&home).unwrap();
    let mut all = vec![
        (
            "project.godot",
            "config_version=5\n[application]\nconfig/name=\"protocol\"\n",
        ),
        ("protocol.gd", PROTOCOL_SOURCE),
        ("main.gd", script),
    ];
    all.extend_from_slice(files);
    for (name, contents) in all {
        let path = project.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    let spawn = Spawn::new(PathBuf::from(godot))
        .args(["--headless", "--path"])
        .arg(&project)
        .args(["--script", "res://main.gd"])
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CACHE_HOME", home.join("cache"));
    let captured = process::run(&spawn, Duration::from_secs(60)).unwrap();
    let stdout = String::from_utf8_lossy(&captured.stdout()).into_owned();
    let stderr = String::from_utf8_lossy(&captured.stderr()).into_owned();
    assert!(
        captured.success() && !captured.timed_out,
        "{stdout}\n{stderr}"
    );
    let envelope = parse::<Value>(&stdout.lines().collect::<Vec<_>>())
        .unwrap_or_else(|error| panic!("{error}\n{stdout}\n{stderr}"));
    EngineRun {
        envelope,
        stdout,
        stderr,
    }
}

fn engine_errors(run: &EngineRun) -> Vec<&str> {
    run.stdout
        .lines()
        .chain(run.stderr.lines())
        .filter(|line| {
            let line = line.trim_start();
            line.starts_with("ERROR:")
                || line.starts_with("SCRIPT ERROR:")
                || line.starts_with("USER ERROR:")
                || line.starts_with("WARNING:")
        })
        .collect()
}

/// Runs the Godot codec over every contract case and compares its envelope
/// with the golden fixture. With GDKIT_REFRESH_GOLDEN set, rewrites the fixture
/// and its byte-identical spike copy from the engine output instead.
#[test]
#[ignore = "requires GDKIT_TEST_GODOT"]
fn real_engine_protocol_gd_matches_golden_fixture() {
    let run = run_engine_script(
        VARIANT_CONTRACTS,
        &[
            (
                "scripted_resource.gd",
                "extends Resource\n@export var answer: int = 42\n",
            ),
            (
                "probe.tres",
                "[gd_resource type=\"Resource\" format=3]\n[resource]\nresource_name=\"probe\"\n",
            ),
        ],
    );
    assert_eq!(engine_errors(&run), Vec::<&str>::new(), "{}", run.stderr);
    let payload = run.envelope.payload.as_ref().unwrap();
    assert_eq!(payload["failures"], json!([]));
    assert_eq!(payload["cases"], GOLDEN_CASES);
    let actual = serde_json::to_value(&run.envelope).unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/protocol_golden.json");
    if std::env::var_os("GDKIT_REFRESH_GOLDEN").is_some() {
        let text = serde_json::to_string_pretty(&actual).unwrap() + "\n";
        let spike = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../spike-real-engine-check-contracts/protocol_golden.json");
        for path in [&fixture, &spike] {
            fs::write(path, &text).unwrap();
        }
    }
    let golden: Value = serde_json::from_str(&fs::read_to_string(&fixture).unwrap()).unwrap();
    assert_eq!(actual, golden, "set GDKIT_REFRESH_GOLDEN=1 to refresh");
    // The engine's output is itself canonical Rust grammar.
    for case in payload["encoded"].as_array().unwrap() {
        let value = VariantJson::from_json(case, &Limits::default()).unwrap();
        assert_eq!(&value.to_json(), case);
    }
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT"]
fn real_engine_protocol_gd_rejects_cycles_budgets_and_non_canonical_input() {
    let run = run_engine_script(
        VARIANT_ERRORS,
        &[
            (
                "holder.gd",
                "extends Resource\n@export var child: Resource\n",
            ),
            (
                "probe.tres",
                "[gd_resource type=\"Resource\" format=3]\n[resource]\nresource_name=\"probe\"\n",
            ),
            (
                "builtin.tres",
                "[gd_resource type=\"Resource\" load_steps=2 format=3]\n\n\
                 [sub_resource type=\"GDScript\" id=\"GDScript_abc\"]\n\
                 script/source = \"extends Resource\\n\"\n\n\
                 [resource]\nscript = SubResource(\"GDScript_abc\")\n",
            ),
        ],
    );
    // Only the deliberate encode(cyclic) diagnostic reaches the engine log.
    let errors = engine_errors(&run);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("Variant contains a cyclic reference"));
    let report = run.envelope.payload.unwrap();
    let report = report.as_object().unwrap();
    assert_eq!(report.len(), 43);
    for (label, expected) in [
        ("encode cyclic array", "cyclic reference"),
        ("encode cyclic dictionary", "cyclic reference"),
        ("encode cyclic resource", "cyclic reference"),
        ("encode shared subgraphs", "exceeds 100000 values"),
        ("encode deep nesting", "exceeds depth 32"),
        ("encode large packed array", "exceeds 100000 values"),
        ("encode node", "Object is not transportable"),
        ("encode rid", "RID is not transportable"),
        ("encode object array", "Object-typed arrays"),
        ("encode built-in script", "not built in"),
        ("decode bare unsafe integer", "ambiguous"),
        ("decode bare huge number", "tagged float form"),
        ("decode float word", "nan, inf, -inf and -0.0"),
        ("decode tag serde name", "unsupported Variant tag: Aabb"),
        ("decode vector count", "array of 2 numbers"),
        ("decode vector2i range", "out of i32 range"),
        ("decode byte range", "out of u8 range"),
        ("decode dictionary duplicate", "Duplicate dictionary key"),
        ("decode ref built-in", "Invalid resource reference"),
        ("decode resource built-in script", "saved res:// script"),
    ] {
        let error = report[label]["error"].as_str().unwrap();
        assert!(error.contains(expected), "{label}: {error}");
    }
    for (label, entry) in report {
        let error = entry["error"].as_str().unwrap();
        assert!(!error.is_empty(), "{label} was accepted");
        if label.starts_with("decode int") {
            assert!(error.contains("canonical decimal"), "{label}: {error}");
        }
        // Cycles fail immediately; budgets bound shared subgraphs.
        let limit = if label.contains("cyclic") { 50 } else { 5_000 };
        assert!(entry["ms"].as_u64().unwrap() < limit, "{label}: {entry}");
    }
}

#[test]
fn only_exact_line_start_prefixes_count() {
    let line = result_line(json!({"protocol": 1, "harness": "probe", "ok": true}));
    for noise in [
        format!(" {line}"),
        format!("\t{line}"),
        format!("log: {line}"),
        line.to_lowercase(),
        format!("\u{feff}{line}"),
    ] {
        assert!(matches!(
            parse::<Value>(&[&noise]),
            Err(ProtocolError::Missing)
        ));
        assert!(parse::<Value>(&[&noise, &line]).is_ok());
    }
    assert!(
        parse::<Value>(&[&format!(
            "{RESULT_PREFIX} \t{}\r\n",
            &line[RESULT_PREFIX.len()..]
        )])
        .is_ok()
    );
}

#[test]
fn invalid_envelope_metadata_is_malformed_not_a_payload_error() {
    for value in [
        json!({"harness": "probe", "ok": true}),
        json!({"protocol": "1", "harness": "probe", "ok": true}),
        json!({"protocol": -1, "harness": "probe", "ok": true}),
        json!({"protocol": 4294967296_u64, "harness": "probe", "ok": true}),
        json!({"protocol": 1.5, "harness": "probe", "ok": true}),
        json!({"protocol": null, "harness": "probe", "ok": true}),
        json!({"protocol": 1, "ok": true}),
        json!({"protocol": 1, "harness": null, "ok": true}),
        json!({"protocol": 1, "harness": "probe"}),
        json!({"protocol": 1, "harness": "probe", "ok": 1}),
        json!({"protocol": 1, "harness": "probe", "ok": false, "error": {"stage": "load"}}),
        json!({"protocol": 1, "harness": "probe", "ok": false, "error": {"stage": 1, "message": "bad"}}),
        json!({"protocol": 1, "harness": "probe", "ok": false, "error": {"stage": "load", "message": "bad", "field": 1}}),
    ] {
        let line = result_line(value);
        assert!(
            matches!(parse::<Payload>(&[&line]), Err(ProtocolError::Malformed(_))),
            "{line}"
        );
    }
}

#[test]
fn payload_shape_errors_are_distinct_from_wire_errors() {
    for payload in [
        json!([]),
        json!(true),
        json!({}),
        json!({"count": -1, "label": "x"}),
        json!({"count": 1, "label": 2}),
    ] {
        let line =
            result_line(json!({"protocol": 1, "harness": "probe", "ok": true, "payload": payload}));
        assert!(
            matches!(parse::<Payload>(&[&line]), Err(ProtocolError::Payload(message)) if !message.is_empty())
        );
    }
}

#[test]
fn optional_fields_and_unknown_fields_follow_envelope_serde_contract() {
    for ok in [true, false] {
        for payload in [None, Some(Value::Null)] {
            let mut wire = json!({"protocol": 1, "harness": "future_harness", "ok": ok, "future_metadata": 42});
            if let Some(payload) = payload {
                wire["payload"] = payload;
            }
            let line = result_line(wire.clone());
            let envelope = parse::<Payload>(&[&line]).unwrap();
            assert_eq!(
                envelope,
                serde_json::from_value::<Envelope<Payload>>(wire).unwrap()
            );
            assert_eq!(envelope.payload, None);
            assert_eq!(envelope.error, None);
        }
    }
}

#[test]
fn duplicate_envelope_fields_and_trailing_json_are_rejected() {
    for body in [
        r#"{"protocol":1,"protocol":1,"harness":"probe","ok":true}"#,
        r#"{"protocol":1,"harness":"probe","harness":"check","ok":true}"#,
        r#"{"protocol":1,"harness":"probe","ok":true,"ok":false}"#,
        r#"{"protocol":1,"harness":"probe","ok":true,"payload":1,"payload":2}"#,
        r#"{"protocol":1,"harness":"probe","ok":true,"error":null,"error":null}"#,
        r#"{"protocol":1,"harness":"probe","ok":true} {}"#,
    ] {
        assert!(
            matches!(
                parse::<Value>(&[&format!("{RESULT_PREFIX}{body}")]),
                Err(ProtocolError::Malformed(_))
            ),
            "{body}"
        );
    }
}

#[test]
fn envelope_round_trips_through_serde_and_protocol_parser() {
    let envelope = Envelope {
        protocol: PROTOCOL_VERSION,
        harness: "check".into(),
        ok: false,
        payload: Some(json!({"diagnostics": ["one", "two"]})),
        error: Some(HarnessError {
            stage: "compile".into(),
            message: "failed\nwith details".into(),
            field: Some("file.gd".into()),
        }),
    };
    let line = format!(
        "{RESULT_PREFIX}{}",
        serde_json::to_string(&envelope).unwrap()
    );
    assert_eq!(parse::<Value>(&[&line]).unwrap(), envelope);
}

#[test]
fn protocol_error_messages_explain_the_failure() {
    assert_eq!(
        ProtocolError::Missing.to_string(),
        "harness did not print a result line"
    );
    assert_eq!(
        ProtocolError::Multiple.to_string(),
        "harness printed more than one result line"
    );
    assert_eq!(
        ProtocolError::VersionMismatch {
            expected: 1,
            found: 2
        }
        .to_string(),
        "protocol version mismatch: gdkit speaks 1, harness spoke 2"
    );
    assert!(
        ProtocolError::Malformed("details".into())
            .to_string()
            .contains("details")
    );
    assert!(
        ProtocolError::Payload("details".into())
            .to_string()
            .contains("details")
    );
}
