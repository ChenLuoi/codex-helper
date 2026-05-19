use crate::auth::{read_codex_auth_status, AuthCommandOptions};
use crate::error::AppError;
use crate::format::to_pretty_json;
use crate::pricing::{
    calculate_credit_cost, list_model_pricing, normalize_model_name,
    TokenUsage as PricingTokenUsage, CODEX_RATE_CARD_SOURCE,
};
use crate::stats::{read_usage_records_report, UsageRecordsReadOptions};
use crate::storage::{resolve_storage_paths, StorageOptions};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct DoctorOptions {
    pub auth_file: Option<PathBuf>,
    pub codex_home: Option<PathBuf>,
    pub sessions_dir: Option<PathBuf>,
    pub cycle_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Eq, PartialEq)]
pub struct DoctorCheck {
    pub name: String,
    pub status: String,
    pub message: String,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DoctorReport {
    pub now: DateTime<Utc>,
    pub codex_home: String,
    pub auth_file: String,
    pub sessions_dir: String,
    pub helper_dir: String,
    pub cycle_file: String,
    pub checks: Vec<DoctorCheck>,
}

#[derive(Debug, Clone, Default)]
struct RecentUsageSummary {
    read_files: usize,
    token_count_events: usize,
    included_usage_events: usize,
    unpriced_models: BTreeMap<String, RecentUnpricedModel>,
}

#[derive(Debug, Clone, Default)]
struct RecentUnpricedModel {
    model: String,
    calls: usize,
    total_tokens: i64,
    note: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DoctorJson<'a> {
    now: String,
    codex_home: &'a str,
    auth_file: &'a str,
    sessions_dir: &'a str,
    helper_dir: &'a str,
    cycle_file: &'a str,
    checks: &'a [DoctorCheck],
    summary: DoctorSummary,
}

#[derive(Serialize)]
struct DoctorSummary {
    errors: usize,
    warnings: usize,
}

pub fn read_doctor_report(options: &DoctorOptions, now: DateTime<Utc>) -> DoctorReport {
    let storage = resolve_storage_paths(&StorageOptions {
        codex_home: options.codex_home.clone(),
        auth_file: options.auth_file.clone(),
        cycle_file: options.cycle_file.clone(),
        sessions_dir: options.sessions_dir.clone(),
        profile_store_dir: None,
        account_history_file: None,
    });

    let mut checks = Vec::new();
    checks.push(check_node_version());
    checks.push(check_directory("Codex home", &storage.codex_home, false));
    checks.push(check_auth_file(&storage.auth_file, options, now));
    checks.push(check_directory(
        "Sessions directory",
        &storage.sessions_dir,
        false,
    ));
    checks.push(check_helper_directory(&storage.helper_dir));
    checks.push(check_cycle_store(&storage.cycle_file));
    checks.push(check_recent_usage(&storage.sessions_dir, now));
    checks.push(check_pricing());

    DoctorReport {
        now,
        codex_home: path_to_string(&storage.codex_home),
        auth_file: path_to_string(&storage.auth_file),
        sessions_dir: path_to_string(&storage.sessions_dir),
        helper_dir: path_to_string(&storage.helper_dir),
        cycle_file: path_to_string(&storage.cycle_file),
        checks,
    }
}

pub fn format_doctor_report(report: &DoctorReport, json: bool) -> Result<String, AppError> {
    if json {
        let value = DoctorJson {
            now: format_iso(report.now),
            codex_home: &report.codex_home,
            auth_file: &report.auth_file,
            sessions_dir: &report.sessions_dir,
            helper_dir: &report.helper_dir,
            cycle_file: &report.cycle_file,
            checks: &report.checks,
            summary: DoctorSummary {
                errors: report
                    .checks
                    .iter()
                    .filter(|check| check.status == "error")
                    .count(),
                warnings: report
                    .checks
                    .iter()
                    .filter(|check| check.status == "warn")
                    .count(),
            },
        };

        return Ok(format!(
            "{}\n",
            to_pretty_json(&value).map_err(|error| AppError::new(error.to_string()))?
        ));
    }

    let mut lines = vec![
        "Codex Ops doctor".to_string(),
        format!("Codex home: {}", report.codex_home),
        format!("Auth file: {}", report.auth_file),
        format!("Sessions dir: {}", report.sessions_dir),
        format!("Helper dir: {}", report.helper_dir),
        format!("Cycle file: {}", report.cycle_file),
        String::new(),
    ];

    for check in &report.checks {
        lines.push(format!(
            "[{}] {}: {}",
            check.status, check.name, check.message
        ));
        for detail in &check.details {
            lines.push(format!("  {detail}"));
        }
    }

    let errors = report
        .checks
        .iter()
        .filter(|check| check.status == "error")
        .count();
    let warnings = report
        .checks
        .iter()
        .filter(|check| check.status == "warn")
        .count();
    lines.push(String::new());
    lines.push(format!("Result: {errors} error(s), {warnings} warning(s)"));
    Ok(format!("{}\n", lines.join("\n")))
}

fn check_node_version() -> DoctorCheck {
    match Command::new("node").arg("--version").output() {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let major = version
                .strip_prefix('v')
                .and_then(|value| value.split('.').next())
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or_default();

            if major >= 20 {
                ok(
                    "Node.js",
                    format!("{version} satisfies >=20.0.0"),
                    Vec::new(),
                )
            } else {
                check_error(
                    "Node.js",
                    format!("{version} is below the required >=20.0.0"),
                    Vec::new(),
                )
            }
        }
        Ok(output) => check_error(
            "Node.js",
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
            Vec::new(),
        ),
        Err(error) => check_error("Node.js", error.to_string(), Vec::new()),
    }
}

