mod common;

use chrono::{Datelike, Duration, Local, NaiveDate};
use common::{
    assert_array, assert_contains, assert_failure_contains, assert_json_eq,
    assert_json_local_day_end, assert_json_local_day_start, assert_success,
    assert_usage_diagnostics_schema, assert_usage_totals_schema, fixed_now_utc, parse_csv,
    parse_json, run_codex_ops, Sandbox, FIXED_NOW,
};

#[test]
fn stat_json_schema_and_account_attribution_are_stable() {
    let sandbox = Sandbox::new();

    let stat_json = run_codex_ops(
        [
            "stat",
            "--all",
            "--format",
            "json",
            "--sessions-dir",
            sandbox.sessions_dir.to_str().unwrap(),
        ],
        &sandbox,
    );
    assert_success(&stat_json, "stat all json schema");
    let stat = parse_json(&stat_json.stdout, "stat all json schema");
    common::assert_has_keys(
        &stat,
        &[
            "start",
            "end",
            "groupBy",
            "rows",
            "totals",
            "warnings",
            "diagnostics",
        ],
        "stat schema",
    );
    assert!(
        !assert_array(&stat["rows"], "stat rows").is_empty(),
        "stat rows should not be empty for all-usage fixture"
    );
    assert_usage_totals_schema(&stat["totals"], "stat totals");
    assert_usage_diagnostics_schema(&stat["diagnostics"], "stat diagnostics");
    assert_json_eq(&stat["totals"]["sessions"], 2, "stat total sessions");
    assert_json_eq(&stat["totals"]["calls"], 3, "stat total calls");
    assert_json_eq(
        &stat["totals"]["usage"]["totalTokens"],
        3600,
        "stat total tokens",
    );
    assert_json_eq(
        &stat["diagnostics"]["includedUsageEvents"],
        3,
        "stat included events",
    );

    let stat_account = run_codex_ops(
        [
            "stat",
            "--all",
            "--group-by",
            "account",
            "--format",
            "json",
            "--auth-file",
            sandbox.auth_file.to_str().unwrap(),
            "--account-history-file",
            sandbox.account_history_file.to_str().unwrap(),
            "--codex-home",
            sandbox.codex_home.to_str().unwrap(),
            "--sessions-dir",
            sandbox.sessions_dir.to_str().unwrap(),
        ],
        &sandbox,
    );
    assert_success(&stat_account, "stat account json");
    let stat_account_json = parse_json(&stat_account.stdout, "stat account json");
    assert_json_eq(
        &stat_account_json["rows"][0]["key"],
        "account-fixture",
        "stat account row",
    );
}

#[test]
fn stat_time_ranges_use_fixed_now_and_local_date_bounds() {
    let sandbox = Sandbox::new();

    let stat_last = run_codex_ops(
        [
            "stat",
            "--last",
            "12h",
            "--format",
            "json",
            "--sessions-dir",
            sandbox.sessions_dir.to_str().unwrap(),
        ],
        &sandbox,
    );
    assert_success(&stat_last, "stat fixed-now range");
    let stat_last_json = parse_json(&stat_last.stdout, "stat fixed-now range");
    assert_json_eq(
        &stat_last_json["start"],
        "2026-05-16T12:00:00.000Z",
        "stat fixed-now start",
    );
    assert_json_eq(&stat_last_json["end"], FIXED_NOW, "stat fixed-now end");
    assert_json_eq(&stat_last_json["groupBy"], "hour", "stat fixed-now group");

    let fixed_local_date = fixed_now_utc().with_timezone(&Local).date_naive();
    let stat_today = run_codex_ops(
        [
            "stat",
            "--today",
            "--format",
            "json",
            "--sessions-dir",
            sandbox.sessions_dir.to_str().unwrap(),
        ],
        &sandbox,
    );
    assert_success(&stat_today, "stat today fixed-now range");
    let today = parse_json(&stat_today.stdout, "stat today fixed-now range");
    assert_json_local_day_start(&today["start"], fixed_local_date, "stat today start");
    assert_json_eq(&today["end"], FIXED_NOW, "stat today end");

    let stat_yesterday = run_codex_ops(
        [
            "stat",
            "--yesterday",
            "--format",
            "json",
            "--sessions-dir",
            sandbox.sessions_dir.to_str().unwrap(),
        ],
        &sandbox,
    );
    assert_success(&stat_yesterday, "stat yesterday fixed-now range");
    let yesterday = parse_json(&stat_yesterday.stdout, "stat yesterday fixed-now range");
    assert_json_local_day_start(
        &yesterday["start"],
        fixed_local_date - Duration::days(1),
        "stat yesterday start",
    );
    assert_json_local_day_end(
        &yesterday["end"],
        fixed_local_date - Duration::days(1),
        "stat yesterday end",
    );

    let stat_month = run_codex_ops(
        [
            "stat",
            "--month",
            "--format",
            "json",
            "--sessions-dir",
            sandbox.sessions_dir.to_str().unwrap(),
        ],
        &sandbox,
    );
    assert_success(&stat_month, "stat month fixed-now range");
    let month = parse_json(&stat_month.stdout, "stat month fixed-now range");
    assert_json_local_day_start(
        &month["start"],
        NaiveDate::from_ymd_opt(fixed_local_date.year(), fixed_local_date.month(), 1)
            .expect("month start"),
        "stat month start",
    );
    assert_json_eq(&month["end"], FIXED_NOW, "stat month end");
    assert_json_eq(&month["groupBy"], "day", "stat month group");

    let stat_explicit = run_codex_ops(
        [
            "stat",
            "--start",
            "2026-05-10",
            "--end=2026-05-11",
            "--format",
            "json",
            "--sessions-dir",
            sandbox.sessions_dir.to_str().unwrap(),
        ],
        &sandbox,
    );
    assert_success(&stat_explicit, "stat explicit range");
    let explicit = parse_json(&stat_explicit.stdout, "stat explicit range");
    assert_json_local_day_start(
        &explicit["start"],
        NaiveDate::from_ymd_opt(2026, 5, 10).expect("explicit start"),
        "stat explicit start",
    );
    assert_json_local_day_end(
        &explicit["end"],
        NaiveDate::from_ymd_opt(2026, 5, 11).expect("explicit end"),
        "stat explicit end",
    );

    let invalid_date = run_codex_ops(
        [
            "stat",
            "--start",
            "2026-02-31",
            "--sessions-dir",
            sandbox.sessions_dir.to_str().unwrap(),
        ],
        &sandbox,
    );
    assert_failure_contains(
        &invalid_date,
        2,
        "Invalid start time: 2026-02-31",
        "stat invalid date",
    );

    let huge_last = run_codex_ops(
        [
            "stat",
            "--last",
            "9223372036854775807d",
            "--sessions-dir",
            sandbox.sessions_dir.to_str().unwrap(),
        ],
        &sandbox,
    );
    assert_failure_contains(
        &huge_last,
        2,
        "Invalid --last value. Duration is too large.",
        "stat huge last",
    );
}

