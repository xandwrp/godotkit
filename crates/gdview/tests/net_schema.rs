use gdview::{Project, net};

#[test]
fn report_schema_three_matches_golden_and_explanation_remaps_candidates() {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in [
        ("project.godot", "config_version=5\n"),
        ("actor.gd", include_str!("fixtures/net/actor.gd")),
        (
            "replication.tscn",
            include_str!("fixtures/net/replication.tscn"),
        ),
        ("legacy.tscn", include_str!("fixtures/net/legacy.tscn")),
    ] {
        std::fs::write(dir.path().join(path), text).unwrap();
    }
    let r = net::analyze_project(&Project::open(dir.path()).unwrap()).unwrap();
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/net/report.json")).unwrap();
    assert_eq!(serde_json::to_value(&r).unwrap(), golden);
    let e = net::explain(&r, "ping");
    assert!(e.matched);
    assert_eq!(e.endpoints.len(), 1);
    assert_eq!(e.endpoints[0].method, "ping");
    assert_eq!(e.calls.len(), 2);
    assert!(e.calls.iter().all(|c| c.candidates == [0]));
}
