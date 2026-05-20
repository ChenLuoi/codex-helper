mod common;

use chrono::NaiveDate;
use common::{
    assert_array, assert_contains, assert_cycle_diagnostics_schema, assert_failure_contains,
    assert_json_eq, assert_json_local_day_end, assert_json_local_day_start, assert_success,
    assert_usage_totals_schema, capture_after, parse_json, read_file_bytes, run_codex_ops, Sandbox,
    FIXED_NOW,
};

#[test]
fn cycle_current_history_and_detail_json_schema_use_fixed_now() {
    let sandbox = Sandbox::new();

    let cycle_current = run_codex_ops(
        [
            "cycle",
            "current",
            "--cycle-file",
            sandbox.cycle_file.to_str().unwrap(),
            "--account-id",
            "account-fixture",
            "--sessions-dir",
            sandbox.sessions_dir.to_str().unwrap(),
            "--format",
            "json",
        ],
        &sandbox,
    );
    assert_success(&cycle_current, "cycle current fixed-now schema");
    let current = parse_json(&cycle_current.stdout, "cycle current fixed-now schema");
    common::assert_has_keys(
        &current,
        &[
            "status",
            "periodHours",
            "now",
            "current",
            "byDay",
            "byModel",
            "totals",
            "diagnostics",
        ],
        "cycle current schema",
    );
    assert_json_eq(&current["now"], FIXED_NOW, "cycle current fixed-now");
    assert_json_eq(&current["status"], "active", "cycle current status");
    assert_json_eq(
        &current["current"]["resetAt"],
        "2026-05-17T09:00:00.000Z",
        "cycle current reset",
    );
    assert_usage_totals_schema(&current["totals"], "cycle current totals");
    assert_cycle_diagnostics_schema(&current["diagnostics"], "cycle current diagnostics");

    let cycle_history = run_codex_ops(
        [
            "cycle",
            "history",
            "--cycle-file",
            sandbox.cycle_file.to_str().unwrap(),
            "--account-id",
            "account-fixture",
            "--sessions-dir",
            sandbox.sessions_dir.to_str().unwrap(),
            "--last",
            "30d",
            "--format",
            "json",
        ],
        &sandbox,
    );
    assert_success(&cycle_history, "cycle history fixed-now schema");
    let history = parse_json(&cycle_history.stdout, "cycle history fixed-now schema");
    common::assert_has_keys(
        &history,
        &[
            "status",
            "periodHours",
            "start",
            "end",
            "rows",
            "totals",
            "diagnostics",
        ],
        "cycle history schema",
    );
    assert_json_eq(
        &history["start"],
        "2026-04-17T00:00:00.000Z",
        "cycle history fixed-now start",
    );
    assert_json_eq(&history["end"], FIXED_NOW, "cycle history fixed-now end");
    assert_eq!(
        assert_array(&history["rows"], "cycle history rows").len(),
        1,
        "cycle history row count"
    );
    assert_usage_totals_schema(&history["totals"], "cycle history totals");
    assert_cycle_diagnostics_schema(&history["diagnostics"], "cycle history diagnostics");

    let cycle_detail = run_codex_ops(
        [
            "cycle",
            "history",
            "anc_20260510T090000000Z",
            "--cycle-file",
            sandbox.cycle_file.to_str().unwrap(),
            "--account-id",
            "account-fixture",
            "--sessions-dir",
            sandbox.sessions_dir.to_str().unwrap(),
            "--last",
            "30d",
            "--format",
            "json",
        ],
        &sandbox,
    );
    assert_success(&cycle_detail, "cycle history detail fixed-now schema");
    let detail = parse_json(
        &cycle_detail.stdout,
        "cycle history detail fixed-now schema",
    );
    assert_json_eq(
        &detail["historyEnd"],
        FIXED_NOW,
        "cycle history detail fixed-now end",
    );
    assert_json_eq(
        &detail["cycle"]["id"],
        "anc_20260510T090000000Z",
        "cycle history detail fixed-now id",
    );
}

#[test]
fn cycle_history_explicit_range_and_estimated_windows_are_stable() {
    let sandbox = Sandbox::new();

    let cycle_history_explicit = run_codex_ops(
        [
            "cycle",
            "history",
            "--cycle-file",
            sandbox.cycle_file.to_str().unwrap(),
            "--account-id",
            "account-fixture",
            "--sessions-dir",
            sandbox.sessions_dir.to_str().unwrap(),
            "--start",
            "2026-05-10",
            "--end=2026-05-17",
            "--format",
            "json",
        ],
        &sandbox,
    );
    assert_success(&cycle_history_explicit, "cycle history explicit range");
    let explicit_history = parse_json(
        &cycle_history_explicit.stdout,
        "cycle history explicit range",
    );
    assert_json_local_day_start(
        &explicit_history["start"],
        NaiveDate::from_ymd_opt(2026, 5, 10).expect("cycle explicit start"),
        "cycle history explicit start",
    );
    assert_json_local_day_end(
        &explicit_history["end"],
        NaiveDate::from_ymd_opt(2026, 5, 17).expect("cycle explicit end"),
        "cycle history explicit end",
    );

    let estimated = run_codex_ops(
        [
            "cycle",
            "history",
            "--cycle-file",
            sandbox.cycle_file.to_str().unwrap(),
            "--account-id",
            "account-fixture",
            "--sessions-dir",
            sandbox.sessions_dir.to_str().unwrap(),
            "--start",
            "2026-05-01T00:00:00Z",
            "--end",
            "2026-05-17T00:00:00Z",
            "--estimate-before-anchor",
            "--format",
            "json",
        ],
        &sandbox,
    );
    assert_success(&estimated, "cycle history estimated before anchor");
    let estimated_json = parse_json(&estimated.stdout, "cycle history estimated before anchor");
    assert_json_eq(
        &estimated_json["diagnostics"]["estimateBeforeAnchor"],
        true,
        "cycle history estimate flag",
    );
    assert_json_eq(
        &estimated_json["diagnostics"]["ignoredBeforeAnchorEvents"],
        0,
        "cycle history estimated ignores before anchor",
    );
}

