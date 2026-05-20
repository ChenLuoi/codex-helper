use super::StatSort;
use crate::pricing::TokenUsage as PricingTokenUsage;
use crate::time::{local_to_utc, StatGroupBy};
use chrono::{DateTime, Datelike, Local, SecondsFormat, Timelike, Utc};
use serde::Serialize;

const FULL_SCAN_ACCURACY_NOTE: &str =
    "Note: This report used balanced scanning, not a full scan. It reads in-range files and checks a bounded lookback by last token_count timestamp. Use -F, --full-scan to check all pre-range rollout files for exact local token_count results.";

#[derive(Debug, Clone)]
pub struct UsageRecordsReadOptions {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub sessions_dir: std::path::PathBuf,
    pub scan_all_files: bool,
    pub account_history_file: Option<std::path::PathBuf>,
    pub account_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct UsageRecordsReport {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub sessions_dir: String,
    pub records: Vec<UsageRecord>,
    pub diagnostics: UsageDiagnostics,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_output_tokens: i64,
    pub total_tokens: i64,
}

impl TokenUsage {
    pub(super) fn add(&mut self, other: &TokenUsage) {
        self.input_tokens += other.input_tokens;
        self.cached_input_tokens += other.cached_input_tokens;
        self.output_tokens += other.output_tokens;
        self.reasoning_output_tokens += other.reasoning_output_tokens;
        self.total_tokens += other.total_tokens;
    }

    pub(super) fn is_empty(&self) -> bool {
        self.input_tokens == 0
            && self.cached_input_tokens == 0
            && self.output_tokens == 0
            && self.reasoning_output_tokens == 0
            && self.total_tokens == 0
    }

    pub(super) fn pricing_usage(&self) -> PricingTokenUsage {
        PricingTokenUsage {
            input_tokens: self.input_tokens.max(0) as u64,
            cached_input_tokens: self.cached_input_tokens.max(0) as u64,
            output_tokens: self.output_tokens.max(0) as u64,
        }
    }
}

#[derive(Clone, Debug)]
pub struct UsageRecord {
    pub timestamp: DateTime<Utc>,
    pub session_id: String,
    pub model: String,
    pub reasoning_effort: Option<String>,
    pub cwd: String,
    pub account_id: Option<String>,
    pub file_path: String,
    pub usage: TokenUsage,
}

#[derive(Clone, Copy)]
pub(super) struct UsageRecordView<'a> {
    pub(super) timestamp: DateTime<Utc>,
    pub(super) session_id: &'a str,
    pub(super) model: &'a str,
    pub(super) reasoning_effort: Option<&'a str>,
    pub(super) cwd: &'a str,
    pub(super) account_id: Option<&'a str>,
    pub(super) file_path: &'a str,
    pub(super) usage: &'a TokenUsage,
}

