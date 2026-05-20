#![allow(dead_code)]
// Shared integration helpers are compiled into each test target; every target
// uses a different subset.

use assert_cmd::Command;
use chrono::{DateTime, Datelike, Local, NaiveDate, Timelike, Utc};
use predicates::prelude::*;
use serde_json::Value;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

pub const FIXED_NOW: &str = "2026-05-17T00:00:00.000Z";
pub const RAW_SECRETS: &[&str] = &[
    "fixture-signature",
    "synthetic-refresh-token",
    "synthetic-refresh-token-other",
];

pub struct Sandbox {
    _root: TempDir,
    pub home: PathBuf,
    pub codex_home: PathBuf,
    pub auth_file: PathBuf,
    pub sessions_dir: PathBuf,
    pub store_dir: PathBuf,
    pub account_history_file: PathBuf,
    pub cycle_file: PathBuf,
}

impl Sandbox {
    pub fn new() -> Self {
        let root = TempDir::new().expect("create smoke sandbox");
        let fixture_root = repo_root().join("test/fixtures/rust-run");
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

pub struct RunResult {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn run_codex_ops<I, S>(args: I, sandbox: &Sandbox) -> RunResult
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::cargo_bin("codex-ops")
        .expect("codex-ops test binary")
        .args(args)
        .current_dir(repo_root())
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

pub fn assert_success(result: &RunResult, label: &str) {
    assert_eq!(
        result.status, 0,
        "{label}: expected exit 0, got {}\n--- stdout ---\n{}\n--- stderr ---\n{}",
        result.status, result.stdout, result.stderr
    );
    assert_no_secrets(result, label);
}

pub fn assert_help(args: &[&str], expected: &[&str], sandbox: &Sandbox, label: &str) {
    let result = run_codex_ops(args.iter().copied(), sandbox);
    assert_success(&result, label);
    assert!(
        result.stderr.is_empty(),
        "{label}: help should not write stderr\n--- stderr ---\n{}",
        result.stderr
    );
    for snippet in expected {
        assert_contains(&result.stdout, snippet, label);
    }
}

pub fn assert_failure_contains(result: &RunResult, status: i32, expected: &str, label: &str) {
    assert_eq!(
        result.status, status,
        "{label}: expected exit {status}, got {}\n--- stdout ---\n{}\n--- stderr ---\n{}",
        result.status, result.stdout, result.stderr
    );
    assert!(
        result.stdout.is_empty(),
        "{label}: failure should not write stdout\n--- stdout ---\n{}",
        result.stdout
    );
    assert_contains(&result.stderr, expected, label);
    assert_no_secrets(result, label);
}

pub fn parse_json(stdout: &str, label: &str) -> Value {
    serde_json::from_str(stdout).unwrap_or_else(|error| {
        panic!("{label}: expected JSON output: {error}\n--- stdout ---\n{stdout}")
    })
}

pub fn fixed_now_utc() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(FIXED_NOW)
        .expect("fixed now")
        .with_timezone(&Utc)
}

pub fn assert_json_local_day_start(value: &Value, expected: NaiveDate, label: &str) {
    let local = parse_json_datetime(value, label).with_timezone(&Local);
    assert_eq!(
        (local.year(), local.month(), local.day()),
        (expected.year(), expected.month(), expected.day()),
        "{label}: local date"
    );
    assert_eq!(
        (
            local.hour(),
            local.minute(),
            local.second(),
            local.timestamp_subsec_millis()
        ),
        (0, 0, 0, 0),
        "{label}: local start of day"
    );
}

pub fn assert_json_local_day_end(value: &Value, expected: NaiveDate, label: &str) {
    let local = parse_json_datetime(value, label).with_timezone(&Local);
    assert_eq!(
        (local.year(), local.month(), local.day()),
        (expected.year(), expected.month(), expected.day()),
        "{label}: local date"
    );
    assert_eq!(
        (
            local.hour(),
            local.minute(),
            local.second(),
            local.timestamp_subsec_millis()
        ),
        (23, 59, 59, 999),
        "{label}: local end of day"
    );
}

pub fn parse_json_datetime(value: &Value, label: &str) -> DateTime<Utc> {
    let text = value
        .as_str()
        .unwrap_or_else(|| panic!("{label}: expected datetime string, got {value}"));
    DateTime::parse_from_rfc3339(text)
        .unwrap_or_else(|error| panic!("{label}: invalid datetime {text:?}: {error}"))
        .with_timezone(&Utc)
}

pub fn assert_check_status(report: &Value, name: &str, expected: &str) {
    let checks = report["checks"]
        .as_array()
        .unwrap_or_else(|| panic!("doctor checks is not an array: {report}"));
    let check = checks
        .iter()
        .find(|item| item["name"] == name)
        .unwrap_or_else(|| panic!("doctor check not found: {name}"));
    assert_json_eq(&check["status"], expected, &format!("doctor check {name}"));
}

pub fn assert_no_secrets(result: &RunResult, label: &str) {
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

pub fn assert_contains(actual: &str, expected: &str, label: &str) {
    assert!(
        predicate::str::contains(expected).eval(actual),
        "{label}: expected output to include {expected:?}\n--- output ---\n{actual}"
    );
}

pub fn assert_json_eq<T>(actual: &Value, expected: T, label: &str)
where
    Value: PartialEq<T>,
    T: std::fmt::Debug,
{
    assert_eq!(
        actual, &expected,
        "{label}: expected {expected:?}, got {actual}"
    );
}

pub fn assert_has_keys(value: &Value, keys: &[&str], label: &str) {
    let object = assert_object(value, label);
    for key in keys {
        assert!(
            object.contains_key(*key),
            "{label}: expected key {key:?} in {value}"
        );
    }
}

pub fn assert_object<'a>(value: &'a Value, label: &str) -> &'a serde_json::Map<String, Value> {
    value
        .as_object()
        .unwrap_or_else(|| panic!("{label}: expected JSON object, got {value}"))
}

pub fn assert_array<'a>(value: &'a Value, label: &str) -> &'a Vec<Value> {
    value
        .as_array()
        .unwrap_or_else(|| panic!("{label}: expected JSON array, got {value}"))
}

