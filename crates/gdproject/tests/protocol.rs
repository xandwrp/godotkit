// Offline acceptance tests for gdproject::protocol.
use gdproject::protocol::{
    Envelope, HarnessError, PROTOCOL_VERSION, ProtocolError, RESULT_PREFIX, parse_envelope,
};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

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
    // Frozen engine output copied from
    // spike-real-engine-check-contracts/protocol_golden.json. The local copy
    // keeps this offline test independent of the spike and a Godot executable.
    // These are JSON transport assertions, not gdview Variant decoding tests.
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
    assert_eq!(payload["cases"], 47);
    let encoded = payload["encoded"].as_array().unwrap();
    assert_eq!(encoded.len(), 47);
    assert_eq!(payload, &golden["payload"]);
    assert_eq!(serde_json::to_value(&envelope).unwrap(), golden);

    // Pin precision-sensitive and nested representations from the engine output.
    for decimal in [
        "9007199254740993",
        "-9223372036854775808",
        "9223372036854775807",
    ] {
        assert!(encoded.contains(&json!({"$variant": {"type": "int", "value": decimal}})));
    }
    for special in ["nan", "inf", "-inf"] {
        assert!(encoded.contains(&json!({"$variant": {"type": "float", "value": special}})));
    }
    assert!(encoded.contains(&json!("line\nquote\"")));
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