impl UsageRecordView<'_> {
    pub(super) fn to_owned_record(self) -> UsageRecord {
        UsageRecord {
            timestamp: self.timestamp,
            session_id: self.session_id.to_string(),
            model: self.model.to_string(),
            reasoning_effort: self.reasoning_effort.map(str::to_string),
            cwd: self.cwd.to_string(),
            account_id: self.account_id.map(str::to_string),
            file_path: self.file_path.to_string(),
            usage: self.usage.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageStatRow {
    pub(super) key: String,
    pub(super) sessions: usize,
    pub(super) calls: i64,
    pub(super) usage: TokenUsage,
    pub(super) credits: f64,
    pub(super) usd: f64,
    pub(super) priced_calls: i64,
    pub(super) unpriced_calls: i64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageUnpricedModelRow {
    pub(super) model: String,
    pub(super) pricing_key: String,
    pub(super) calls: i64,
    pub(super) total_tokens: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) note: Option<String>,
    pub(super) pricing_stub: String,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageDiagnostics {
    pub scan_all_files: bool,
    pub scanned_directories: i64,
    pub skipped_directories: i64,
    pub read_files: i64,
    pub skipped_files: i64,
    pub prefiltered_files: i64,
    pub read_lines: i64,
    pub invalid_json_lines: i64,
    pub token_count_events: i64,
    pub included_usage_events: i64,
    pub skipped_events: SkippedEvents,
    pub file_read_concurrency: i64,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkippedEvents {
    pub missing_metadata: i64,
    pub missing_usage: i64,
    pub empty_usage: i64,
    pub out_of_range: i64,
    pub account_mismatch: i64,
}

impl UsageDiagnostics {
    pub(super) fn new(file_read_concurrency: i64, scan_all_files: bool) -> Self {
        Self {
            scan_all_files,
            scanned_directories: 0,
            skipped_directories: 0,
            read_files: 0,
            skipped_files: 0,
            prefiltered_files: 0,
            read_lines: 0,
            invalid_json_lines: 0,
            token_count_events: 0,
            included_usage_events: 0,
            skipped_events: SkippedEvents::default(),
            file_read_concurrency,
        }
    }

    pub(super) fn merge_file_scan(&mut self, other: &UsageDiagnostics) {
        self.prefiltered_files += other.prefiltered_files;
        self.read_lines += other.read_lines;
        self.invalid_json_lines += other.invalid_json_lines;
        self.token_count_events += other.token_count_events;
        self.included_usage_events += other.included_usage_events;
        self.skipped_events.missing_metadata += other.skipped_events.missing_metadata;
        self.skipped_events.missing_usage += other.skipped_events.missing_usage;
        self.skipped_events.empty_usage += other.skipped_events.empty_usage;
        self.skipped_events.out_of_range += other.skipped_events.out_of_range;
        self.skipped_events.account_mismatch += other.skipped_events.account_mismatch;
    }
}

#[derive(Clone, Debug)]
pub(super) struct UsageStatsReport {
    pub(super) start: DateTime<Utc>,
    pub(super) end: DateTime<Utc>,
    pub(super) group_by: StatGroupBy,
    pub(super) include_reasoning_effort: bool,
    pub(super) sort_by: Option<StatSort>,
    pub(super) limit: Option<usize>,
    pub(super) sessions_dir: String,
    pub(super) rows: Vec<UsageStatRow>,
    pub(super) totals: UsageStatRow,
    pub(super) unpriced_models: Vec<UsageUnpricedModelRow>,
    pub(super) diagnostics: Option<UsageDiagnostics>,
}

#[derive(Clone, Debug)]
pub(super) struct UsageSessionRow {
    pub(super) session_id: String,
    pub(super) model: String,
    pub(super) cwd: String,
    pub(super) first_seen: DateTime<Utc>,
    pub(super) last_seen: DateTime<Utc>,
    pub(super) calls: i64,
    pub(super) usage: TokenUsage,
    pub(super) credits: f64,
    pub(super) usd: f64,
    pub(super) priced_calls: i64,
    pub(super) unpriced_calls: i64,
    pub(super) file_path: String,
}

#[derive(Clone, Debug)]
pub(super) struct UsageSessionEventRow {
    pub(super) timestamp: DateTime<Utc>,
    pub(super) model: String,
    pub(super) reasoning_effort: Option<String>,
    pub(super) cwd: String,
    pub(super) usage: TokenUsage,
    pub(super) credits: f64,
    pub(super) usd: f64,
    pub(super) priced: bool,
    pub(super) file_path: String,
}

#[derive(Clone, Debug)]
pub(super) struct UsageSessionCompactRow {
    pub(super) start: DateTime<Utc>,
    pub(super) end: DateTime<Utc>,
    pub(super) events: usize,
    pub(super) model: String,
    pub(super) reasoning_effort: Option<String>,
    pub(super) usage: TokenUsage,
    pub(super) credits: f64,
    pub(super) usd: f64,
    pub(super) unpriced_calls: i64,
}

#[derive(Clone, Debug)]
pub(super) struct UsageSessionsReport {
    pub(super) start: DateTime<Utc>,
    pub(super) end: DateTime<Utc>,
    pub(super) sort_by: Option<StatSort>,
    pub(super) limit: usize,
    pub(super) sessions_dir: String,
    pub(super) rows: Vec<UsageSessionRow>,
    pub(super) totals: UsageStatRow,
    pub(super) unpriced_models: Vec<UsageUnpricedModelRow>,
    pub(super) diagnostics: Option<UsageDiagnostics>,
}

#[derive(Clone, Debug)]
pub(super) struct UsageSessionDetailReport {
    pub(super) start: DateTime<Utc>,
    pub(super) end: DateTime<Utc>,
    pub(super) session_id: String,
    pub(super) limit: Option<usize>,
    pub(super) sessions_dir: String,
    pub(super) summary: Option<UsageSessionRow>,
    pub(super) rows: Vec<UsageSessionEventRow>,
    pub(super) by_model: Vec<UsageStatRow>,
    pub(super) by_cwd: Vec<UsageStatRow>,
    pub(super) by_reasoning_effort: Vec<UsageStatRow>,
    pub(super) model_switches: i64,
    pub(super) cwd_switches: i64,
    pub(super) reasoning_effort_switches: i64,
    pub(super) totals: UsageStatRow,
    pub(super) unpriced_models: Vec<UsageUnpricedModelRow>,
    pub(super) diagnostics: Option<UsageDiagnostics>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct UsageStatsJson<'a> {
    start: String,
    end: String,
    group_by: &'static str,
    include_reasoning_effort: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    sort_by: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<usize>,
    sessions_dir: &'a str,
    rows: &'a [UsageStatRow],
    totals: &'a UsageStatRow,
    unpriced_models: &'a [UsageUnpricedModelRow],
    warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostics: Option<&'a UsageDiagnostics>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct UsageSessionsJson<'a> {
    start: String,
    end: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    sort_by: Option<&'static str>,
    limit: usize,
    sessions_dir: &'a str,
    rows: Vec<UsageSessionRowJson<'a>>,
    totals: &'a UsageStatRow,
    unpriced_models: &'a [UsageUnpricedModelRow],
    warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostics: Option<&'a UsageDiagnostics>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct UsageSessionRowJson<'a> {
    session_id: &'a str,
    model: &'a str,
    cwd: &'a str,
    first_seen: String,
    last_seen: String,
    calls: i64,
    usage: &'a TokenUsage,
    credits: f64,
    usd: f64,
    priced_calls: i64,
    unpriced_calls: i64,
    file_path: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct UsageSessionDetailJson<'a> {
    start: String,
    end: String,
    session_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<usize>,
    sessions_dir: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<UsageSessionRowJson<'a>>,
    rows: Vec<UsageSessionEventRowJson<'a>>,
    by_model: &'a [UsageStatRow],
    by_cwd: &'a [UsageStatRow],
    by_reasoning_effort: &'a [UsageStatRow],
    model_switches: i64,
    cwd_switches: i64,
    reasoning_effort_switches: i64,
    totals: &'a UsageStatRow,
    unpriced_models: &'a [UsageUnpricedModelRow],
    warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostics: Option<&'a UsageDiagnostics>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct UsageSessionEventRowJson<'a> {
    timestamp: String,
    model: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<&'a str>,
    cwd: &'a str,
    usage: &'a TokenUsage,
    credits: f64,
    usd: f64,
    priced: bool,
    file_path: &'a str,
}

pub(super) fn to_usage_stats_json(report: &UsageStatsReport) -> UsageStatsJson<'_> {
    UsageStatsJson {
        start: iso_string(report.start),
        end: iso_string(report.end),
        group_by: report.group_by.as_str(),
        include_reasoning_effort: report.include_reasoning_effort,
        sort_by: report.sort_by.map(StatSort::as_str),
        limit: report.limit,
        sessions_dir: &report.sessions_dir,
        rows: &report.rows,
        totals: &report.totals,
        unpriced_models: &report.unpriced_models,
        warnings: usage_warnings(report.start, report.end, report.diagnostics.as_ref()),
        diagnostics: report.diagnostics.as_ref(),
    }
}

pub(super) fn to_usage_sessions_json(report: &UsageSessionsReport) -> UsageSessionsJson<'_> {
    UsageSessionsJson {
        start: iso_string(report.start),
        end: iso_string(report.end),
        sort_by: report.sort_by.map(StatSort::as_str),
        limit: report.limit,
        sessions_dir: &report.sessions_dir,
        rows: report.rows.iter().map(to_session_row_json).collect(),
        totals: &report.totals,
        unpriced_models: &report.unpriced_models,
        warnings: usage_warnings(report.start, report.end, report.diagnostics.as_ref()),
        diagnostics: report.diagnostics.as_ref(),
    }
}

pub(super) fn to_usage_session_detail_json(
    report: &UsageSessionDetailReport,
) -> UsageSessionDetailJson<'_> {
    UsageSessionDetailJson {
        start: iso_string(report.start),
        end: iso_string(report.end),
        session_id: &report.session_id,
        limit: report.limit,
        sessions_dir: &report.sessions_dir,
        summary: report.summary.as_ref().map(to_session_row_json),
        rows: report.rows.iter().map(to_session_event_row_json).collect(),
        by_model: &report.by_model,
        by_cwd: &report.by_cwd,
        by_reasoning_effort: &report.by_reasoning_effort,
        model_switches: report.model_switches,
        cwd_switches: report.cwd_switches,
        reasoning_effort_switches: report.reasoning_effort_switches,
        totals: &report.totals,
        unpriced_models: &report.unpriced_models,
        warnings: usage_warnings(report.start, report.end, report.diagnostics.as_ref()),
        diagnostics: report.diagnostics.as_ref(),
    }
}

fn to_session_row_json(row: &UsageSessionRow) -> UsageSessionRowJson<'_> {
    UsageSessionRowJson {
        session_id: &row.session_id,
        model: &row.model,
        cwd: &row.cwd,
        first_seen: iso_string(row.first_seen),
        last_seen: iso_string(row.last_seen),
        calls: row.calls,
        usage: &row.usage,
        credits: row.credits,
        usd: row.usd,
        priced_calls: row.priced_calls,
        unpriced_calls: row.unpriced_calls,
        file_path: &row.file_path,
    }
}

fn to_session_event_row_json(row: &UsageSessionEventRow) -> UsageSessionEventRowJson<'_> {
    UsageSessionEventRowJson {
        timestamp: iso_string(row.timestamp),
        model: &row.model,
        reasoning_effort: row.reasoning_effort.as_deref(),
        cwd: &row.cwd,
        usage: &row.usage,
        credits: row.credits,
        usd: row.usd,
        priced: row.priced,
        file_path: &row.file_path,
    }
}

pub(super) fn usage_warnings(
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    diagnostics: Option<&UsageDiagnostics>,
) -> Vec<String> {
    if should_suggest_full_scan(start, end, diagnostics) {
        vec![FULL_SCAN_ACCURACY_NOTE.to_string()]
    } else {
        Vec::new()
    }
}

pub(super) fn should_suggest_full_scan(
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    diagnostics: Option<&UsageDiagnostics>,
) -> bool {
    diagnostics
        .is_some_and(|diagnostics| !diagnostics.scan_all_files && !is_all_usage_range(start, end))
}

pub(super) fn is_all_usage_range(start: DateTime<Utc>, end: DateTime<Utc>) -> bool {
    start == local_to_utc(1900, 1, 1, 0, 0, 0, 0)
        && end == local_to_utc(9999, 12, 31, 23, 59, 59, 999)
}

pub(super) fn format_report_range(start: DateTime<Utc>, end: DateTime<Utc>) -> String {
    if is_all_usage_range(start, end) {
        "all".to_string()
    } else {
        format!("{} to {}", format_date_time(start), format_date_time(end))
    }
}

pub(super) fn format_group_by(report: &UsageStatsReport) -> String {
    if report.group_by == StatGroupBy::Model && report.include_reasoning_effort {
        "model + reasoning_effort".to_string()
    } else {
        report.group_by.as_str().to_string()
    }
}

pub(super) fn format_date_time(date: DateTime<Utc>) -> String {
    let local = date.with_timezone(&Local);
    format!(
        "{}-{:02}-{:02} {:02}:{:02}:{:02}",
        local.year(),
        local.month(),
        local.day(),
        local.hour(),
        local.minute(),
        local.second()
    )
}

fn iso_string(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}
