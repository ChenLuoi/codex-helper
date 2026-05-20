use super::accumulators::{
    UsageSessionDetailAccumulator, UsageSessionsAccumulator, UsageStatsAccumulator,
};
use super::formatters::{format_usage_session_detail, format_usage_sessions, format_usage_stats};
use super::reports::{
    UsageRecordsReadOptions, UsageRecordsReport, UsageSessionDetailReport, UsageSessionsReport,
    UsageStatsReport,
};
use super::scan::{process_usage_records, process_usage_records_parallel};
use super::{StatFormat, StatSort};
use crate::account_history::{self, AccountHistoryAccount, UsageAccountHistory};
use crate::auth::{read_codex_auth_status, AuthCommandOptions};
use crate::error::AppError;
use crate::storage::{path_to_string, resolve_storage_paths, StorageOptions};
use crate::time::{self, RawRangeOptions, StatGroupBy};
use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct StatCommandOptions {
    pub start: Option<String>,
    pub end: Option<String>,
    pub group_by: Option<String>,
    pub format: Option<String>,
    pub codex_home: Option<PathBuf>,
    pub sessions_dir: Option<PathBuf>,
    pub auth_file: Option<PathBuf>,
    pub account_history_file: Option<PathBuf>,
    pub today: bool,
    pub yesterday: bool,
    pub month: bool,
    pub all: bool,
    pub reasoning_effort: bool,
    pub account_id: Option<String>,
    pub last: Option<String>,
    pub sort: Option<String>,
    pub limit: Option<String>,
    pub top: Option<String>,
    pub detail: bool,
    pub full_scan: bool,
    pub verbose: bool,
    pub json: bool,
}

#[derive(Debug, Clone)]
pub(super) struct ResolvedStatOptions {
    pub(super) start: DateTime<Utc>,
    pub(super) end: DateTime<Utc>,
    pub(super) group_by: StatGroupBy,
    pub(super) format: StatFormat,
    pub(super) sessions_dir: PathBuf,
    pub(super) sort_by: Option<StatSort>,
    pub(super) limit: Option<usize>,
    pub(super) include_reasoning_effort: bool,
    pub(super) scan_all_files: bool,
    pub(super) verbose: bool,
    pub(super) account_id: Option<String>,
    pub(super) account_history: Option<UsageAccountHistory>,
}

#[derive(Debug, Clone)]
pub struct ResolvedStatRangeOptions {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub format: StatFormat,
    pub sessions_dir: PathBuf,
    pub verbose: bool,
}

pub fn resolve_stat_range_options_from_raw(
    raw: &StatCommandOptions,
    now: DateTime<Utc>,
) -> Result<ResolvedStatRangeOptions, AppError> {
    let format = if raw.json {
        StatFormat::Json
    } else {
        match raw.format.as_deref() {
            Some(value) => StatFormat::parse(value)?,
            None => StatFormat::Table,
        }
    };
    let range_options = raw_range_options(raw);
    let range = time::resolve_date_range(&range_options, now)?;
    if range.start > range.end {
        return Err(AppError::new(
            "The stat start time must be earlier than or equal to the end time.",
        ));
    }
    let paths = resolve_storage_paths(&StorageOptions {
        codex_home: raw.codex_home.clone(),
        auth_file: raw.auth_file.clone(),
        profile_store_dir: None,
        account_history_file: raw.account_history_file.clone(),
        cycle_file: None,
        sessions_dir: raw.sessions_dir.clone(),
    });

    Ok(ResolvedStatRangeOptions {
        start: range.start,
        end: range.end,
        format,
        sessions_dir: paths.sessions_dir,
        verbose: raw.verbose,
    })
}

pub fn read_usage_records_report(
    options: &UsageRecordsReadOptions,
) -> Result<UsageRecordsReport, AppError> {
    let account_history = match &options.account_history_file {
        Some(path) => account_history::read_optional_usage_account_history(path)?,
        None => None,
    };
    let mut records = Vec::new();
    let resolved = ResolvedStatOptions {
        start: options.start,
        end: options.end,
        group_by: StatGroupBy::Day,
        format: StatFormat::Json,
        sessions_dir: options.sessions_dir.clone(),
        sort_by: None,
        limit: None,
        include_reasoning_effort: false,
        scan_all_files: options.scan_all_files,
        verbose: false,
        account_id: account_history
            .as_ref()
            .and_then(|_| options.account_id.clone()),
        account_history,
    };
    let diagnostics =
        process_usage_records(&resolved, |record| records.push(record.to_owned_record()))?;

    Ok(UsageRecordsReport {
        start: options.start,
        end: options.end,
        sessions_dir: path_to_string(&options.sessions_dir),
        records,
        diagnostics,
    })
}

