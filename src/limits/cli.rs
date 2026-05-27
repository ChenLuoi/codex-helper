use super::formatters::{
    format_limit_current, format_limit_resets, format_limit_samples, format_limit_trend,
    format_limit_windows,
};
use super::{
    attach_usage_to_limit_current, attach_usage_to_limit_windows, build_limit_current_report,
    build_limit_resets_report, build_limit_samples_report, build_limit_trend_report,
    build_limit_windows_report, limit_current_usage_range, limit_windows_usage_range,
    read_rate_limit_samples_report, LimitReportOptions, LimitWindowSelector,
    RateLimitSamplesReadOptions,
};
use crate::auth::{ensure_usage_account_history, AuthCommandOptions};
use crate::error::AppError;
use crate::stats::{read_usage_records_report, UsageRecord, UsageRecordsReadOptions};
use crate::storage::{normalize_optional_string, resolve_storage_paths, StorageOptions};
use crate::time::{self, DateBound, RawRangeOptions};
use chrono::{DateTime, Duration, Utc};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum LimitCommand {
    Current,
    Windows,
    Trend,
    Resets,
    Samples,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum LimitFormat {
    Table,
    Json,
    Csv,
    Markdown,
}

impl LimitFormat {
    fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "table" => Ok(Self::Table),
            "json" => Ok(Self::Json),
            "csv" => Ok(Self::Csv),
            "markdown" => Ok(Self::Markdown),
            _ => Err(AppError::invalid_input(
                "Invalid format value. Expected one of: table, json, csv, markdown.",
            )),
        }
    }
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct LimitCommandOptions {
    pub start: Option<String>,
    pub end: Option<String>,
    pub last: Option<String>,
    pub format: Option<String>,
    pub codex_home: Option<PathBuf>,
    pub sessions_dir: Option<PathBuf>,
    pub auth_file: Option<PathBuf>,
    pub account_history_file: Option<PathBuf>,
    pub account_id: Option<String>,
    pub window: Option<String>,
    pub early_only: bool,
    pub json: bool,
    pub verbose: bool,
}

const DEFAULT_LIMIT_RANGE_DAYS: i64 = 30;
const DEFAULT_CURRENT_RANGE_DAYS: i64 = 7;

#[derive(Debug, Clone)]
struct ResolvedLimitOptions {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    format: LimitFormat,
    sessions_dir: PathBuf,
    account_history_file: Option<PathBuf>,
    account_id: Option<String>,
    window_minutes: Option<i64>,
    early_only: bool,
    verbose: bool,
}

pub fn run_limit_command(
    command: LimitCommand,
    options: LimitCommandOptions,
    now: DateTime<Utc>,
) -> Result<String, AppError> {
    let resolved = resolve_limit_options(command, &options, now)?;
    let window_minutes = command_window_minutes(command, resolved.window_minutes);
    let samples = read_rate_limit_samples_report(&RateLimitSamplesReadOptions {
        start: resolved.start,
        end: resolved.end,
        sessions_dir: resolved.sessions_dir.clone(),
        scan_all_files: false,
        account_history_file: resolved.account_history_file.clone(),
        account_id: resolved.account_id.clone(),
        plan_type: None,
        window_minutes,
    })?;
    let report_options = LimitReportOptions {
        include_diagnostics: resolved.verbose,
        include_source_evidence: resolved.verbose && resolved.format == LimitFormat::Json,
    };

    match command {
        LimitCommand::Current => {
            let mut report = build_limit_current_report(&samples, now, report_options);
            if let Some((start, end)) = limit_current_usage_range(&report.current) {
                let records = read_limit_usage_records(&resolved, start, end)?;
                attach_usage_to_limit_current(&mut report.current, &records);
            }
            format_limit_current(&report, resolved.format, resolved.verbose)
        }
        LimitCommand::Windows => {
            let mut report = build_limit_windows_report(&samples, report_options);
            if let Some((start, end)) = limit_windows_usage_range(&report.windows) {
                let records = read_limit_usage_records(&resolved, start, end)?;
                attach_usage_to_limit_windows(&mut report.windows, &records);
            }
            format_limit_windows(&report, resolved.format, resolved.verbose)
        }
        LimitCommand::Trend => {
            let report = build_limit_trend_report(&samples, window_minutes, report_options);
            format_limit_trend(&report, resolved.format, resolved.verbose)
        }
        LimitCommand::Resets => {
            let report = build_limit_resets_report(&samples, resolved.early_only, report_options);
            format_limit_resets(&report, resolved.format, resolved.verbose)
        }
        LimitCommand::Samples => {
            let report = build_limit_samples_report(&samples, report_options);
            format_limit_samples(&report, resolved.format, resolved.verbose)
        }
    }
}

