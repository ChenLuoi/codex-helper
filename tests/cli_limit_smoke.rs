mod common;

use chrono::DateTime;
use common::{
    assert_array, assert_contains, assert_has_keys, assert_json_eq, assert_limit_current_schema,
    assert_limit_diagnostics_schema, assert_limit_reset_schema, assert_limit_sample_schema,
    assert_limit_trend_change_schema, assert_limit_window_schema, assert_no_limit_source_leakage,
    assert_no_source_paths_by_default, assert_not_contains, assert_success, parse_csv, parse_json,
    run_codex_ops, Sandbox,
};
use serde_json::{json, Value};
use std::fs;

#[test]
fn limit_current_json_reports_latest_state_and_unobserved() {
    let sandbox = Sandbox::new();

    let current = run_codex_ops(limit_args(&sandbox, "current", &["--json"]), &sandbox);
    assert_success(&current, "limit current json");
    assert_no_limit_source_leakage(&current.stdout, &sandbox, "limit current json");
    let report = parse_json(&current.stdout, "limit current json");
    assert_has_keys(
        &report,
        &["status", "now", "start", "end", "sessionsDir", "current"],
        "limit current report",
    );
    assert_json_eq(&report["status"], "ok", "limit current status");
    let rows = assert_array(&report["current"], "current rows");
    assert_eq!(rows.len(), 3);
    for (index, row) in rows.iter().enumerate() {
        assert_limit_current_schema(row, &format!("current row {index}"));
    }

    let expired_five_hour = find_row(rows, "5h", "account-fixture", "pro");
    assert_json_eq(&expired_five_hour["status"], "expired", "fixture 5h status");
    assert_json_eq(&expired_five_hour["usedPercent"], 8.0, "expired 5h used");
    assert!(
        expired_five_hour["resetInSeconds"].is_null(),
        "expired 5h reset seconds"
    );

    let active_weekly = rows
        .iter()
        .filter(|row| {
            row["window"] == "7d"
                && row["accountId"] == "account-fixture"
                && row["planType"] == "pro"
                && row["status"] == "active"
        })
        .collect::<Vec<_>>();
    assert_eq!(
        active_weekly.len(),
        1,
        "current should report only the latest logical weekly cycle"
    );
    assert!(
        active_weekly
            .iter()
            .all(|row| row["resetsAt"] == "2026-05-19T09:00:00Z" && row["usedPercent"] == 4.0),
        "fixture current weekly cycle"
    );
    assert_no_source_paths_by_default(&report, "limit current json");

    let empty_sessions_dir = sandbox.home.join("empty-sessions");
    let empty_current = run_codex_ops(
        [
            "limit",
            "current",
            "--sessions-dir",
            empty_sessions_dir.to_str().unwrap(),
            "--json",
        ],
        &sandbox,
    );
    assert_success(&empty_current, "limit current empty json");
    let empty = parse_json(&empty_current.stdout, "limit current empty json");
    assert_json_eq(&empty["status"], "unobserved", "empty current status");
    assert!(assert_array(&empty["current"], "empty current rows").is_empty());
}