pub fn run_stat_command(
    view: Option<&str>,
    session: Option<&str>,
    options: StatCommandOptions,
    now: DateTime<Utc>,
) -> Result<String, AppError> {
    match view {
        None => {
            let resolved = resolve_stat_options(&options, now, false)?;
            let report = read_usage_stats(&resolved)?;
            format_usage_stats(&report, resolved.format, resolved.verbose)
        }
        Some("sessions") => {
            let mut resolved = resolve_stat_options(&options, now, session.is_some())?;
            if let Some(session_id) = session {
                resolved.scan_all_files = true;
                let report = read_usage_session_detail(&resolved, session_id)?;
                format_usage_session_detail(
                    &report,
                    resolved.format,
                    resolved.verbose,
                    options.detail,
                )
            } else {
                let top = match options.top.as_deref() {
                    Some(value) => Some(parse_positive_usize(value, "--top")?),
                    None => None,
                }
                .or(resolved.limit)
                .unwrap_or(10);
                let report = read_usage_sessions(&resolved, top)?;
                format_usage_sessions(&report, resolved.format, resolved.verbose)
            }
        }
        Some(other) => Err(AppError::new(format!("Unknown stat view: {other}"))),
    }
}

fn raw_range_options(raw: &StatCommandOptions) -> RawRangeOptions {
    RawRangeOptions {
        start: raw.start.clone(),
        end: raw.end.clone(),
        all: raw.all,
        today: raw.today,
        yesterday: raw.yesterday,
        month: raw.month,
        last: raw.last.clone(),
    }
}

fn resolve_stat_options(
    raw: &StatCommandOptions,
    now: DateTime<Utc>,
    force_full_scan: bool,
) -> Result<ResolvedStatOptions, AppError> {
    let format = if raw.json {
        StatFormat::Json
    } else {
        match raw.format.as_deref() {
            Some(value) => StatFormat::parse(value)?,
            None => StatFormat::Table,
        }
    };
    let range_options = raw_range_options(raw);
    let range = time::resolve_date_range(&range_options, now)?;
    if range.start > range.end {
        return Err(AppError::new(
            "The stat start time must be earlier than or equal to the end time.",
        ));
    }

    let group_by = match raw.group_by.as_deref() {
        Some(value) => StatGroupBy::parse(value)?,
        None => time::resolve_group_by(None, &range_options, &range)?,
    };
    let sort_by = match raw.sort.as_deref() {
        Some(value) => Some(StatSort::parse(value)?),
        None => None,
    };
    let limit = match raw.limit.as_deref() {
        Some(value) => Some(parse_positive_usize(value, "--limit")?),
        None => None,
    };
    let paths = resolve_storage_paths(&StorageOptions {
        codex_home: raw.codex_home.clone(),
        auth_file: raw.auth_file.clone(),
        profile_store_dir: None,
        account_history_file: raw.account_history_file.clone(),
        cycle_file: None,
        sessions_dir: raw.sessions_dir.clone(),
    });
    let account_id = normalize_optional_account_id(raw.account_id.as_deref());
    let needs_account_history = account_id.is_some() || group_by == StatGroupBy::Account;
    let account_history = if needs_account_history {
        Some(ensure_usage_account_history(
            &paths.account_history_file,
            raw,
            now,
        )?)
    } else {
        None
    };

    Ok(ResolvedStatOptions {
        start: range.start,
        end: range.end,
        group_by,
        format,
        sessions_dir: paths.sessions_dir,
        sort_by,
        limit,
        include_reasoning_effort: raw.reasoning_effort,
        scan_all_files: raw.full_scan || force_full_scan,
        verbose: raw.verbose,
        account_id,
        account_history,
    })
}