#[test]
fn stat_format_outputs_and_sessions_view_remain_stable() {
    let sandbox = Sandbox::new();

    let stat_table = run_codex_ops(
        [
            "stat",
            "--all",
            "--format",
            "table",
            "--sessions-dir",
            sandbox.sessions_dir.to_str().unwrap(),
        ],
        &sandbox,
    );
    assert_success(&stat_table, "stat table");
    assert_contains(&stat_table.stdout, "Codex usage", "stat table title");
    assert_contains(&stat_table.stdout, "Total", "stat table total");
    assert_contains(&stat_table.stdout, "3,600", "stat table total tokens");

    let stat_csv = run_codex_ops(
        [
            "stat",
            "--all",
            "--format",
            "csv",
            "--sessions-dir",
            sandbox.sessions_dir.to_str().unwrap(),
        ],
        &sandbox,
    );
    assert_success(&stat_csv, "stat csv");
    let csv_rows = parse_csv(&stat_csv.stdout);
    assert_eq!(
        csv_rows[0].join(","),
        "Group,Sessions,Calls,Input,Cached,Output,Reasoning,Total,Credits,USD",
        "stat csv header"
    );
    assert_eq!(csv_rows.last().unwrap()[0], "Total", "stat csv total row");

    let stat_markdown = run_codex_ops(
        [
            "stat",
            "--all",
            "--format",
            "markdown",
            "--sessions-dir",
            sandbox.sessions_dir.to_str().unwrap(),
        ],
        &sandbox,
    );
    assert_success(&stat_markdown, "stat markdown");
    assert_contains(
        &stat_markdown.stdout,
        "| Group | Sessions | Calls |",
        "stat markdown header",
    );
    assert_contains(
        &stat_markdown.stdout,
        "| Total | 2 | 3 |",
        "stat markdown total",
    );

    let stat_sessions = run_codex_ops(
        [
            "stat",
            "sessions",
            "--all",
            "--top",
            "10",
            "--format",
            "json",
            "--sessions-dir",
            sandbox.sessions_dir.to_str().unwrap(),
        ],
        &sandbox,
    );
    assert_success(&stat_sessions, "stat sessions json");
    let sessions = parse_json(&stat_sessions.stdout, "stat sessions json");
    assert_eq!(
        sessions["rows"].as_array().unwrap().len(),
        2,
        "stat sessions rows"
    );
    assert_json_eq(
        &sessions["rows"][0]["sessionId"],
        "rust-run-session-alpha",
        "stat sessions first id",
    );
    assert_json_eq(
        &sessions["totals"]["usage"]["totalTokens"],
        3600,
        "stat sessions total tokens",
    );
}