#[test]
fn limit_current_table_shows_status_and_nonstandard_primary_minutes() {
    let sandbox = Sandbox::new();
    add_nonstandard_primary_fixture(&sandbox);

    let table = run_codex_ops(
        limit_args(&sandbox, "current", &["--format", "table"]),
        &sandbox,
    );
    assert_success(&table, "limit current table");
    assert_contains(&table.stdout, "Status", "current status header");
    assert_contains(
        &table.stdout,
        "Window minutes",
        "current window minutes header",
    );
    assert_contains(&table.stdout, "expired", "current expired row");

    let csv = run_codex_ops(
        limit_args(&sandbox, "current", &["--format", "csv"]),
        &sandbox,
    );
    assert_success(&csv, "limit current csv");
    let csv_rows = parse_csv(&csv.stdout);
    assert_eq!(
        csv_rows[0],
        vec![
            "Status",
            "Window",
            "Window minutes",
            "Account",
            "Plan",
            "Limit",
            "Used",
            "Remaining",
            "Resets at",
            "Last seen",
        ],
        "limit current csv header"
    );
    assert!(
        !csv_rows[0]
            .iter()
            .any(|header| header == "Reset in seconds"),
        "limit current csv should not expose reset seconds"
    );

    let markdown = run_codex_ops(
        limit_args(&sandbox, "current", &["--format", "markdown"]),
        &sandbox,
    );
    assert_success(&markdown, "limit current markdown");
    assert_contains(
        &markdown.stdout,
        "| Status | Window | Window minutes |",
        "current markdown header",
    );

    let current = run_codex_ops(limit_args(&sandbox, "current", &["--json"]), &sandbox);
    assert_success(&current, "limit current nonstandard json");
    let report = parse_json(&current.stdout, "limit current nonstandard json");
    let rows = assert_array(&report["current"], "current rows");
    let primary = find_row(rows, "primary", "account-other", "team");
    assert_json_eq(&primary["status"], "active", "nonstandard primary status");
    assert_json_eq(
        &primary["windowMinutes"],
        60,
        "nonstandard primary window minutes",
    );
    assert_json_eq(&primary["usedPercent"], 11.0, "nonstandard primary used");
}

#[test]
fn limit_current_ignores_zero_minute_windows() {
    let sandbox = Sandbox::new();
    add_zero_minute_window_fixture(&sandbox);

    let current = run_codex_ops(
        limit_args(&sandbox, "current", &["--json", "--verbose"]),
        &sandbox,
    );
    assert_success(&current, "limit current zero-minute json");
    let report = parse_json(&current.stdout, "limit current zero-minute json");
    let rows = assert_array(&report["current"], "current rows");
    assert!(
        rows.iter()
            .all(|row| row["windowMinutes"].as_i64() != Some(0)),
        "zero-minute windows should not be reported"
    );
    assert_json_eq(
        &report["diagnostics"]["invalidWindowMinutes"],
        1,
        "zero-minute window diagnostics",
    );
}

#[test]
fn limit_windows_json_is_sorted_and_uses_fixed_window_schema() {
    let sandbox = Sandbox::new();

    let windows = run_codex_ops(
        limit_args(&sandbox, "windows", &["--window", "7d", "--json"]),
        &sandbox,
    );
    assert_success(&windows, "limit windows json");
    assert_no_limit_source_leakage(&windows.stdout, &sandbox, "limit windows json");
    let report = parse_json(&windows.stdout, "limit windows json");
    assert_json_eq(&report["status"], "ok", "limit windows status");
    let rows = assert_array(&report["windows"], "window rows");
    assert_eq!(rows.len(), 3);
    for (index, row) in rows.iter().enumerate() {
        assert_limit_window_schema(row, &format!("window row {index}"));
        assert_json_eq(&row["window"], "7d", "window filter");
        assert!(row["id"].as_str().is_some(), "window json keeps id");
    }

    assert_json_eq(&rows[0]["resetAt"], "2026-05-11T09:00:00Z", "first reset");
    assert_json_eq(&rows[0]["sampleCount"], 2, "first window sample count");
    assert_json_eq(&rows[0]["resetKind"], "firstObserved", "first reset kind");
    assert_json_eq(&rows[1]["resetAt"], "2026-05-18T09:00:00Z", "second reset");
    assert_json_eq(&rows[1]["resetKind"], "normal", "normal reset kind");
    assert_json_eq(&rows[2]["resetAt"], "2026-05-19T09:00:00Z", "third reset");
    assert_json_eq(&rows[2]["resetKind"], "early", "early reset kind");
    assert!(rows
        .windows(2)
        .all(|pair| pair[0]["resetAt"].as_str() <= pair[1]["resetAt"].as_str()));
    assert_no_source_paths_by_default(&report, "limit windows json");

    let table = run_codex_ops(
        limit_args(
            &sandbox,
            "windows",
            &["--window", "7d", "--format", "table"],
        ),
        &sandbox,
    );
    assert_success(&table, "limit windows table");
    assert_contains(
        &table.stdout,
        "Codex rate limit windows",
        "windows table title",
    );
    assert_contains(&table.stdout, "Window", "windows table window header");
    assert_contains(&table.stdout, "Account", "windows table account header");
    assert_not_contains(&table.stdout, "ID", "windows table should omit id header");

    let default_windows = run_codex_ops(limit_args(&sandbox, "windows", &["--json"]), &sandbox);
    assert_success(&default_windows, "limit windows default json");
    let default_report = parse_json(&default_windows.stdout, "limit windows default json");
    let default_rows = assert_array(&default_report["windows"], "default window rows");
    assert!(!default_rows.is_empty(), "default windows rows");
    assert!(
        default_rows.iter().all(|row| row["window"] == "7d"),
        "limit windows defaults to 7d"
    );
}

