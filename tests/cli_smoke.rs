use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const FIXED_NOW: &str = "2026-05-17T00:00:00.000Z";
const RAW_SECRETS: &[&str] = &[
    "fixture-signature",
    "synthetic-refresh-token",
    "synthetic-refresh-token-other",
];

#[test]
fn rust_cli_fixture_smoke() {
    let sandbox = Sandbox::new();

    let help = run_codex_ops(["--help"], &sandbox);
    assert_success(&help, "root help");
    assert_contains(
        &help.stdout,
        "Usage: codex-ops <command> [options]",
        "root help",
    );

    let auth_status = run_codex_ops(
        [
            "auth",
            "status",
            "--auth-file",
            sandbox.auth_file.to_str().unwrap(),
            "--json",
        ],
        &sandbox,
    );
    assert_success(&auth_status, "auth status json");
    assert_no_secrets(&auth_status, "auth status json");
    let auth = parse_json(&auth_status.stdout, "auth status json");
    assert_json_eq(
        &auth["summary"]["chatgptAccountId"],
        "account-fixture",
        "auth account id",
    );
    assert_json_eq(
        &auth["summary"]["email"],
        "fixture@example.test",
        "auth email",
    );
    assert_json_eq(&auth["tokenClaimsIncluded"], false, "auth claims default");

    let auth_save = run_codex_ops(
        [
            "auth",
            "save",
            "--auth-file",
            sandbox.auth_file.to_str().unwrap(),
            "--store-dir",
            sandbox.store_dir.to_str().unwrap(),
        ],
        &sandbox,
    );
    assert_success(&auth_save, "auth save");
    assert_contains(
        &auth_save.stdout,
        "Saved auth profile: fixture@example.test(account-fixture) - pro",
        "auth save",
    );
    assert!(
        sandbox.store_dir.join("account-fixture.json").exists(),
        "auth save did not write current profile"
    );

    let auth_list = run_codex_ops(
        [
            "auth",
            "list",
            "--auth-file",
            sandbox.auth_file.to_str().unwrap(),
            "--store-dir",
            sandbox.store_dir.to_str().unwrap(),
        ],
        &sandbox,
    );
    assert_success(&auth_list, "auth list");
    assert_no_secrets(&auth_list, "auth list");
    assert_contains(
        &auth_list.stdout,
        "Current: fixture@example.test(account-fixture) - pro",
        "auth list current",
    );
    assert_contains(
        &auth_list.stdout,
        "other@example.test(account-other) - plus",
        "auth list persisted",
    );

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
    assert_success(&stat_json, "stat json");
    let stat = parse_json(&stat_json.stdout, "stat json");
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
    assert_success(&cycle_current, "cycle current json");
    let current = parse_json(&cycle_current.stdout, "cycle current json");
    assert_json_eq(&current["status"], "active", "cycle current status");
    assert_json_eq(&current["totals"]["calls"], 3, "cycle current calls");
    assert_json_eq(
        &current["current"]["id"],
        "anc_20260510T090000000Z",
        "cycle current id",
    );

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
            "--all",
            "--format",
            "json",
        ],
        &sandbox,
    );
    assert_success(&cycle_history, "cycle history json");
    let history = parse_json(&cycle_history.stdout, "cycle history json");
    assert_json_eq(&history["status"], "ok", "cycle history status");
    assert_eq!(
        history["rows"].as_array().unwrap().len(),
        1,
        "cycle history rows"
    );

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
            "--all",
            "--format",
            "json",
        ],
        &sandbox,
    );
    assert_success(&cycle_detail, "cycle history detail json");
    let detail = parse_json(&cycle_detail.stdout, "cycle history detail json");
    assert_json_eq(&detail["status"], "ok", "cycle detail status");
    assert_json_eq(
        &detail["cycle"]["id"],
        "anc_20260510T090000000Z",
        "cycle detail id",
    );
    assert_json_eq(
        &detail["cycle"]["usage"]["totalTokens"],
        3600,
        "cycle detail tokens",
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
        "cycle list after remove",
    );
    assert_eq!(
        cycle_list_json["anchors"].as_array().unwrap().len(),
        1,
        "cycle list anchors after remove"
    );
}