fn command_window_minutes(command: LimitCommand, window_minutes: Option<i64>) -> Option<i64> {
    match (command, window_minutes) {
        (LimitCommand::Current, None) => None,
        (_, None) => Some(LimitWindowSelector::SevenDays.window_minutes()),
        (_, Some(window_minutes)) => Some(window_minutes),
    }
}

fn read_limit_usage_records(
    resolved: &ResolvedLimitOptions,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<UsageRecord>, AppError> {
    Ok(read_usage_records_report(&UsageRecordsReadOptions {
        start,
        end,
        sessions_dir: resolved.sessions_dir.clone(),
        scan_all_files: false,
        account_history_file: resolved.account_history_file.clone(),
        account_id: resolved.account_id.clone(),
    })?
    .records)
}

fn resolve_limit_options(
    command: LimitCommand,
    raw: &LimitCommandOptions,
    now: DateTime<Utc>,
) -> Result<ResolvedLimitOptions, AppError> {
    let format = if raw.json {
        LimitFormat::Json
    } else {
        match raw.format.as_deref() {
            Some(value) => LimitFormat::parse(value)?,
            None => LimitFormat::Table,
        }
    };
    let range = resolve_limit_date_range(command, raw, now)?;
    if range.start > range.end {
        return Err(AppError::new(
            "The limit start time must be earlier than or equal to the end time.",
        ));
    }

    let paths = resolve_storage_paths(&StorageOptions {
        codex_home: raw.codex_home.clone(),
        auth_file: raw.auth_file.clone(),
        profile_store_dir: None,
        account_history_file: raw.account_history_file.clone(),
        sessions_dir: raw.sessions_dir.clone(),
    });

    let account_id = normalize_optional_string(raw.account_id.as_deref());
    if account_id.is_some() {
        ensure_usage_account_history(
            &paths.account_history_file,
            &AuthCommandOptions {
                auth_file: raw.auth_file.clone(),
                codex_home: raw.codex_home.clone(),
                store_dir: None,
                account_history_file: raw.account_history_file.clone(),
            },
            now,
        )?;
    }

    Ok(ResolvedLimitOptions {
        start: range.start,
        end: range.end,
        format,
        sessions_dir: paths.sessions_dir,
        account_history_file: Some(paths.account_history_file),
        account_id,
        window_minutes: match raw.window.as_deref() {
            Some(value) => Some(LimitWindowSelector::parse(value)?.window_minutes()),
            None => None,
        },
        early_only: raw.early_only,
        verbose: raw.verbose,
    })
}

fn resolve_limit_date_range(
    command: LimitCommand,
    raw: &LimitCommandOptions,
    now: DateTime<Utc>,
) -> Result<time::DateRange, AppError> {
    if command == LimitCommand::Current {
        if raw.start.is_some() || raw.end.is_some() || raw.last.is_some() {
            return Err(AppError::invalid_input(
                "limit current uses a fixed recent 7-day range and does not accept --start, --end, or --last.",
            ));
        }
        return Ok(time::DateRange {
            start: now - Duration::days(DEFAULT_CURRENT_RANGE_DAYS),
            end: now,
        });
    }

    if raw.start.is_none() && raw.last.is_none() {
        let end = match &raw.end {
            Some(end) => time::parse_date_bound(end, DateBound::End)?,
            None => now,
        };
        return Ok(time::DateRange {
            start: end - Duration::days(DEFAULT_LIMIT_RANGE_DAYS),
            end,
        });
    }

    time::resolve_date_range(
        &RawRangeOptions {
            start: raw.start.clone(),
            end: raw.end.clone(),
            last: raw.last.clone(),
            all: false,
            today: false,
            yesterday: false,
            month: false,
        },
        now,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn json_flag_overrides_format_and_window_values_are_fixed() {
        let resolved = resolve_limit_options(
            LimitCommand::Windows,
            &LimitCommandOptions {
                window: Some("7d".to_string()),
                format: Some("csv".to_string()),
                json: true,
                sessions_dir: Some(PathBuf::from("/tmp/sessions")),
                ..LimitCommandOptions::default()
            },
            now(),
        )
        .expect("resolve options");

        assert_eq!(resolved.window_minutes, Some(10080));
        assert_eq!(resolved.format, LimitFormat::Json);
    }

    #[test]
    fn default_range_reads_recent_thirty_days() {
        let resolved = resolve_limit_options(
            LimitCommand::Windows,
            &LimitCommandOptions {
                sessions_dir: Some(PathBuf::from("/tmp/sessions")),
                ..LimitCommandOptions::default()
            },
            now(),
        )
        .expect("resolve options");

        assert_eq!(resolved.start, now() - Duration::days(30));
        assert_eq!(resolved.end, now());
    }

    #[test]
    fn end_without_start_uses_thirty_day_lookback() {
        let resolved = resolve_limit_options(
            LimitCommand::Windows,
            &LimitCommandOptions {
                end: Some("2026-05-10T00:00:00Z".to_string()),
                sessions_dir: Some(PathBuf::from("/tmp/sessions")),
                ..LimitCommandOptions::default()
            },
            now(),
        )
        .expect("resolve options");
        let end = Utc
            .with_ymd_and_hms(2026, 5, 10, 0, 0, 0)
            .single()
            .expect("valid end");

        assert_eq!(resolved.start, end - Duration::days(30));
        assert_eq!(resolved.end, end);
    }

    #[test]
    fn explicit_last_keeps_requested_duration() {
        let resolved = resolve_limit_options(
            LimitCommand::Windows,
            &LimitCommandOptions {
                last: Some("7d".to_string()),
                sessions_dir: Some(PathBuf::from("/tmp/sessions")),
                ..LimitCommandOptions::default()
            },
            now(),
        )
        .expect("resolve options");

        assert_eq!(resolved.start, now() - Duration::days(7));
        assert_eq!(resolved.end, now());
    }

    #[test]
    fn current_range_is_fixed_to_recent_seven_days() {
        let resolved = resolve_limit_options(
            LimitCommand::Current,
            &LimitCommandOptions {
                sessions_dir: Some(PathBuf::from("/tmp/sessions")),
                ..LimitCommandOptions::default()
            },
            now(),
        )
        .expect("resolve current options");

        assert_eq!(resolved.start, now() - Duration::days(7));
        assert_eq!(resolved.end, now());
    }

    #[test]
    fn current_rejects_explicit_date_ranges() {
        let error = resolve_limit_options(
            LimitCommand::Current,
            &LimitCommandOptions {
                last: Some("30d".to_string()),
                sessions_dir: Some(PathBuf::from("/tmp/sessions")),
                ..LimitCommandOptions::default()
            },
            now(),
        )
        .expect_err("current range override");

        assert_eq!(error.exit_code(), 2);
        assert!(error.message().contains("does not accept"));
    }

    #[test]
    fn invalid_window_is_rejected() {
        let bad_window = resolve_limit_options(
            LimitCommand::Windows,
            &LimitCommandOptions {
                window: Some("1d".to_string()),
                ..LimitCommandOptions::default()
            },
            now(),
        )
        .expect_err("bad window");
        assert_eq!(bad_window.exit_code(), 2);
        assert!(bad_window.message().contains("5h"));
        assert!(bad_window.message().contains("7d"));
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 17, 0, 0, 0)
            .single()
            .expect("valid time")
    }
}