#[test]
fn limit_account_filter_initializes_missing_account_history_from_auth() {
    let sandbox = Sandbox::new();
    fs::remove_file(&sandbox.account_history_file).expect("remove account history fixture");

    let windows = run_codex_ops(
        [
            "limit",
            "windows",
            "--window",
            "7d",
            "--account-id",
            "account-fixture",
            "--auth-file",
            sandbox.auth_file.to_str().unwrap(),
            "--sessions-dir",
            sandbox.sessions_dir.to_str().unwrap(),
            "--json",
        ],
        &sandbox,
    );
    assert_success(&windows, "limit account filter fresh history json");
    let report = parse_json(&windows.stdout, "limit account filter fresh history json");
    let rows = assert_array(&report["windows"], "fresh history account windows");
    assert!(
        !rows.is_empty(),
        "account-filtered windows should not be empty"
    );
    assert!(
        rows.iter()
            .all(|row| row["accountId"].as_str() == Some("account-fixture")),
        "account-filtered windows should be attributed to the auth account"
    );
    assert!(
        sandbox.account_history_file.exists(),
        "limit account filter should create missing account history"
    );
}

#[test]
fn limit_trend_json_table_csv_and_markdown_are_change_points() {
    let sandbox = Sandbox::new();
    add_trend_change_fixture(&sandbox);
    add_trend_vector_fixture(&sandbox);

    let trend = run_codex_ops(
        limit_args(&sandbox, "trend", &["--window", "5h", "--json"]),
        &sandbox,
    );
    assert_success(&trend, "limit trend json");
    assert_no_limit_source_leakage(&trend.stdout, &sandbox, "limit trend json");
    let report = parse_json(&trend.stdout, "limit trend json");
    assert!(
        report.get("groupBy").is_none(),
        "trend json omits old groupBy"
    );
    assert!(
        report.get("buckets").is_none(),
        "trend json omits old buckets"
    );
    let changes = assert_array(&report["changes"], "trend changes");
    for (index, change) in changes.iter().enumerate() {
        assert_limit_trend_change_schema(change, &format!("trend change {index}"));
        assert_json_eq(&change["window"], "5h", "trend window filter");
        assert!(
            change.get("minUsedPercent").is_none(),
            "old min field omitted"
        );
        assert!(
            change.get("maxUsedPercent").is_none(),
            "old max field omitted"
        );
        assert!(
            change.get("firstUsedPercent").is_none(),
            "old first field omitted"
        );
        assert!(
            change.get("lastUsedPercent").is_none(),
            "old last field omitted"
        );
    }
    let fixture_changes = changes
        .iter()
        .filter(|change| change["limitId"] == "fixture-trend-change")
        .collect::<Vec<_>>();
    assert_eq!(fixture_changes.len(), 4);
    assert!(
        fixture_changes
            .iter()
            .all(|change| change["usedPercent"] != 24.0),
        "minor same-window decrease should be compressed"
    );
    assert!(
        fixture_changes
            .iter()
            .all(|change| change["resetsAt"] != "2026-05-12T17:00:01Z"),
        "reset timestamp jitter should be compressed"
    );
    assert_json_eq(
        &fixture_changes[0]["kind"],
        "firstObserved",
        "trend first observed",
    );
    assert!(fixture_changes[0]["deltaUsedPercent"].is_null());
    assert_json_eq(&fixture_changes[1]["kind"], "increased", "trend increase");
    assert_json_eq(
        &fixture_changes[1]["usedPercent"],
        25.0,
        "trend increase used",
    );
    assert_json_eq(
        &fixture_changes[1]["deltaUsedPercent"],
        5.0,
        "trend increase delta",
    );
    assert_json_eq(&fixture_changes[2]["kind"], "decreased", "trend decrease");
    assert_json_eq(
        &fixture_changes[2]["deltaUsedPercent"],
        -10.0,
        "trend decrease delta",
    );
    assert_json_eq(
        &fixture_changes[3]["kind"],
        "resetChanged",
        "trend reset changed",
    );
    assert_json_eq(
        &fixture_changes[3]["deltaUsedPercent"],
        0.0,
        "trend reset delta",
    );
    assert_no_source_paths_by_default(&report, "limit trend json");

    let weekly_trend = run_codex_ops(limit_args(&sandbox, "trend", &["--json"]), &sandbox);
    assert_success(&weekly_trend, "limit trend 7d json");
    let weekly_report = parse_json(&weekly_trend.stdout, "limit trend 7d json");
    let weekly_changes = assert_array(&weekly_report["changes"], "weekly trend changes");
    assert!(
        weekly_changes.iter().all(|change| change["window"] == "7d"),
        "limit trend defaults to 7d"
    );
    let vector_changes = weekly_changes
        .iter()
        .filter(|change| change["limitId"] == "fixture-trend-vector")
        .collect::<Vec<_>>();
    assert_eq!(vector_changes.len(), 2);
    assert_json_eq(
        &vector_changes[0]["kind"],
        "firstObserved",
        "weekly first observed",
    );
    assert_json_eq(&vector_changes[1]["kind"], "increased", "weekly increase");
    assert_json_eq(
        &vector_changes[1]["usedPercent"],
        11.0,
        "weekly increased used",
    );

    let table = run_codex_ops(
        limit_args(&sandbox, "trend", &["--window", "5h", "--format", "table"]),
        &sandbox,
    );
    assert_success(&table, "limit trend table");
    assert_contains(&table.stdout, "Codex rate limit trend", "trend table title");
    assert_contains(&table.stdout, "Delta used", "trend table delta header");
    assert_contains(
        &table.stdout,
        "fixture-trend-change",
        "trend table limit id",
    );
    assert_not_contains(&table.stdout, "Min used", "trend table omits min");
    assert_not_contains(&table.stdout, "Max used", "trend table omits max");
    assert_not_contains(&table.stdout, "First used", "trend table omits first");
    assert_not_contains(&table.stdout, "Last used", "trend table omits last");
    assert_no_limit_source_leakage(&table.stdout, &sandbox, "limit trend table");

    let csv = run_codex_ops(
        limit_args(&sandbox, "trend", &["--window", "5h", "--format", "csv"]),
        &sandbox,
    );
    assert_success(&csv, "limit trend csv");
    let csv_rows = parse_csv(&csv.stdout);
    assert_eq!(
        csv_rows[0],
        vec![
            "At",
            "Window",
            "Account",
            "Plan",
            "Limit",
            "Used",
            "Remaining",
            "Delta used",
            "Resets at",
            "Kind",
        ],
        "limit trend csv header"
    );

    let markdown = run_codex_ops(
        limit_args(
            &sandbox,
            "trend",
            &["--window", "5h", "--format", "markdown"],
        ),
        &sandbox,
    );
    assert_success(&markdown, "limit trend markdown");
    assert_contains(
        &markdown.stdout,
        "| At | Window | Account | Plan | Limit | Used | Remaining | Delta used | Resets at | Kind |",
        "trend markdown header",
    );
    assert_not_contains(&markdown.stdout, "Min used", "trend markdown omits min");

    let markdown = run_codex_ops(
        limit_args(
            &sandbox,
            "windows",
            &["--window", "7d", "--format", "markdown"],
        ),
        &sandbox,
    );
    assert_success(&markdown, "limit windows markdown");
    assert_contains(
        &markdown.stdout,
        "| Window | Account | Plan |",
        "windows markdown header",
    );
    assert_not_contains(
        &markdown.stdout,
        "| ID |",
        "windows markdown should omit id header",
    );
    assert_no_limit_source_leakage(&markdown.stdout, &sandbox, "limit windows markdown");
}