fn check_auth_file(auth_file: &Path, options: &DoctorOptions, now: DateTime<Utc>) -> DoctorCheck {
    match read_codex_auth_status(
        &AuthCommandOptions {
            auth_file: Some(auth_file.to_path_buf()),
            codex_home: options.codex_home.clone(),
            store_dir: None,
            account_history_file: None,
        },
        now,
    ) {
        Ok(report) => {
            let summary = report.summary;
            let label = summary
                .email
                .as_deref()
                .or(summary.name.as_deref())
                .or(summary.user_id.as_deref())
                .unwrap_or("authenticated");
            let mut details = vec![
                format!(
                    "Account: {}",
                    summary
                        .chatgpt_account_id
                        .as_deref()
                        .or(summary.token_account_id.as_deref())
                        .unwrap_or("unknown")
                ),
                format!(
                    "Plan: {}",
                    summary.plan_type.as_deref().unwrap_or("unknown")
                ),
            ];
            if let Some(expires_at) = summary.expires_at {
                details.push(format!("Token expires: {expires_at}"));
            }

            if summary.is_expired == Some(true) {
                warn(
                    "Auth file",
                    format!(
                        "Decoded {}, but the ID token is expired",
                        path_to_string(auth_file)
                    ),
                    details,
                )
            } else {
                ok(
                    "Auth file",
                    format!("Decoded {} for {label}", path_to_string(auth_file)),
                    details,
                )
            }
        }
        Err(error) if error.message().starts_with("ENOENT:") => warn(
            "Auth file",
            format!("Missing auth.json at {}", path_to_string(auth_file)),
            Vec::new(),
        ),
        Err(error) => check_error("Auth file", error.message().to_string(), Vec::new()),
    }
}

