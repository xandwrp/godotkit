// Acceptance tests for gdproject::probe. Offline unless prefixed real_engine_.
#![allow(unused)]

#[test]
#[ignore = "scaffold"]
fn request_sends_token_and_rejects_replies_without_matching_request_id() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn request_times_out_and_reports_connection_refused_distinctly() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn status_reports_frames_and_ready() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn checkpoints_report_adapter_errors_as_status_not_transport_error() {
    todo!()
}

#[test]
#[ignore = "scaffold"]
fn network_observation_decodes_peers_authority_and_inventory() {
    todo!()
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT"]
fn real_engine_probe_reports_ready_and_answers_every_query() {
    todo!()
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT"]
fn real_engine_script_error_does_not_freeze_the_probe() {
    todo!()
}