#[test]
fn limit_resets_json_and_early_only_filter_are_stable() {
    let sandbox = Sandbox::new();

    let all = run_codex_ops(limit_args(&sandbox, "resets", &["--json"]), &sandbox);
    assert_success(&all, "limit resets json");
    assert_no_limit_source_leakage(&all.stdout, &sandbox, "limit resets json");
    let report = parse_json(&all.stdout, "limit resets json");
    let rows = assert_array(&report["resets"], "reset rows");
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter().all(|row| row["window"] == "7d"),
        "limit resets defaults to 7d"
    );
    for (index, row) in rows.iter().enumerate() {
        assert_limit_reset_schema(row, &format!("reset row {index}"));
    }
    assert!(rows.iter().any(|row| row["kind"] == "normal"));
    assert!(rows.iter().any(|row| row["kind"] == "early"));

    let early = run_codex_ops(
        limit_args(
            &sandbox,
            "resets",
            &["--window", "7d", "--early-only", "--json"],
        ),
        &sandbox,
    );
    assert_success(&early, "limit resets early json");
    assert_no_limit_source_leakage(&early.stdout, &sandbox, "limit resets early json");
    let early_report = parse_json(&early.stdout, "limit resets early json");
    let early_rows = assert_array(&early_report["resets"], "early reset rows");
    assert_eq!(early_rows.len(), 1);
    assert_limit_reset_schema(&early_rows[0], "early reset row");
    assert_json_eq(&early_rows[0]["kind"], "early", "early reset kind");
    assert_json_eq(&early_rows[0]["earlyBySeconds"], 507_600, "early seconds");
    assert_json_eq(&early_rows[0]["previousUsedPercent"], 91.0, "previous used");
    assert_json_eq(&early_rows[0]["nextUsedPercent"], 4.0, "next used");
    assert_no_source_paths_by_default(&early_report, "limit resets early json");
}