fn check_directory(name: &str, path: &Path, writable: bool) -> DoctorCheck {
    match fs::metadata(path) {
        Ok(info) if !info.is_dir() => check_error(
            name,
            format!("{} exists but is not a directory", path_to_string(path)),
            Vec::new(),
        ),
        Ok(_) => {
            if let Err(error) = fs::read_dir(path) {
                return check_error(name, error.to_string(), Vec::new());
            }
            if writable && is_readonly(path) {
                return check_error(name, "permission denied".to_string(), Vec::new());
            }
            ok(
                name,
                format!("{} is accessible", path_to_string(path)),
                Vec::new(),
            )
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => warn(
            name,
            format!("{} does not exist", path_to_string(path)),
            Vec::new(),
        ),
        Err(error) => check_error(name, error.to_string(), Vec::new()),
    }
}

fn check_helper_directory(helper_dir: &Path) -> DoctorCheck {
    match fs::metadata(helper_dir) {
        Ok(info) if !info.is_dir() => check_error(
            "Helper directory",
            format!(
                "{} exists but is not a directory",
                path_to_string(helper_dir)
            ),
            Vec::new(),
        ),
        Ok(_) => {
            if let Err(error) = fs::read_dir(helper_dir) {
                return check_error("Helper directory", error.to_string(), Vec::new());
            }
            if is_readonly(helper_dir) {
                return check_error(
                    "Helper directory",
                    "permission denied".to_string(),
                    Vec::new(),
                );
            }
            ok(
                "Helper directory",
                format!("{} is readable and writable", path_to_string(helper_dir)),
                Vec::new(),
            )
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => ok(
            "Helper directory",
            format!(
                "{} does not exist yet; helper commands will create it",
                path_to_string(helper_dir)
            ),
            Vec::new(),
        ),
        Err(error) => check_error("Helper directory", error.to_string(), Vec::new()),
    }
}

fn check_cycle_store(cycle_file: &Path) -> DoctorCheck {
    let content = match fs::read_to_string(cycle_file) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return ok(
                "Cycle store",
                format!("{} does not exist yet", path_to_string(cycle_file)),
                Vec::new(),
            )
        }
        Err(error) => return check_error("Cycle store", error.to_string(), Vec::new()),
    };

    match parse_cycle_store_counts(&content, cycle_file) {
        Ok((account_count, anchor_count)) => ok(
            "Cycle store",
            format!("Read {}", path_to_string(cycle_file)),
            vec![
                format!("Accounts: {account_count}"),
                format!("Weekly anchors: {anchor_count}"),
            ],
        ),
        Err(error_message) => check_error("Cycle store", error_message, Vec::new()),
    }
}

fn check_recent_usage(sessions_dir: &Path, now: DateTime<Utc>) -> DoctorCheck {
    if !sessions_dir.exists() {
        return warn(
            "Recent usage",
            format!(
                "Cannot scan usage because {} does not exist",
                path_to_string(sessions_dir)
            ),
            Vec::new(),
        );
    }

    match read_recent_usage_summary(sessions_dir, now) {
        Ok(summary) => {
            let details = vec![
                format!("Files read: {}", summary.read_files),
                format!("Token events: {}", summary.token_count_events),
                format!("Included usage events: {}", summary.included_usage_events),
            ];

            if !summary.unpriced_models.is_empty() {
                let mut details = details;
                for model in summary.unpriced_models.values() {
                    details.push(format!(
                        "{}: {} call(s), {} token(s){}",
                        model.model,
                        model.calls,
                        model.total_tokens,
                        model
                            .note
                            .as_ref()
                            .map(|note| format!(" ({note})"))
                            .unwrap_or_default()
                    ));
                }
                return warn(
                    "Recent usage",
                    format!(
                        "{} usage event(s), with unpriced model usage found",
                        summary.included_usage_events
                    ),
                    details,
                );
            }

            if summary.included_usage_events == 0 {
                return warn(
                    "Recent usage",
                    "No token_count usage events found in the last 7 days",
                    details,
                );
            }

            ok(
                "Recent usage",
                format!(
                    "{} usage event(s) found in the last 7 days",
                    summary.included_usage_events
                ),
                details,
            )
        }
        Err(error) => check_error("Recent usage", error, Vec::new()),
    }
}

fn check_pricing() -> DoctorCheck {
    let priced = list_model_pricing();
    let unpriced_count = 0usize;
    let mut details = vec![
        format!("Source: {}", CODEX_RATE_CARD_SOURCE.name),
        format!("Checked: {}", CODEX_RATE_CARD_SOURCE.checked_at),
        format!("Credits: {}", CODEX_RATE_CARD_SOURCE.credit_to_usd),
    ];

    for model in priced.iter().filter(|model| model.note.is_some()) {
        details.push(format!(
            "{}: {}",
            model.label,
            model.note.unwrap_or_default()
        ));
    }

    ok(
        "Pricing",
        format!(
            "{} priced model(s), {} known unpriced model(s)",
            priced.len(),
            unpriced_count
        ),
        details,
    )
}