#[test]
fn cycle_add_list_remove_mutates_only_cycle_store() {
    let sandbox = Sandbox::new();

    let cycle_list = run_codex_ops(
        [
            "cycle",
            "list",
            "--cycle-file",
            sandbox.cycle_file.to_str().unwrap(),
            "--account-id",
            "account-fixture",
            "--format",
            "json",
        ],
        &sandbox,
    );
    assert_success(&cycle_list, "cycle list json");
    let mut cycle_list_json = parse_json(&cycle_list.stdout, "cycle list json");
    assert_eq!(
        cycle_list_json["anchors"].as_array().unwrap().len(),
        1,
        "cycle list initial anchors"
    );
    assert_json_eq(
        &cycle_list_json["anchors"][0]["id"],
        "anc_20260510T090000000Z",
        "cycle list anchor id",
    );

    let cycle_add = run_codex_ops(
        [
            "cycle",
            "add",
            "2026-05-17",
            "09:00",
            "--note",
            "smoke",
            "--cycle-file",
            sandbox.cycle_file.to_str().unwrap(),
            "--account-id",
            "account-fixture",
        ],
        &sandbox,
    );
    assert_success(&cycle_add, "cycle add");
    assert_contains(&cycle_add.stdout, "Added weekly cycle anchor:", "cycle add");
    let added_anchor_id = capture_after(&cycle_add.stdout, "Added weekly cycle anchor: ");

    cycle_list_json = parse_json(
        &run_codex_ops(
            [
                "cycle",
                "list",
                "--cycle-file",
                sandbox.cycle_file.to_str().unwrap(),
                "--account-id",
                "account-fixture",
                "--format",
                "json",
            ],
            &sandbox,
        )
        .stdout,
        "cycle list after add",
    );
    let anchors = cycle_list_json["anchors"].as_array().unwrap();
    assert_eq!(anchors.len(), 2, "cycle list anchors after add");
    assert_json_eq(
        &anchors.last().unwrap()["id"],
        added_anchor_id.as_str(),
        "cycle added anchor id",
    );

    let cycle_remove = run_codex_ops(
        [
            "cycle",
            "remove",
            added_anchor_id.as_str(),
            "--cycle-file",
            sandbox.cycle_file.to_str().unwrap(),
            "--account-id",
            "account-fixture",
        ],
        &sandbox,
    );
    assert_success(&cycle_remove, "cycle remove");
    assert_contains(
        &cycle_remove.stdout,
        &format!("Removed weekly cycle anchor: {added_anchor_id}"),
        "cycle remove",
    );
}

#[test]
fn cycle_history_select_non_tty_paths_do_not_mutate_store() {
    let sandbox = Sandbox::new();
    let cycle_file_before = read_file_bytes(&sandbox.cycle_file);

    let cycle_select_empty = run_codex_ops(
        [
            "cycle",
            "history",
            "--select",
            "--cycle-file",
            sandbox.cycle_file.to_str().unwrap(),
            "--account-id",
            "account-fixture",
            "--sessions-dir",
            sandbox.sessions_dir.to_str().unwrap(),
            "--start",
            "2026-04-01T00:00:00Z",
            "--end",
            "2026-04-02T00:00:00Z",
        ],
        &sandbox,
    );
    assert_success(&cycle_select_empty, "cycle history select empty");
    assert_contains(
        &cycle_select_empty.stdout,
        "No weekly cycles to select.",
        "cycle history select empty",
    );

    let cycle_select = run_codex_ops(
        [
            "cycle",
            "history",
            "--select",
            "--cycle-file",
            sandbox.cycle_file.to_str().unwrap(),
            "--account-id",
            "account-fixture",
            "--sessions-dir",
            sandbox.sessions_dir.to_str().unwrap(),
            "--last",
            "30d",
            "--format",
            "json",
        ],
        &sandbox,
    );
    assert_failure_contains(
        &cycle_select,
        1,
        "cycle history --select requires an interactive terminal unless a cycle id is supplied.",
        "cycle history select non-tty",
    );

    let cycle_select_with_id = run_codex_ops(
        [
            "cycle",
            "history",
            "anc_20260510T090000000Z",
            "--select",
            "--cycle-file",
            sandbox.cycle_file.to_str().unwrap(),
            "--account-id",
            "account-fixture",
            "--sessions-dir",
            sandbox.sessions_dir.to_str().unwrap(),
            "--format",
            "json",
        ],
        &sandbox,
    );
    assert_failure_contains(
        &cycle_select_with_id,
        1,
        "cycle history accepts either a cycle id or --select, not both.",
        "cycle history id and select",
    );
    assert_eq!(
        read_file_bytes(&sandbox.cycle_file),
        cycle_file_before,
        "cycle history --select changed cycle store"
    );
}