#[test]
fn limit_samples_json_csv_and_verbose_privacy_are_stable() {
    let sandbox = Sandbox::new();

    let samples = run_codex_ops(limit_args(&sandbox, "samples", &["--json"]), &sandbox);
    assert_success(&samples, "limit samples json");
    assert_no_limit_source_leakage(&samples.stdout, &sandbox, "limit samples json");
    let report = parse_json(&samples.stdout, "limit samples json");
    let rows = assert_array(&report["samples"], "sample rows");
    assert_eq!(rows.len(), 5);
    assert!(
        rows.iter().all(|row| row["window"] == "7d"),
        "limit samples defaults to 7d"
    );
    assert_limit_sample_schema(&rows[0], "limit sample row");
    assert_json_eq(&rows[0]["window"], "7d", "sample window");
    assert_no_source_paths_by_default(&report, "limit samples json");

    let csv = run_codex_ops(
        limit_args(&sandbox, "samples", &["--window", "7d", "--format", "csv"]),
        &sandbox,
    );
    assert_success(&csv, "limit samples csv");
    assert_no_limit_source_leakage(&csv.stdout, &sandbox, "limit samples csv");
    let rows = parse_csv(&csv.stdout);
    assert_eq!(
        rows[0],
        vec![
            "timestamp",
            "sessionId",
            "accountId",
            "planType",
            "limitId",
            "window",
            "windowMinutes",
            "usedPercent",
            "remainingPercent",
            "resetsAt",
        ],
        "limit samples csv header"
    );
    assert_eq!(rows.len(), 6);

    let verbose = run_codex_ops(
        limit_args(
            &sandbox,
            "samples",
            &["--window", "7d", "--json", "--verbose"],
        ),
        &sandbox,
    );
    assert_success(&verbose, "limit samples verbose json");
    let verbose_report = parse_json(&verbose.stdout, "limit samples verbose json");
    assert_limit_diagnostics_schema(&verbose_report["diagnostics"], "limit diagnostics");
    assert_json_eq(
        &verbose_report["diagnostics"]["unknownLimitSamples"],
        0,
        "unknown limit samples",
    );
    assert_json_eq(
        &verbose_report["diagnostics"]["unknownLimitResetEvents"],
        0,
        "unknown limit reset events",
    );
    let evidence = assert_array(
        &verbose_report["diagnostics"]["sourceEvidence"],
        "source evidence",
    );
    assert_eq!(evidence.len(), 5);
    assert!(evidence[0]["path"]
        .as_str()
        .expect("source path")
        .ends_with(".jsonl"));
    assert!(evidence[0]["lineNumber"].as_u64().expect("line number") > 0);
}