fn read_recent_usage_summary(
    sessions_dir: &Path,
    now: DateTime<Utc>,
) -> Result<RecentUsageSummary, String> {
    let start = now - Duration::days(7);
    let report = read_usage_records_report(&UsageRecordsReadOptions {
        start,
        end: now,
        sessions_dir: sessions_dir.to_path_buf(),
        scan_all_files: false,
        account_history_file: None,
        account_id: None,
    })
    .map_err(|error| error.message().to_string())?;

    let mut summary = RecentUsageSummary {
        read_files: report.diagnostics.read_files.max(0) as usize,
        token_count_events: report.diagnostics.token_count_events.max(0) as usize,
        included_usage_events: report.diagnostics.included_usage_events.max(0) as usize,
        ..RecentUsageSummary::default()
    };

    for record in report.records {
        let cost = calculate_credit_cost(
            &record.model,
            PricingTokenUsage {
                input_tokens: record.usage.input_tokens.max(0) as u64,
                cached_input_tokens: record.usage.cached_input_tokens.max(0) as u64,
                output_tokens: record.usage.output_tokens.max(0) as u64,
            },
        );
        if !cost.priced {
            let key = normalize_model_name(&record.model);
            let entry = summary
                .unpriced_models
                .entry(key)
                .or_insert_with(|| RecentUnpricedModel {
                    model: record.model.clone(),
                    calls: 0,
                    total_tokens: 0,
                    note: cost.unpriced_reason.clone(),
                });
            entry.calls += 1;
            entry.total_tokens += record.usage.total_tokens;
        }
    }

    Ok(summary)
}

fn parse_cycle_store_counts(content: &str, cycle_file: &Path) -> Result<(usize, usize), String> {
    let value: Value = serde_json::from_str(content)
        .map_err(|error| format!("Failed to parse {}: {}", path_to_string(cycle_file), error))?;
    let object = value.as_object().ok_or_else(|| {
        format!(
            "Expected {} to contain a weekly cycle store object.",
            path_to_string(cycle_file)
        )
    })?;
    if object.get("version").and_then(Value::as_i64) != Some(1) {
        return Err(format!(
            "Unsupported weekly cycle store version in {}: {}.",
            path_to_string(cycle_file),
            object
                .get("version")
                .map(Value::to_string)
                .unwrap_or_else(|| "undefined".to_string())
        ));
    }
    let accounts = object
        .get("accounts")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            format!(
                "Expected {} accounts to be an object.",
                path_to_string(cycle_file)
            )
        })?;

    let mut anchors = 0usize;
    for account in accounts.values() {
        anchors += account
            .get("weekly")
            .and_then(|weekly| weekly.get("anchors"))
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or_default();
    }

    Ok((accounts.len(), anchors))
}

fn ok(name: &str, message: impl Into<String>, details: Vec<String>) -> DoctorCheck {
    DoctorCheck {
        name: name.to_string(),
        status: "ok".to_string(),
        message: message.into(),
        details,
    }
}

fn warn(name: &str, message: impl Into<String>, details: Vec<String>) -> DoctorCheck {
    DoctorCheck {
        name: name.to_string(),
        status: "warn".to_string(),
        message: message.into(),
        details,
    }
}

fn check_error(name: &str, message: impl Into<String>, details: Vec<String>) -> DoctorCheck {
    DoctorCheck {
        name: name.to_string(),
        status: "error".to_string(),
        message: message.into(),
        details,
    }
}

fn is_readonly(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.permissions().readonly())
        .unwrap_or(false)
}

fn format_iso(date: DateTime<Utc>) -> String {
    date.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pricing_check_matches_typescript_summary() {
        let check = check_pricing();

        assert_eq!(check.name, "Pricing");
        assert_eq!(check.status, "ok");
        assert_eq!(
            check.message,
            "8 priced model(s), 0 known unpriced model(s)"
        );
        assert!(check
            .details
            .iter()
            .any(|detail| detail.contains("GPT-5.3-Codex-Spark")));
    }

    #[test]
    fn parses_cycle_store_counts() {
        let result = parse_cycle_store_counts(
            r#"{"version":1,"accounts":{"account-a":{"weekly":{"periodHours":168,"anchors":[{"id":"a"}]}}}}"#,
            Path::new("/tmp/stat-cycles.json"),
        )
        .unwrap();

        assert_eq!(result, (1, 1));
    }
}