fn read_usage_stats(options: &ResolvedStatOptions) -> Result<UsageStatsReport, AppError> {
    let accumulator = UsageStatsAccumulator::new(
        options.start,
        options.end,
        options.group_by,
        path_to_string(&options.sessions_dir),
        options.include_reasoning_effort,
        options.sort_by,
        options.limit,
    );
    let (accumulator, diagnostics) = process_usage_records_parallel(options, accumulator)?;
    Ok(accumulator.finish(Some(diagnostics)))
}

fn read_usage_sessions(
    options: &ResolvedStatOptions,
    limit: usize,
) -> Result<UsageSessionsReport, AppError> {
    let accumulator = UsageSessionsAccumulator::new(
        options.start,
        options.end,
        path_to_string(&options.sessions_dir),
        options.sort_by,
        limit,
    );
    let (accumulator, diagnostics) = process_usage_records_parallel(options, accumulator)?;
    Ok(accumulator.finish(Some(diagnostics)))
}

fn read_usage_session_detail(
    options: &ResolvedStatOptions,
    session_id: &str,
) -> Result<UsageSessionDetailReport, AppError> {
    let accumulator = UsageSessionDetailAccumulator::new(
        options.start,
        options.end,
        path_to_string(&options.sessions_dir),
        options.limit,
        session_id.to_string(),
    );
    let (accumulator, diagnostics) = process_usage_records_parallel(options, accumulator)?;
    Ok(accumulator.finish(Some(diagnostics)))
}

fn ensure_usage_account_history(
    account_history_file: &Path,
    raw: &StatCommandOptions,
    now: DateTime<Utc>,
) -> Result<UsageAccountHistory, AppError> {
    let mut store = account_history::read_account_history_store(account_history_file)?;
    if store.default_account.is_none() {
        let report = read_codex_auth_status(
            &AuthCommandOptions {
                auth_file: raw.auth_file.clone(),
                codex_home: raw.codex_home.clone(),
                store_dir: None,
                account_history_file: raw.account_history_file.clone(),
            },
            now,
        )?;
        let account_id = report
            .summary
            .chatgpt_account_id
            .clone()
            .or(report.summary.token_account_id.clone())
            .ok_or_else(|| AppError::new("No account id found in auth.json."))?;
        store = account_history::ensure_default_account_in_file(
            account_history_file,
            AccountHistoryAccount::auth_json(
                account_id,
                now,
                report.summary.name.clone(),
                report.summary.email.clone(),
                report.summary.plan_type.clone(),
            ),
        )?;
    }
    account_history::usage_account_history_from_store(store)?
        .ok_or_else(|| AppError::new("No account history default account found."))
}

fn normalize_optional_account_id(value: Option<&str>) -> Option<String> {
    let normalized = value?.trim();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized.to_string())
    }
}

fn parse_positive_usize(value: &str, name: &str) -> Result<usize, AppError> {
    let parsed = value.parse::<usize>().map_err(|_| {
        AppError::invalid_input(format!(
            "Invalid {name} value. Expected a positive integer."
        ))
    })?;
    if parsed == 0 {
        return Err(AppError::invalid_input(format!(
            "Invalid {name} value. Expected a positive integer."
        )));
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_stat_command_options() {
        let options = StatCommandOptions {
            group_by: Some("model".to_string()),
            sort: Some("credits".to_string()),
            limit: Some("1".to_string()),
            reasoning_effort: true,
            all: true,
            full_scan: true,
            verbose: true,
            json: true,
            sessions_dir: Some(PathBuf::from("/tmp/sessions")),
            ..StatCommandOptions::default()
        };
        let resolved = resolve_stat_options(
            &options,
            DateTime::parse_from_rfc3339("2026-05-17T00:00:00.000Z")
                .expect("now")
                .with_timezone(&Utc),
            false,
        )
        .expect("resolve");

        assert_eq!(resolved.group_by, StatGroupBy::Model);
        assert_eq!(resolved.sort_by, Some(StatSort::Credits));
        assert_eq!(resolved.limit, Some(1));
        assert!(resolved.include_reasoning_effort);
        assert!(resolved.scan_all_files);
        assert_eq!(resolved.format, StatFormat::Json);
    }
}