fn limit_args(sandbox: &Sandbox, command: &str, extra: &[&str]) -> Vec<String> {
    let mut args = vec!["limit".to_string(), command.to_string()];
    args.extend(extra.iter().map(|value| value.to_string()));
    args.extend([
        "--sessions-dir".to_string(),
        sandbox.sessions_dir.to_string_lossy().to_string(),
        "--account-history-file".to_string(),
        sandbox.account_history_file.to_string_lossy().to_string(),
    ]);
    args
}

fn add_nonstandard_primary_fixture(sandbox: &Sandbox) {
    let dir = sandbox.sessions_dir.join("2026/05/16");
    fs::create_dir_all(&dir).expect("create nonstandard primary fixture dir");
    let path = dir.join("rollout-2026-05-16T23-00-00-rust-run-session-nonstandard.jsonl");
    let reset_at = DateTime::parse_from_rfc3339("2026-05-17T02:00:00Z")
        .expect("reset time")
        .timestamp();
    let line = json!({
        "timestamp": "2026-05-16T23:00:00.000Z",
        "type": "event_msg",
        "payload": {
            "rate_limits": {
                "primary": {
                    "window_minutes": 60,
                    "used_percent": 11.0,
                    "resets_at": reset_at
                },
                "plan_type": "team",
                "limit_id": "fixture-nonstandard-primary"
            }
        }
    })
    .to_string();
    fs::write(path, format!("{line}\n")).expect("write nonstandard primary fixture");
}

fn add_zero_minute_window_fixture(sandbox: &Sandbox) {
    let dir = sandbox.sessions_dir.join("2026/05/16");
    fs::create_dir_all(&dir).expect("create zero-minute fixture dir");
    let path = dir.join("rollout-2026-05-16T23-05-00-rust-run-session-zero-window.jsonl");
    let reset_at = DateTime::parse_from_rfc3339("2026-05-17T02:00:00Z")
        .expect("reset time")
        .timestamp();
    let line = json!({
        "timestamp": "2026-05-16T23:05:00.000Z",
        "type": "event_msg",
        "payload": {
            "rate_limits": {
                "primary": {
                    "window_minutes": 0,
                    "used_percent": 88.0,
                    "resets_at": reset_at
                },
                "plan_type": "team",
                "limit_id": "fixture-zero-minute-primary"
            }
        }
    })
    .to_string();
    fs::write(path, format!("{line}\n")).expect("write zero-minute fixture");
}

