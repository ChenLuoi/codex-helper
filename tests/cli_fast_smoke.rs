mod common;

use common::{assert_contains, assert_json_eq, assert_success, parse_json, run_codex_ops, Sandbox};
use std::fs;

#[test]
fn fast_records_status_and_history() {
    let sandbox = Sandbox::new();
    let history_file = &sandbox.usage_mode_history_file;
    let _ = fs::remove_file(history_file);

    let initial_status = run_codex_ops(
        [
            "fast",
            "status",
            "--usage-mode-history-file",
            history_file.to_str().unwrap(),
            "--json",
        ],
        &sandbox,
    );
    assert_success(&initial_status, "initial fast status");
    let initial = parse_json(&initial_status.stdout, "initial fast status");
    assert_json_eq(&initial["state"], "unknown", "initial mode state");
    assert!(initial["fast"].is_null(), "initial fast should be null");
    assert_json_eq(&initial["switchCount"], 0, "initial switch count");

    let on = run_codex_ops(
        [
            "fast",
            "on",
            "--at",
            "2026-05-12T12:34:56Z",
            "--usage-mode-history-file",
            history_file.to_str().unwrap(),
        ],
        &sandbox,
    );
    assert_success(&on, "fast on");
    assert_contains(
        &on.stdout,
        "Recorded local fast attribution only. This does not change Codex settings.",
        "fast on local-only message",
    );
    assert_contains(&on.stdout, "Fast: on", "fast on state");
    assert!(history_file.exists(), "fast on should write history");

    let status = run_codex_ops(
        [
            "fast",
            "status",
            "--usage-mode-history-file",
            history_file.to_str().unwrap(),
            "--json",
        ],
        &sandbox,
    );
    assert_success(&status, "fast status after on");
    let status_json = parse_json(&status.stdout, "fast status after on");
    assert_json_eq(&status_json["state"], "on", "status state after on");
    assert_json_eq(&status_json["fast"], true, "status fast after on");
    assert_json_eq(
        &status_json["switchCount"],
        1,
        "status switch count after on",
    );
    assert_json_eq(
        &status_json["latestSwitch"]["source"],
        "fast on",
        "status latest switch source",
    );
    assert_json_eq(
        &status_json["changesCodexSettings"],
        false,
        "status does not change Codex settings",
    );

    let off = run_codex_ops(
        [
            "fast",
            "off",
            "--at",
            "2026-05-12T13:00:00Z",
            "--usage-mode-history-file",
            history_file.to_str().unwrap(),
            "--json",
        ],
        &sandbox,
    );
    assert_success(&off, "fast off json");
    let off_json = parse_json(&off.stdout, "fast off json");
    assert_json_eq(&off_json["state"], "off", "off json state");
    assert_json_eq(&off_json["fast"], false, "off json fast");
    assert_json_eq(
        &off_json["recordedLocalAttributionOnly"],
        true,
        "off json local-only flag",
    );

    let history = run_codex_ops(
        [
            "fast",
            "history",
            "--usage-mode-history-file",
            history_file.to_str().unwrap(),
            "--json",
        ],
        &sandbox,
    );
    assert_success(&history, "fast history json");
    let history_json = parse_json(&history.stdout, "fast history json");
    assert_json_eq(&history_json["switchCount"], 2, "history switch count");
    assert_json_eq(
        &history_json["switches"][0]["source"],
        "fast on",
        "history first source",
    );
    assert_json_eq(
        &history_json["switches"][1]["source"],
        "fast off",
        "history second source",
    );
}

#[test]
fn fast_history_table_keeps_local_only_semantics() {
    let sandbox = Sandbox::new();
    let history_file = &sandbox.usage_mode_history_file;
    let _ = fs::remove_file(history_file);

    let record = run_codex_ops(
        [
            "fast",
            "on",
            "--at",
            "2026-05-12T12:00:00Z",
            "--usage-mode-history-file",
            history_file.to_str().unwrap(),
        ],
        &sandbox,
    );
    assert_success(&record, "fast on for table history");

    let history = run_codex_ops(
        [
            "fast",
            "history",
            "--usage-mode-history-file",
            history_file.to_str().unwrap(),
        ],
        &sandbox,
    );
    assert_success(&history, "fast history table");
    assert_contains(
        &history.stdout,
        "Recorded local fast attribution only. This does not change Codex settings.",
        "history local-only message",
    );
    assert_contains(&history.stdout, "Time", "history table header");
    assert_contains(&history.stdout, "fast on", "history table source");
}

#[test]
fn fast_status_reads_checked_in_fixture_history() {
    let sandbox = Sandbox::new();

    let status = run_codex_ops(
        [
            "fast",
            "status",
            "--usage-mode-history-file",
            sandbox.fixture_usage_mode_history_file.to_str().unwrap(),
            "--json",
        ],
        &sandbox,
    );
    assert_success(&status, "fast fixture status json");
    let status_json = parse_json(&status.stdout, "fast fixture status json");
    assert_json_eq(&status_json["state"], "off", "fixture mode state");
    assert_json_eq(&status_json["fast"], false, "fixture fast flag");
    assert_json_eq(&status_json["switchCount"], 2, "fixture switch count");
    assert_json_eq(
        &status_json["latestSwitch"]["source"],
        "fast off",
        "fixture latest switch source",
    );
    assert_json_eq(
        &status_json["changesCodexSettings"],
        false,
        "fixture status does not change Codex settings",
    );
}
