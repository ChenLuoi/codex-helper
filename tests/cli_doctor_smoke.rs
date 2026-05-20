mod common;

use common::{
    assert_check_status, assert_json_eq, assert_no_secrets, assert_success, parse_json,
    run_codex_ops, Sandbox,
};

#[test]
fn doctor_json_reports_core_checks_and_pricing_source() {
    let sandbox = Sandbox::new();

    let doctor = run_codex_ops(
        [
            "doctor",
            "--auth-file",
            sandbox.auth_file.to_str().unwrap(),
            "--codex-home",
            sandbox.codex_home.to_str().unwrap(),
            "--sessions-dir",
            sandbox.sessions_dir.to_str().unwrap(),
            "--cycle-file",
            sandbox.cycle_file.to_str().unwrap(),
            "--json",
        ],
        &sandbox,
    );
    assert_success(&doctor, "doctor json");
    assert_no_secrets(&doctor, "doctor json");
    let doctor_json = parse_json(&doctor.stdout, "doctor json");
    assert_json_eq(&doctor_json["summary"]["errors"], 0, "doctor errors");
    assert_json_eq(&doctor_json["summary"]["warnings"], 0, "doctor warnings");
    assert_check_status(&doctor_json, "Auth file", "ok");
    assert_check_status(&doctor_json, "Recent usage", "ok");
    assert_check_status(&doctor_json, "Cycle store", "ok");
    assert_check_status(&doctor_json, "Pricing", "ok");

    let pricing = doctor_json["checks"]
        .as_array()
        .expect("doctor checks array")
        .iter()
        .find(|check| check["name"] == "Pricing")
        .expect("pricing check");
    let details = pricing["details"].as_array().expect("pricing details");
    assert!(
        details
            .iter()
            .any(|detail| detail == "Source: OpenAI Help Center Codex rate card"),
        "pricing source detail missing: {details:?}"
    );
    assert!(
        details.iter().any(|detail| detail == "Checked: 2026-05-13"),
        "pricing checked_at detail missing: {details:?}"
    );
    assert!(
        details
            .iter()
            .any(|detail| detail == "Credits: 25 credits = $1"),
        "pricing credit conversion detail missing: {details:?}"
    );
}