fn add_trend_change_fixture(sandbox: &Sandbox) {
    let dir = sandbox.sessions_dir.join("2026/05/12");
    fs::create_dir_all(&dir).expect("create trend fixture dir");
    let path = dir.join("rollout-2026-05-12T12-20-00-rust-run-session-trend.jsonl");
    let first_reset = unix_seconds("2026-05-12T17:00:00Z");
    let jittered_first_reset = unix_seconds("2026-05-12T17:00:01Z");
    let next_reset = unix_seconds("2026-05-12T18:00:00Z");
    let lines = [
        rate_limit_line("2026-05-12T12:20:00.000Z", 20.0, first_reset),
        rate_limit_line("2026-05-12T12:21:00.000Z", 20.0, first_reset),
        rate_limit_line("2026-05-12T12:22:00.000Z", 20.0, jittered_first_reset),
        rate_limit_line("2026-05-12T12:23:00.000Z", 25.0, first_reset),
        rate_limit_line("2026-05-12T12:24:00.000Z", 24.0, first_reset),
        rate_limit_line("2026-05-12T12:25:00.000Z", 15.0, first_reset),
        rate_limit_line("2026-05-12T12:26:00.000Z", 15.0, jittered_first_reset),
        rate_limit_line("2026-05-12T12:27:00.000Z", 15.0, next_reset),
    ];
    fs::write(path, format!("{}\n", lines.join("\n"))).expect("write trend fixture");
}

fn add_trend_vector_fixture(sandbox: &Sandbox) {
    let dir = sandbox.sessions_dir.join("2026/05/12");
    fs::create_dir_all(&dir).expect("create trend vector fixture dir");
    let path = dir.join("rollout-2026-05-12T12-30-00-rust-run-session-trend-vector.jsonl");
    let five_hour_reset = unix_seconds("2026-05-12T17:30:00Z");
    let weekly_reset = unix_seconds("2026-05-19T17:30:00Z");
    let lines = [
        rate_limit_vector_line(
            "2026-05-12T12:30:00.000Z",
            0.0,
            five_hour_reset,
            10.0,
            weekly_reset,
        ),
        rate_limit_vector_line(
            "2026-05-12T12:31:00.000Z",
            1.0,
            five_hour_reset,
            10.0,
            weekly_reset,
        ),
        rate_limit_vector_line(
            "2026-05-12T12:32:00.000Z",
            2.0,
            five_hour_reset,
            11.0,
            weekly_reset,
        ),
    ];
    fs::write(path, format!("{}\n", lines.join("\n"))).expect("write trend vector fixture");
}

fn rate_limit_line(timestamp: &str, used_percent: f64, resets_at: i64) -> String {
    json!({
        "timestamp": timestamp,
        "type": "event_msg",
        "payload": {
            "rate_limits": {
                "primary": {
                    "window_minutes": 300,
                    "used_percent": used_percent,
                    "resets_at": resets_at
                },
                "plan_type": "pro",
                "limit_id": "fixture-trend-change"
            }
        }
    })
    .to_string()
}

fn rate_limit_vector_line(
    timestamp: &str,
    five_hour_used_percent: f64,
    five_hour_resets_at: i64,
    weekly_used_percent: f64,
    weekly_resets_at: i64,
) -> String {
    json!({
        "timestamp": timestamp,
        "type": "event_msg",
        "payload": {
            "rate_limits": {
                "primary": {
                    "window_minutes": 300,
                    "used_percent": five_hour_used_percent,
                    "resets_at": five_hour_resets_at
                },
                "secondary": {
                    "window_minutes": 10080,
                    "used_percent": weekly_used_percent,
                    "resets_at": weekly_resets_at
                },
                "plan_type": "pro",
                "limit_id": "fixture-trend-vector"
            }
        }
    })
    .to_string()
}

fn unix_seconds(value: &str) -> i64 {
    DateTime::parse_from_rfc3339(value)
        .expect("fixture timestamp")
        .timestamp()
}

fn find_row<'a>(rows: &'a [Value], window: &str, account_id: &str, plan_type: &str) -> &'a Value {
    rows.iter()
        .find(|row| {
            row["window"] == window
                && row["accountId"] == account_id
                && row["planType"] == plan_type
        })
        .unwrap_or_else(|| {
            panic!("row not found for window={window}, account={account_id}, plan={plan_type}")
        })
}