struct Sandbox {
    _root: TempDir,
    home: PathBuf,
    codex_home: PathBuf,
    auth_file: PathBuf,
    sessions_dir: PathBuf,
    store_dir: PathBuf,
    account_history_file: PathBuf,
    cycle_file: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let root = TempDir::new().expect("create smoke sandbox");
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let fixture_root = repo_root.join("test/fixtures/rust-run");
        let fixture_copy = root.path().join("fixture");
        let home = root.path().join("home");

        copy_dir(&fixture_root, &fixture_copy).expect("copy rust-run fixture");
        fs::create_dir_all(&home).expect("create smoke home");

        let codex_home = fixture_copy.join("codex-home");
        let helper_dir = codex_home.join("codex-ops");

        Self {
            _root: root,
            home,
            auth_file: codex_home.join("auth.json"),
            sessions_dir: codex_home.join("sessions"),
            store_dir: helper_dir.join("auth-profiles"),
            account_history_file: helper_dir.join("auth-account-history.json"),
            cycle_file: helper_dir.join("stat-cycles.json"),
            codex_home,
        }
    }
}

struct RunResult {
    status: i32,
    stdout: String,
    stderr: String,
}

fn run_codex_ops<I, S>(args: I, sandbox: &Sandbox) -> RunResult
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::cargo_bin("codex-ops")
        .expect("codex-ops test binary")
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("CODEX_HOME", &sandbox.codex_home)
        .env("CODEX_OPS_FIXED_NOW", FIXED_NOW)
        .env("HOME", &sandbox.home)
        .assert()
        .append_context("binary", "codex-ops")
        .get_output()
        .clone();

    RunResult {
        status: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn assert_success(result: &RunResult, label: &str) {
    assert_eq!(
        result.status, 0,
        "{label}: expected exit 0, got {}\n--- stdout ---\n{}\n--- stderr ---\n{}",
        result.status, result.stdout, result.stderr
    );
    assert_no_secrets(result, label);
}

fn parse_json(stdout: &str, label: &str) -> Value {
    serde_json::from_str(stdout).unwrap_or_else(|error| {
        panic!("{label}: expected JSON output: {error}\n--- stdout ---\n{stdout}")
    })
}

fn assert_check_status(report: &Value, name: &str, expected: &str) {
    let checks = report["checks"]
        .as_array()
        .unwrap_or_else(|| panic!("doctor checks is not an array: {report}"));
    let check = checks
        .iter()
        .find(|item| item["name"] == name)
        .unwrap_or_else(|| panic!("doctor check not found: {name}"));
    assert_json_eq(&check["status"], expected, &format!("doctor check {name}"));
}

fn assert_no_secrets(result: &RunResult, label: &str) {
    for secret in RAW_SECRETS {
        assert!(
            !result.stdout.contains(secret),
            "{label}: stdout contains raw secret marker {secret}"
        );
        assert!(
            !result.stderr.contains(secret),
            "{label}: stderr contains raw secret marker {secret}"
        );
    }
}

fn assert_contains(actual: &str, expected: &str, label: &str) {
    assert!(
        predicate::str::contains(expected).eval(actual),
        "{label}: expected output to include {expected:?}\n--- output ---\n{actual}"
    );
}

fn assert_json_eq<T>(actual: &Value, expected: T, label: &str)
where
    Value: PartialEq<T>,
    T: std::fmt::Debug,
{
    assert_eq!(
        actual, &expected,
        "{label}: expected {expected:?}, got {actual}"
    );
}

fn capture_after(text: &str, prefix: &str) -> String {
    text.lines()
        .find_map(|line| line.strip_prefix(prefix))
        .map(|value| value.split_whitespace().next().unwrap_or(value).to_string())
        .unwrap_or_else(|| panic!("pattern not found: {prefix:?}\n--- output ---\n{text}"))
}

fn parse_csv(stdout: &str) -> Vec<Vec<String>> {
    stdout
        .trim()
        .lines()
        .filter(|line| !line.is_empty())
        .map(parse_csv_line)
        .collect()
}

fn parse_csv_line(line: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '"' && quoted && chars.peek() == Some(&'"') {
            current.push('"');
            chars.next();
            continue;
        }
        if ch == '"' {
            quoted = !quoted;
            continue;
        }
        if ch == ',' && !quoted {
            cells.push(current.trim().to_string());
            current.clear();
            continue;
        }
        current.push(ch);
    }

    cells.push(current.trim().to_string());
    cells
}

fn copy_dir(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let next_destination = destination.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir(&entry.path(), &next_destination)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), next_destination)?;
        }
    }
    Ok(())
}