pub fn assert_usage_totals_schema(value: &Value, label: &str) {
    assert_has_keys(
        value,
        &[
            "sessions",
            "calls",
            "usage",
            "credits",
            "usd",
            "pricedCalls",
            "unpricedCalls",
        ],
        label,
    );
    assert_has_keys(
        &value["usage"],
        &[
            "inputTokens",
            "cachedInputTokens",
            "outputTokens",
            "reasoningOutputTokens",
            "totalTokens",
        ],
        &format!("{label} usage"),
    );
}

pub fn assert_usage_diagnostics_schema(value: &Value, label: &str) {
    assert_has_keys(
        value,
        &[
            "scanAllFiles",
            "scannedDirectories",
            "skippedDirectories",
            "readFiles",
            "skippedFiles",
            "prefilteredFiles",
            "readLines",
            "invalidJsonLines",
            "tokenCountEvents",
            "includedUsageEvents",
            "skippedEvents",
            "fileReadConcurrency",
        ],
        label,
    );
    assert_has_keys(
        &value["skippedEvents"],
        &[
            "missingMetadata",
            "missingUsage",
            "emptyUsage",
            "outOfRange",
            "accountMismatch",
        ],
        &format!("{label} skipped events"),
    );
}

pub fn assert_cycle_diagnostics_schema(value: &Value, label: &str) {
    assert_has_keys(
        value,
        &[
            "anchors",
            "usageRecords",
            "windows",
            "derivedWindows",
            "estimatedWindows",
            "includedUsageEvents",
            "ignoredBeforeAnchorEvents",
            "estimateBeforeAnchor",
            "unanchored",
            "usageDiagnostics",
        ],
        label,
    );
    assert_usage_diagnostics_schema(
        &value["usageDiagnostics"],
        &format!("{label} usage diagnostics"),
    );
}

pub fn read_file_bytes(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

pub fn capture_after(text: &str, prefix: &str) -> String {
    text.lines()
        .find_map(|line| line.strip_prefix(prefix))
        .map(|value| value.split_whitespace().next().unwrap_or(value).to_string())
        .unwrap_or_else(|| panic!("pattern not found: {prefix:?}\n--- output ---\n{text}"))
}

pub fn parse_csv(stdout: &str) -> Vec<Vec<String>> {
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
