use crate::auth::{
    list_codex_auth_profiles, read_codex_auth_status, AuthCommandOptions, AuthStatusSummary,
};
use crate::error::AppError;
use crate::format::{format_csv, format_integer, format_markdown_table, to_pretty_json};
use crate::pricing::{
    calculate_credit_cost, normalize_model_name, TokenUsage as PricingTokenUsage,
};
use crate::stats::{
    read_usage_records_report, resolve_stat_range_options_from_raw, StatCommandOptions, StatFormat,
    TokenUsage, UsageDiagnostics, UsageRecord, UsageRecordsReadOptions,
};
use crate::storage::{resolve_storage_paths, write_sensitive_file, StorageOptions};
use chrono::{
    DateTime, Datelike, Duration, FixedOffset, Local, LocalResult, Offset, SecondsFormat, TimeZone,
    Timelike, Utc,
};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const WEEKLY_CYCLE_STORE_VERSION: u8 = 1;
const WEEKLY_CYCLE_PERIOD_HOURS: i64 = 168;
const WEEKLY_CYCLE_PERIOD_MS: i64 = WEEKLY_CYCLE_PERIOD_HOURS * 60 * 60 * 1000;
const DEFAULT_WEEKLY_CYCLE_ACCOUNT_ID: &str = "default";

#[derive(Debug, Clone)]
pub struct CycleCommandHelps<'a> {
    pub add: &'a str,
    pub list: &'a str,
    pub remove: &'a str,
    pub current: &'a str,
    pub history: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WeeklyCycleAnchor {
    pub id: String,
    pub at: String,
    pub input: String,
    pub time_zone: String,
    pub source: String,
    #[serde(default)]
    pub note: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct WeeklyCycleAccountEntry {
    weekly: WeeklyCycleWeeklyEntry,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct WeeklyCycleWeeklyEntry {
    period_hours: i64,
    anchors: Vec<WeeklyCycleAnchor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct WeeklyCycleStore {
    version: u8,
    accounts: BTreeMap<String, WeeklyCycleAccountEntry>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum WeeklyCycleAccountSource {
    Explicit,
    ChatgptAccountId,
    TokenAccountId,
    Default,
}

impl WeeklyCycleAccountSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::ChatgptAccountId => "chatgpt_account_id",
            Self::TokenAccountId => "token_account_id",
            Self::Default => "default",
        }
    }
}

#[derive(Debug, Clone)]
struct WeeklyCycleAccountResolution {
    account_id: String,
    source: WeeklyCycleAccountSource,
}

#[derive(Debug, Clone)]
struct WeeklyCycleAnchorWithDate {
    anchor: WeeklyCycleAnchor,
    at_date: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum WeeklyCycleWindowSource {
    Manual,
    Derived,
    Estimated,
}

impl WeeklyCycleWindowSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Derived => "derived",
            Self::Estimated => "estimated",
        }
    }
}

#[derive(Debug, Clone)]
struct InternalWeeklyCycleWindow {
    start: DateTime<Utc>,
    reset_at: DateTime<Utc>,
    exclusive_end: DateTime<Utc>,
    source: WeeklyCycleWindowSource,
    anchor_id: Option<String>,
    calibration_anchor_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct WeeklyCycleUnpricedModelRow {
    model: String,
    pricing_key: String,
    calls: i64,
    total_tokens: i64,
    pricing_stub: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct WeeklyCycleUsageTotals {
    sessions: usize,
    calls: i64,
    usage: TokenUsage,
    credits: f64,
    usd: f64,
    priced_calls: i64,
    unpriced_calls: i64,
    unpriced_models: Vec<WeeklyCycleUnpricedModelRow>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct WeeklyCycleReportRow {
    sessions: usize,
    calls: i64,
    usage: TokenUsage,
    credits: f64,
    usd: f64,
    priced_calls: i64,
    unpriced_calls: i64,
    unpriced_models: Vec<WeeklyCycleUnpricedModelRow>,
    id: String,
    index: usize,
    start: String,
    reset_at: String,
    exclusive_end: String,
    source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    anchor_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    calibration_anchor_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct WeeklyCycleBreakdownRow {
    key: String,
    sessions: usize,
    calls: i64,
    usage: TokenUsage,
    credits: f64,
    usd: f64,
    priced_calls: i64,
    unpriced_calls: i64,
    unpriced_models: Vec<WeeklyCycleUnpricedModelRow>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct WeeklyCycleDiagnostics {
    anchors: usize,
    usage_records: usize,
    windows: usize,
    derived_windows: usize,
    estimated_windows: usize,
    included_usage_events: i64,
    ignored_before_anchor_events: usize,
    estimate_before_anchor: bool,
    unanchored: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage_diagnostics: Option<UsageDiagnostics>,
}

#[derive(Debug, Clone)]
struct WeeklyCycleHistoryReport {
    status: &'static str,
    period_hours: i64,
    start: Option<DateTime<Utc>>,
    end: DateTime<Utc>,
    rows: Vec<WeeklyCycleReportRow>,
    totals: WeeklyCycleUsageTotals,
    diagnostics: WeeklyCycleDiagnostics,
}

#[derive(Debug, Clone)]
struct WeeklyCycleCurrentReport {
    status: &'static str,
    period_hours: i64,
    now: DateTime<Utc>,
    current: Option<WeeklyCycleReportRow>,
    by_day: Vec<WeeklyCycleBreakdownRow>,
    by_model: Vec<WeeklyCycleBreakdownRow>,
    totals: WeeklyCycleUsageTotals,
    diagnostics: WeeklyCycleDiagnostics,
}

#[derive(Debug, Clone)]
struct WeeklyCycleDetailReport {
    status: &'static str,
    cycle_id: String,
    period_hours: i64,
    start: Option<DateTime<Utc>>,
    end: DateTime<Utc>,
    row: WeeklyCycleReportRow,
    by_day: Vec<WeeklyCycleBreakdownRow>,
    by_model: Vec<WeeklyCycleBreakdownRow>,
    totals: WeeklyCycleUsageTotals,
    diagnostics: WeeklyCycleDiagnostics,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WeeklyCycleReportContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    account_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    account_source: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cycle_file: Option<String>,
}

#[derive(Default)]
struct CycleCliOptions {
    auth_file: Option<PathBuf>,
    codex_home: Option<PathBuf>,
    cycle_file: Option<PathBuf>,
    account_history_file: Option<PathBuf>,
    sessions_dir: Option<PathBuf>,
    account_id: Option<String>,
    note: Option<String>,
    format: Option<String>,
    json: bool,
    select: bool,
    estimate_before_anchor: bool,
    stat: StatCommandOptions,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthAccountHistoryStoreForLabel {
    default_account: Option<AuthAccountHistoryAccountForLabel>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthAccountHistoryAccountForLabel {
    account_id: String,
    name: Option<String>,
    email: Option<String>,
}

pub fn run_cycle_add_from_args(
    args: &[String],
    help: &str,
    now: DateTime<Utc>,
) -> Result<String, AppError> {
    let (time_parts, options) = parse_cycle_cli_options(args, help, CycleParseMode::Add)?;
    let times = parse_cycle_add_times(&time_parts)?;
    let report = add_weekly_cycle_anchors_to_file(&options, &times, now)?;
    let account_label = resolve_cycle_account_label(&report.account_id, &options, now);
    let mut lines = Vec::new();

    if report.anchors.len() == 1 {
        let anchor = report
            .anchors
            .first()
            .ok_or_else(|| AppError::new("No weekly cycle anchor was added."))?;
        lines.push(format!("Added weekly cycle anchor: {}", anchor.id));
        lines.push(format!("At: {}", anchor.at));
    } else {
        lines.push(format!(
            "Added {} weekly cycle anchors:",
            report.anchors.len()
        ));
        for anchor in &report.anchors {
            lines.push(format!("- {} at {}", anchor.id, anchor.at));
        }
    }

    lines.push(format_cycle_account_line(
        &report.account_id,
        account_label.as_deref(),
    ));
    lines.push(format!("Cycle file: {}", report.cycle_file));
    Ok(lines.join("\n"))
}

pub fn run_cycle_list_from_args(
    args: &[String],
    help: &str,
    now: DateTime<Utc>,
) -> Result<String, AppError> {
    let (_, options) = parse_cycle_cli_options(args, help, CycleParseMode::List)?;
    let report = list_weekly_cycle_anchors_from_file(&options, now)?;
    let context = cycle_report_context(
        &report.account_id,
        report.account_source,
        &report.cycle_file,
        &options,
        now,
    );
    format_weekly_cycle_anchor_list(&report, resolve_cycle_format(&options)?, &context)
}

pub fn run_cycle_remove_from_args(
    args: &[String],
    help: &str,
    now: DateTime<Utc>,
) -> Result<String, AppError> {
    let (positionals, options) = parse_cycle_cli_options(args, help, CycleParseMode::Remove)?;
    let anchor_id = positionals.first().ok_or_else(|| {
        AppError::invalid_input(format!("error: Missing argument: anchor-id\n\n{help}"))
    })?;
    let report = remove_weekly_cycle_anchor_from_file(anchor_id, &options, now)?;
    let account_label = resolve_cycle_account_label(&report.account_id, &options, now);

    Ok(format!(
        "Removed weekly cycle anchor: {}\n{}\nCycle file: {}",
        report.anchor.id,
        format_cycle_account_line(&report.account_id, account_label.as_deref()),
        report.cycle_file
    ))
}

pub fn run_cycle_current_from_args(
    args: &[String],
    help: &str,
    now: DateTime<Utc>,
) -> Result<String, AppError> {
    let (_, options) = parse_cycle_cli_options(args, help, CycleParseMode::Current)?;
    let format = resolve_cycle_format(&options)?;
    let anchor_report = list_weekly_cycle_anchors_from_file(&options, now)?;
    let context = cycle_report_context(
        &anchor_report.account_id,
        anchor_report.account_source,
        &anchor_report.cycle_file,
        &options,
        now,
    );
    let usage = read_weekly_cycle_usage_for_current(
        &anchor_report.anchors,
        &anchor_report.account_id,
        &options,
        now,
    )?;
    let report = build_weekly_cycle_current_report(
        &anchor_report.anchors,
        usage.records,
        now,
        usage.diagnostics,
    );

    format_weekly_cycle_current(&report, format, &context)
}

pub fn run_cycle_history_from_args(
    args: &[String],
    help: &str,
    now: DateTime<Utc>,
) -> Result<String, AppError> {
    let (positionals, mut options) = parse_cycle_cli_options(args, help, CycleParseMode::History)?;
    if positionals.len() > 1 {
        return Err(AppError::invalid_input(format!(
            "error: Unexpected argument: {}\n\n{help}",
            positionals[1]
        )));
    }
    let cycle_id = positionals.first().cloned();
    if cycle_id.is_some() && options.select {
        return Err(AppError::new(
            "cycle history accepts either a cycle id or --select, not both.",
        ));
    }

    if !has_explicit_cycle_history_range(&options) {
        options.stat.all = true;
    }

    let format = resolve_cycle_format(&options)?;
    let range = resolve_stat_range_options_from_raw(&options.stat, now)?;
    let anchor_report = list_weekly_cycle_anchors_from_file(&options, now)?;
    let context = cycle_report_context(
        &anchor_report.account_id,
        anchor_report.account_source,
        &anchor_report.cycle_file,
        &options,
        now,
    );
    let usage = read_weekly_cycle_usage_for_history(
        &anchor_report.anchors,
        &anchor_report.account_id,
        &options,
        range.start,
        range.end,
    )?;
    let history = build_weekly_cycle_history_report(
        &anchor_report.anchors,
        usage.records.clone(),
        Some(range.start),
        range.end,
        options.estimate_before_anchor,
        usage.diagnostics.clone(),
    );

    if let Some(cycle_id) = cycle_id {
        let detail = build_weekly_cycle_detail_report(
            &history,
            &cycle_id,
            usage.records,
            usage.diagnostics,
        )?;
        return format_weekly_cycle_detail(&detail, format, &context);
    }

    if options.select {
        if history.rows.is_empty() {
            return Ok("No weekly cycles to select.\n".to_string());
        }
        return Err(AppError::new(
            "cycle history --select requires an interactive terminal unless a cycle id is supplied.",
        ));
    }

    format_weekly_cycle_history(&history, format, &context)
}

struct AnchorMutationReport {
    cycle_file: String,
    account_id: String,
    anchor: WeeklyCycleAnchor,
    anchors: Vec<WeeklyCycleAnchor>,
}

struct AnchorListReport {
    cycle_file: String,
    account_id: String,
    account_source: WeeklyCycleAccountSource,
    anchors: Vec<WeeklyCycleAnchor>,
}

struct CycleUsageReadResult {
    records: Vec<UsageRecord>,
    diagnostics: Option<UsageDiagnostics>,
}

fn create_empty_weekly_cycle_store() -> WeeklyCycleStore {
    WeeklyCycleStore {
        version: WEEKLY_CYCLE_STORE_VERSION,
        accounts: BTreeMap::new(),
    }
}

fn add_weekly_cycle_anchors_to_file(
    options: &CycleCliOptions,
    times: &[String],
    now: DateTime<Utc>,
) -> Result<AnchorMutationReport, AppError> {
    if times.is_empty() {
        return Err(AppError::new(
            "At least one weekly cycle anchor time is required.",
        ));
    }

    let cycle_file = resolve_cycle_file(options);
    let account = resolve_weekly_cycle_account(options, now)?;
    let mut store = read_weekly_cycle_store(&cycle_file)?;
    let mut anchors = Vec::new();
    for at in times {
        let anchor = add_weekly_cycle_anchor(
            &mut store,
            &account.account_id,
            at,
            options.note.as_deref(),
            now,
        )?;
        anchors.push(anchor);
    }
    write_weekly_cycle_store(&cycle_file, &store)?;

    Ok(AnchorMutationReport {
        cycle_file: path_to_string(&cycle_file),
        account_id: account.account_id,
        anchor: anchors
            .first()
            .cloned()
            .ok_or_else(|| AppError::new("No weekly cycle anchor was added."))?,
        anchors,
    })
}

fn list_weekly_cycle_anchors_from_file(
    options: &CycleCliOptions,
    now: DateTime<Utc>,
) -> Result<AnchorListReport, AppError> {
    let cycle_file = resolve_cycle_file(options);
    let account = resolve_weekly_cycle_account(options, now)?;
    let store = read_weekly_cycle_store(&cycle_file)?;
    Ok(AnchorListReport {
        cycle_file: path_to_string(&cycle_file),
        account_id: account.account_id.clone(),
        account_source: account.source,
        anchors: list_weekly_cycle_anchors(&store, &account.account_id),
    })
}

fn remove_weekly_cycle_anchor_from_file(
    anchor_id: &str,
    options: &CycleCliOptions,
    now: DateTime<Utc>,
) -> Result<AnchorMutationReport, AppError> {
    let cycle_file = resolve_cycle_file(options);
    let account = resolve_weekly_cycle_account(options, now)?;
    let mut store = read_weekly_cycle_store(&cycle_file)?;
    let removed = remove_weekly_cycle_anchor(&mut store, &account.account_id, anchor_id)?;
    write_weekly_cycle_store(&cycle_file, &store)?;

    Ok(AnchorMutationReport {
        cycle_file: path_to_string(&cycle_file),
        account_id: account.account_id,
        anchor: removed,
        anchors: Vec::new(),
    })
}

fn add_weekly_cycle_anchor(
    store: &mut WeeklyCycleStore,
    account_id: &str,
    at: &str,
    note: Option<&str>,
    now: DateTime<Utc>,
) -> Result<WeeklyCycleAnchor, AppError> {
    let account_id = normalize_required_id(account_id, "account id")?;
    let parsed = parse_weekly_cycle_anchor_time(at)?;
    normalize_weekly_cycle_store(store)?;
    let entry = store
        .accounts
        .entry(account_id.clone())
        .or_insert_with(create_weekly_cycle_account_entry);

    if entry
        .weekly
        .anchors
        .iter()
        .any(|anchor| anchor.at == parsed.at_iso)
    {
        return Err(AppError::new(format!(
            "Weekly cycle anchor already exists for account {account_id} at {}.",
            parsed.at_iso
        )));
    }

    let anchor = WeeklyCycleAnchor {
        id: weekly_cycle_anchor_id(parsed.at),
        at: parsed.at_iso,
        input: parsed.input,
        time_zone: parsed.time_zone,
        source: "manual".to_string(),
        note: note.unwrap_or("").to_string(),
        created_at: iso_string(now),
    };
    entry.weekly.anchors.push(anchor.clone());
    sort_weekly_cycle_anchors(&mut entry.weekly.anchors);
    Ok(anchor)
}

fn list_weekly_cycle_anchors(store: &WeeklyCycleStore, account_id: &str) -> Vec<WeeklyCycleAnchor> {
    let mut anchors = store
        .accounts
        .get(account_id)
        .map(|entry| entry.weekly.anchors.clone())
        .unwrap_or_default();
    sort_weekly_cycle_anchors(&mut anchors);
    anchors
}

fn remove_weekly_cycle_anchor(
    store: &mut WeeklyCycleStore,
    account_id: &str,
    anchor_id: &str,
) -> Result<WeeklyCycleAnchor, AppError> {
    let account_id = normalize_required_id(account_id, "account id")?;
    let anchor_id = normalize_required_id(anchor_id, "anchor id")?;
    normalize_weekly_cycle_store(store)?;
    let entry = store
        .accounts
        .entry(account_id.clone())
        .or_insert_with(create_weekly_cycle_account_entry);
    let index = entry
        .weekly
        .anchors
        .iter()
        .position(|anchor| anchor.id == anchor_id)
        .ok_or_else(|| {
            AppError::new(format!(
                "No weekly cycle anchor found for account {account_id}: {anchor_id}."
            ))
        })?;

    Ok(entry.weekly.anchors.remove(index))
}

fn read_weekly_cycle_store(cycle_file: &Path) -> Result<WeeklyCycleStore, AppError> {
    let content = match fs::read_to_string(cycle_file) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(create_empty_weekly_cycle_store())
        }
        Err(error) => return Err(AppError::new(error.to_string())),
    };
    let mut store: WeeklyCycleStore = serde_json::from_str(&content).map_err(|error| {
        AppError::new(format!(
            "Failed to parse {}: {}",
            path_to_string(cycle_file),
            error
        ))
    })?;
    normalize_weekly_cycle_store(&mut store)?;
    Ok(store)
}

fn write_weekly_cycle_store(cycle_file: &Path, store: &WeeklyCycleStore) -> Result<(), AppError> {
    let content =
        serde_json::to_string_pretty(store).map_err(|error| AppError::new(error.to_string()))?;
    write_sensitive_file(cycle_file, &format!("{content}\n"))
        .map_err(|error| AppError::new(error.to_string()))
}

fn normalize_weekly_cycle_store(store: &mut WeeklyCycleStore) -> Result<(), AppError> {
    if store.version != WEEKLY_CYCLE_STORE_VERSION {
        return Err(AppError::new(format!(
            "Unsupported weekly cycle store version: {}.",
            store.version
        )));
    }

    for (account_id, entry) in &mut store.accounts {
        if entry.weekly.period_hours != WEEKLY_CYCLE_PERIOD_HOURS {
            return Err(AppError::new(format!(
                "Expected weekly periodHours for account {account_id} to be {WEEKLY_CYCLE_PERIOD_HOURS}."
            )));
        }
        for anchor in &entry.weekly.anchors {
            if anchor.source != "manual" {
                return Err(AppError::new(
                    "Expected weekly cycle anchor source to be manual.",
                ));
            }
            assert_iso_timestamp(&anchor.at, "anchor.at")?;
            assert_iso_timestamp(&anchor.created_at, "anchor.createdAt")?;
        }
        sort_weekly_cycle_anchors(&mut entry.weekly.anchors);
    }
    Ok(())
}

fn create_weekly_cycle_account_entry() -> WeeklyCycleAccountEntry {
    WeeklyCycleAccountEntry {
        weekly: WeeklyCycleWeeklyEntry {
            period_hours: WEEKLY_CYCLE_PERIOD_HOURS,
            anchors: Vec::new(),
        },
    }
}

fn resolve_weekly_cycle_account(
    options: &CycleCliOptions,
    now: DateTime<Utc>,
) -> Result<WeeklyCycleAccountResolution, AppError> {
    if let Some(account_id) = options.account_id.as_deref() {
        return Ok(WeeklyCycleAccountResolution {
            account_id: normalize_required_id(account_id, "--account-id")?,
            source: WeeklyCycleAccountSource::Explicit,
        });
    }

    if let Some(status) = read_optional_auth_status(options, now) {
        if let Some(account_id) =
            normalize_optional_id(status.summary.chatgpt_account_id.as_deref())
        {
            return Ok(WeeklyCycleAccountResolution {
                account_id,
                source: WeeklyCycleAccountSource::ChatgptAccountId,
            });
        }
        if let Some(account_id) = normalize_optional_id(status.summary.token_account_id.as_deref())
        {
            return Ok(WeeklyCycleAccountResolution {
                account_id,
                source: WeeklyCycleAccountSource::TokenAccountId,
            });
        }
    }

    Ok(WeeklyCycleAccountResolution {
        account_id: DEFAULT_WEEKLY_CYCLE_ACCOUNT_ID.to_string(),
        source: WeeklyCycleAccountSource::Default,
    })
}

fn read_optional_auth_status(
    options: &CycleCliOptions,
    now: DateTime<Utc>,
) -> Option<crate::auth::AuthStatusReport> {
    read_codex_auth_status(&auth_options(options), now).ok()
}

fn cycle_report_context(
    account_id: &str,
    source: WeeklyCycleAccountSource,
    cycle_file: &str,
    options: &CycleCliOptions,
    now: DateTime<Utc>,
) -> WeeklyCycleReportContext {
    WeeklyCycleReportContext {
        account_id: Some(account_id.to_string()),
        account_label: resolve_cycle_account_label(account_id, options, now),
        account_source: Some(source.as_str()),
        cycle_file: Some(cycle_file.to_string()),
    }
}

fn resolve_cycle_account_label(
    account_id: &str,
    options: &CycleCliOptions,
    now: DateTime<Utc>,
) -> Option<String> {
    if let Some(status) = read_optional_auth_status(options, now) {
        let auth_account_id = status
            .summary
            .chatgpt_account_id
            .as_deref()
            .or(status.summary.token_account_id.as_deref());
        if auth_account_id == Some(account_id) {
            return format_cycle_account_label(account_id, &status.summary);
        }
    }

    if let Ok(profiles) = list_codex_auth_profiles(&auth_options(options), now) {
        if let Some(current) = profiles.current.as_ref() {
            if current.account_id == account_id {
                return format_cycle_account_label(account_id, &current.summary);
            }
        }
        for profile in &profiles.stored {
            if profile.account_id == account_id {
                return format_cycle_account_label(account_id, &profile.summary);
            }
        }
    }

    read_history_default_cycle_account_label(account_id, options)
}

fn read_history_default_cycle_account_label(
    account_id: &str,
    options: &CycleCliOptions,
) -> Option<String> {
    let path = resolve_account_history_file(options);
    let content = fs::read_to_string(path).ok()?;
    let store: AuthAccountHistoryStoreForLabel = serde_json::from_str(&content).ok()?;
    let account = store.default_account?;
    if account.account_id != account_id {
        return None;
    }
    let label = account.email.or(account.name)?;
    if label.is_empty() {
        None
    } else {
        Some(format!("{label}({account_id})"))
    }
}

fn format_cycle_account_label(account_id: &str, account: &AuthStatusSummary) -> Option<String> {
    let label = account.email.as_deref().or(account.name.as_deref())?;
    if label.is_empty() {
        None
    } else {
        Some(format!("{label}({account_id})"))
    }
}

fn format_cycle_account_line(account_id: &str, account_label: Option<&str>) -> String {
    format!("Account: {}", account_label.unwrap_or(account_id))
}

fn read_weekly_cycle_usage_for_current(
    anchors: &[WeeklyCycleAnchor],
    account_id: &str,
    options: &CycleCliOptions,
    now: DateTime<Utc>,
) -> Result<CycleUsageReadResult, AppError> {
    let Some(earliest_anchor) = earliest_anchor_date(anchors) else {
        return Ok(CycleUsageReadResult {
            records: Vec::new(),
            diagnostics: None,
        });
    };
    if earliest_anchor > now {
        return Ok(CycleUsageReadResult {
            records: Vec::new(),
            diagnostics: None,
        });
    }

    let paths = resolve_storage_paths(&StorageOptions {
        codex_home: options.codex_home.clone(),
        sessions_dir: options.sessions_dir.clone(),
        ..StorageOptions::default()
    });
    let report = read_usage_records_report(&UsageRecordsReadOptions {
        start: earliest_anchor,
        end: now,
        sessions_dir: paths.sessions_dir,
        scan_all_files: true,
        account_history_file: Some(resolve_account_history_file(options)),
        account_id: Some(account_id.to_string()),
    })?;

    Ok(CycleUsageReadResult {
        records: report.records,
        diagnostics: Some(report.diagnostics),
    })
}

fn read_weekly_cycle_usage_for_history(
    anchors: &[WeeklyCycleAnchor],
    account_id: &str,
    options: &CycleCliOptions,
    range_start: DateTime<Utc>,
    range_end: DateTime<Utc>,
) -> Result<CycleUsageReadResult, AppError> {
    let Some(earliest_anchor) = earliest_anchor_date(anchors) else {
        return Ok(CycleUsageReadResult {
            records: Vec::new(),
            diagnostics: None,
        });
    };
    let scan_start = if options.estimate_before_anchor && range_start < earliest_anchor {
        range_start
    } else {
        earliest_anchor
    };
    if scan_start > range_end {
        return Ok(CycleUsageReadResult {
            records: Vec::new(),
            diagnostics: None,
        });
    }

    let range = resolve_stat_range_options_from_raw(&options.stat, Utc::now())?;
    let report = read_usage_records_report(&UsageRecordsReadOptions {
        start: scan_start,
        end: range_end,
        sessions_dir: range.sessions_dir,
        scan_all_files: true,
        account_history_file: Some(resolve_account_history_file(options)),
        account_id: Some(account_id.to_string()),
    })?;

    Ok(CycleUsageReadResult {
        records: report.records,
        diagnostics: Some(report.diagnostics),
    })
}

fn build_weekly_cycle_history_report(
    anchors: &[WeeklyCycleAnchor],
    records: Vec<UsageRecord>,
    start: Option<DateTime<Utc>>,
    end: DateTime<Utc>,
    estimate_before_anchor: bool,
    usage_diagnostics: Option<UsageDiagnostics>,
) -> WeeklyCycleHistoryReport {
    let mut records = records;
    sort_usage_records(&mut records);
    let anchors = sort_anchors_with_dates(anchors);
    let empty_totals = empty_weekly_cycle_totals();

    if anchors.is_empty() {
        return WeeklyCycleHistoryReport {
            status: "unanchored",
            period_hours: WEEKLY_CYCLE_PERIOD_HOURS,
            start,
            end,
            rows: Vec::new(),
            totals: empty_totals.clone(),
            diagnostics: create_weekly_cycle_diagnostics(
                &anchors,
                &records,
                &[],
                &empty_totals,
                estimate_before_anchor,
                true,
                usage_diagnostics,
            ),
        };
    }

    let first_anchor = anchors.first().expect("anchor exists");
    let derived = derive_anchored_weekly_cycle_windows(&anchors, &records, end);
    let estimated = if estimate_before_anchor {
        derive_estimated_weekly_cycle_windows(first_anchor, &records, start, end)
    } else {
        Vec::new()
    };
    let mut windows = [estimated, derived].concat();
    windows.retain(|window| window_overlaps_range(window, start, end));
    windows.sort_by(compare_windows);
    let (rows, totals) = build_cycle_rows(&windows, &records, start, Some(end));

    WeeklyCycleHistoryReport {
        status: "ok",
        period_hours: WEEKLY_CYCLE_PERIOD_HOURS,
        start,
        end,
        diagnostics: create_weekly_cycle_diagnostics(
            &anchors,
            &records,
            &rows,
            &totals,
            estimate_before_anchor,
            false,
            usage_diagnostics,
        ),
        rows,
        totals,
    }
}

fn build_weekly_cycle_current_report(
    anchors: &[WeeklyCycleAnchor],
    records: Vec<UsageRecord>,
    now: DateTime<Utc>,
    usage_diagnostics: Option<UsageDiagnostics>,
) -> WeeklyCycleCurrentReport {
    let mut records = records
        .into_iter()
        .filter(|record| record.timestamp <= now)
        .collect::<Vec<_>>();
    sort_usage_records(&mut records);
    let anchors = sort_anchors_with_dates(anchors)
        .into_iter()
        .filter(|anchor| anchor.at_date <= now)
        .collect::<Vec<_>>();
    let empty_totals = empty_weekly_cycle_totals();

    if anchors.is_empty() {
        return WeeklyCycleCurrentReport {
            status: "unanchored",
            period_hours: WEEKLY_CYCLE_PERIOD_HOURS,
            now,
            current: None,
            by_day: Vec::new(),
            by_model: Vec::new(),
            totals: empty_totals.clone(),
            diagnostics: create_weekly_cycle_diagnostics(
                &anchors,
                &records,
                &[],
                &empty_totals,
                false,
                true,
                usage_diagnostics,
            ),
        };
    }

    let windows = derive_anchored_weekly_cycle_windows(&anchors, &records, now);
    let Some(current_window) = windows.last().cloned() else {
        return WeeklyCycleCurrentReport {
            status: "unanchored",
            period_hours: WEEKLY_CYCLE_PERIOD_HOURS,
            now,
            current: None,
            by_day: Vec::new(),
            by_model: Vec::new(),
            totals: empty_totals.clone(),
            diagnostics: create_weekly_cycle_diagnostics(
                &anchors,
                &records,
                &[],
                &empty_totals,
                false,
                true,
                usage_diagnostics,
            ),
        };
    };
    let (rows, totals) = build_cycle_rows(&[current_window.clone()], &records, None, None);
    let current = rows.first().cloned();
    let current_records = records
        .iter()
        .filter(|record| record_belongs_to_window(record, &current_window, None, None))
        .cloned()
        .collect::<Vec<_>>();
    let status = if current_window.reset_at <= now {
        "waiting_for_usage"
    } else {
        "active"
    };

    WeeklyCycleCurrentReport {
        status,
        period_hours: WEEKLY_CYCLE_PERIOD_HOURS,
        now,
        current,
        by_day: build_weekly_cycle_breakdown(&current_records, |record| {
            local_date_key(record.timestamp)
        }),
        by_model: build_weekly_cycle_breakdown(&current_records, |record| record.model.clone()),
        diagnostics: create_weekly_cycle_diagnostics(
            &anchors,
            &records,
            &rows,
            &totals,
            false,
            false,
            usage_diagnostics,
        ),
        totals,
    }
}

fn build_weekly_cycle_detail_report(
    history: &WeeklyCycleHistoryReport,
    cycle_id: &str,
    mut records: Vec<UsageRecord>,
    usage_diagnostics: Option<UsageDiagnostics>,
) -> Result<WeeklyCycleDetailReport, AppError> {
    let cycle_id = normalize_required_id(cycle_id, "cycle id")?;
    let row = history
        .rows
        .iter()
        .find(|row| row.id == cycle_id)
        .cloned()
        .ok_or_else(|| AppError::new(format!("No weekly cycle found for id: {cycle_id}")))?;
    sort_usage_records(&mut records);
    let row_start = parse_iso_timestamp(&row.start).expect("row start is ISO");
    let row_end = parse_iso_timestamp(&row.exclusive_end).expect("row end is ISO");
    let row_records = records
        .iter()
        .filter(|record| {
            record.timestamp >= row_start
                && record.timestamp < row_end
                && history.start.is_none_or(|start| record.timestamp >= start)
                && record.timestamp <= history.end
        })
        .cloned()
        .collect::<Vec<_>>();
    let diagnostics = WeeklyCycleDiagnostics {
        usage_records: records.len(),
        windows: 1,
        derived_windows: if row.source == "derived" { 1 } else { 0 },
        estimated_windows: if row.source == "estimated" { 1 } else { 0 },
        included_usage_events: row.calls,
        usage_diagnostics: usage_diagnostics
            .or_else(|| history.diagnostics.usage_diagnostics.clone()),
        ..history.diagnostics.clone()
    };

    Ok(WeeklyCycleDetailReport {
        status: "ok",
        cycle_id: row.id.clone(),
        period_hours: history.period_hours,
        start: history.start,
        end: history.end,
        by_day: build_weekly_cycle_breakdown(&row_records, |record| {
            local_date_key(record.timestamp)
        }),
        by_model: build_weekly_cycle_breakdown(&row_records, |record| record.model.clone()),
        totals: usage_totals_from_row(&row),
        row,
        diagnostics,
    })
}

fn derive_anchored_weekly_cycle_windows(
    anchors: &[WeeklyCycleAnchorWithDate],
    records: &[UsageRecord],
    until: DateTime<Utc>,
) -> Vec<InternalWeeklyCycleWindow> {
    let mut windows = Vec::new();

    for index in 0..anchors.len() {
        let anchor = &anchors[index];
        let next_anchor = anchors.get(index + 1);
        if anchor.at_date > until {
            continue;
        }

        let mut start = anchor.at_date;
        let mut source = WeeklyCycleWindowSource::Manual;
        let mut anchor_id = Some(anchor.anchor.id.clone());

        while start <= until {
            let calculated_reset = start + Duration::hours(WEEKLY_CYCLE_PERIOD_HOURS);
            let reset_at = if next_anchor.is_some_and(|next| next.at_date <= calculated_reset) {
                next_anchor.expect("checked").at_date
            } else {
                calculated_reset
            };
            windows.push(InternalWeeklyCycleWindow {
                start,
                reset_at,
                exclusive_end: reset_at,
                source,
                anchor_id: anchor_id.clone(),
                calibration_anchor_id: Some(anchor.anchor.id.clone()),
            });

            let next_start = records
                .iter()
                .find(|record| {
                    record.timestamp >= reset_at
                        && record.timestamp <= until
                        && next_anchor.is_none_or(|next| record.timestamp < next.at_date)
                })
                .map(|record| record.timestamp);
            let Some(next_start) = next_start else {
                break;
            };
            start = next_start;
            source = WeeklyCycleWindowSource::Derived;
            anchor_id = None;
        }
    }

    windows.sort_by(compare_windows);
    windows
}

fn derive_estimated_weekly_cycle_windows(
    first_anchor: &WeeklyCycleAnchorWithDate,
    records: &[UsageRecord],
    start: Option<DateTime<Utc>>,
    end: DateTime<Utc>,
) -> Vec<InternalWeeklyCycleWindow> {
    let mut windows: BTreeMap<i64, InternalWeeklyCycleWindow> = BTreeMap::new();
    for record in records {
        if record.timestamp >= first_anchor.at_date || record.timestamp > end {
            continue;
        }
        if start.is_some_and(|start| record.timestamp < start) {
            continue;
        }
        let diff_ms = (first_anchor.at_date - record.timestamp).num_milliseconds();
        let periods = ((diff_ms + WEEKLY_CYCLE_PERIOD_MS - 1) / WEEKLY_CYCLE_PERIOD_MS).max(1);
        let window_start =
            first_anchor.at_date - Duration::milliseconds(periods * WEEKLY_CYCLE_PERIOD_MS);
        windows
            .entry(window_start.timestamp_millis())
            .or_insert_with(|| {
                let reset_at = window_start + Duration::hours(WEEKLY_CYCLE_PERIOD_HOURS);
                InternalWeeklyCycleWindow {
                    start: window_start,
                    reset_at,
                    exclusive_end: reset_at,
                    source: WeeklyCycleWindowSource::Estimated,
                    anchor_id: None,
                    calibration_anchor_id: None,
                }
            });
    }
    windows.into_values().collect()
}

fn build_cycle_rows(
    windows: &[InternalWeeklyCycleWindow],
    records: &[UsageRecord],
    range_start: Option<DateTime<Utc>>,
    range_end: Option<DateTime<Utc>>,
) -> (Vec<WeeklyCycleReportRow>, WeeklyCycleUsageTotals) {
    let mut included = Vec::new();
    let rows = windows
        .iter()
        .enumerate()
        .map(|(index, window)| {
            let window_records = records
                .iter()
                .filter(|record| record_belongs_to_window(record, window, range_start, range_end))
                .cloned()
                .collect::<Vec<_>>();
            included.extend(window_records.clone());
            cycle_row_from_window(
                window,
                index + 1,
                aggregate_weekly_cycle_records(&window_records),
            )
        })
        .collect::<Vec<_>>();
    let totals = aggregate_weekly_cycle_records(&included);
    (rows, totals)
}

fn record_belongs_to_window(
    record: &UsageRecord,
    window: &InternalWeeklyCycleWindow,
    range_start: Option<DateTime<Utc>>,
    range_end: Option<DateTime<Utc>>,
) -> bool {
    record.timestamp >= window.start
        && record.timestamp < window.exclusive_end
        && range_start.is_none_or(|start| record.timestamp >= start)
        && range_end.is_none_or(|end| record.timestamp <= end)
}

fn aggregate_weekly_cycle_records(records: &[UsageRecord]) -> WeeklyCycleUsageTotals {
    let mut sessions = HashSet::new();
    let mut usage = TokenUsage::default();
    let mut credits = 0.0;
    let mut priced_calls = 0;
    let mut unpriced_calls = 0;
    let mut unpriced_models: HashMap<String, WeeklyCycleUnpricedModelRow> = HashMap::new();

    for record in records {
        let cost = calculate_credit_cost(
            &record.model,
            PricingTokenUsage {
                input_tokens: record.usage.input_tokens.max(0) as u64,
                cached_input_tokens: record.usage.cached_input_tokens.max(0) as u64,
                output_tokens: record.usage.output_tokens.max(0) as u64,
            },
        );
        sessions.insert(record.session_id.clone());
        usage.input_tokens += record.usage.input_tokens;
        usage.cached_input_tokens += record.usage.cached_input_tokens;
        usage.output_tokens += record.usage.output_tokens;
        usage.reasoning_output_tokens += record.usage.reasoning_output_tokens;
        usage.total_tokens += record.usage.total_tokens;
        credits += cost.credits;

        if cost.priced {
            priced_calls += 1;
        } else {
            unpriced_calls += 1;
            add_unpriced_model(&mut unpriced_models, record);
        }
    }

    WeeklyCycleUsageTotals {
        sessions: sessions.len(),
        calls: records.len() as i64,
        usage,
        credits: round_credits(credits),
        usd: credits_to_usd(credits),
        priced_calls,
        unpriced_calls,
        unpriced_models: format_unpriced_models(unpriced_models),
    }
}

fn build_weekly_cycle_breakdown(
    records: &[UsageRecord],
    key_for_record: impl Fn(&UsageRecord) -> String,
) -> Vec<WeeklyCycleBreakdownRow> {
    let mut grouped: BTreeMap<String, Vec<UsageRecord>> = BTreeMap::new();
    for record in records {
        grouped
            .entry(key_for_record(record))
            .or_default()
            .push(record.clone());
    }

    grouped
        .into_iter()
        .map(|(key, records)| WeeklyCycleBreakdownRow {
            key,
            ..breakdown_totals(aggregate_weekly_cycle_records(&records))
        })
        .collect()
}

fn breakdown_totals(totals: WeeklyCycleUsageTotals) -> WeeklyCycleBreakdownRow {
    WeeklyCycleBreakdownRow {
        key: String::new(),
        sessions: totals.sessions,
        calls: totals.calls,
        usage: totals.usage,
        credits: totals.credits,
        usd: totals.usd,
        priced_calls: totals.priced_calls,
        unpriced_calls: totals.unpriced_calls,
        unpriced_models: totals.unpriced_models,
    }
}

fn cycle_row_from_window(
    window: &InternalWeeklyCycleWindow,
    index: usize,
    totals: WeeklyCycleUsageTotals,
) -> WeeklyCycleReportRow {
    WeeklyCycleReportRow {
        sessions: totals.sessions,
        calls: totals.calls,
        usage: totals.usage,
        credits: totals.credits,
        usd: totals.usd,
        priced_calls: totals.priced_calls,
        unpriced_calls: totals.unpriced_calls,
        unpriced_models: totals.unpriced_models,
        id: weekly_cycle_window_id(window),
        index,
        start: iso_string(window.start),
        reset_at: iso_string(window.reset_at),
        exclusive_end: iso_string(window.exclusive_end),
        source: window.source.as_str(),
        anchor_id: window.anchor_id.clone(),
        calibration_anchor_id: window.calibration_anchor_id.clone(),
    }
}

fn usage_totals_from_row(row: &WeeklyCycleReportRow) -> WeeklyCycleUsageTotals {
    WeeklyCycleUsageTotals {
        sessions: row.sessions,
        calls: row.calls,
        usage: row.usage.clone(),
        credits: row.credits,
        usd: row.usd,
        priced_calls: row.priced_calls,
        unpriced_calls: row.unpriced_calls,
        unpriced_models: row.unpriced_models.clone(),
    }
}

fn create_weekly_cycle_diagnostics(
    anchors: &[WeeklyCycleAnchorWithDate],
    records: &[UsageRecord],
    rows: &[WeeklyCycleReportRow],
    totals: &WeeklyCycleUsageTotals,
    estimate_before_anchor: bool,
    unanchored: bool,
    usage_diagnostics: Option<UsageDiagnostics>,
) -> WeeklyCycleDiagnostics {
    let ignored_before_anchor_events = anchors.first().map_or(0, |anchor| {
        if estimate_before_anchor {
            0
        } else {
            records
                .iter()
                .filter(|record| record.timestamp < anchor.at_date)
                .count()
        }
    });

    WeeklyCycleDiagnostics {
        anchors: anchors.len(),
        usage_records: records.len(),
        windows: rows.len(),
        derived_windows: rows.iter().filter(|row| row.source == "derived").count(),
        estimated_windows: rows.iter().filter(|row| row.source == "estimated").count(),
        included_usage_events: totals.calls,
        ignored_before_anchor_events,
        estimate_before_anchor,
        unanchored,
        usage_diagnostics,
    }
}

fn format_weekly_cycle_anchor_list(
    report: &AnchorListReport,
    format: StatFormat,
    context: &WeeklyCycleReportContext,
) -> Result<String, AppError> {
    if format == StatFormat::Json {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Json<'a> {
            account_id: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            account_label: Option<&'a str>,
            account_source: &'static str,
            cycle_file: &'a str,
            period_hours: i64,
            anchors: &'a [WeeklyCycleAnchor],
        }
        let value = Json {
            account_id: &report.account_id,
            account_label: context.account_label.as_deref(),
            account_source: report.account_source.as_str(),
            cycle_file: &report.cycle_file,
            period_hours: WEEKLY_CYCLE_PERIOD_HOURS,
            anchors: &report.anchors,
        };
        return Ok(format!(
            "{}\n",
            to_pretty_json(&value).map_err(|error| AppError::new(error.to_string()))?
        ));
    }

    let account_display = context
        .account_label
        .as_deref()
        .unwrap_or(&report.account_id);
    let mut rows = vec![anchor_headers()];
    rows.extend(
        report
            .anchors
            .iter()
            .map(|anchor| anchor_row(anchor, account_display)),
    );

    if format == StatFormat::Csv {
        return Ok(format!("{}\n", format_csv(&rows)));
    }
    if format == StatFormat::Markdown {
        return Ok(format!("{}\n", format_markdown_table(&rows)));
    }

    let mut lines = vec![
        "Codex weekly cycle anchors".to_string(),
        format!("Account: {account_display}"),
        format!("Cycle file: {}", report.cycle_file),
        String::new(),
    ];
    if report.anchors.is_empty() {
        lines.push("No weekly cycle anchors configured.".to_string());
        return Ok(lines.join("\n"));
    }
    lines.push(format_cycle_table(&rows, report.anchors.len()));
    Ok(lines.join("\n"))
}

fn format_weekly_cycle_current(
    report: &WeeklyCycleCurrentReport,
    format: StatFormat,
    context: &WeeklyCycleReportContext,
) -> Result<String, AppError> {
    if format == StatFormat::Json {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Json<'a> {
            #[serde(flatten)]
            context: &'a WeeklyCycleReportContext,
            status: &'static str,
            period_hours: i64,
            now: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            current: Option<&'a WeeklyCycleReportRow>,
            by_day: &'a [WeeklyCycleBreakdownRow],
            by_model: &'a [WeeklyCycleBreakdownRow],
            totals: &'a WeeklyCycleUsageTotals,
            diagnostics: &'a WeeklyCycleDiagnostics,
        }
        let value = Json {
            context,
            status: report.status,
            period_hours: report.period_hours,
            now: iso_string(report.now),
            current: report.current.as_ref(),
            by_day: &report.by_day,
            by_model: &report.by_model,
            totals: &report.totals,
            diagnostics: &report.diagnostics,
        };
        return Ok(format!(
            "{}\n",
            to_pretty_json(&value).map_err(|error| AppError::new(error.to_string()))?
        ));
    }

    let mut rows = vec![current_headers()];
    if let Some(current) = &report.current {
        rows.push(current_row(current, report.status));
    }
    if format == StatFormat::Csv {
        return Ok(format!("{}\n", format_csv(&rows)));
    }
    if format == StatFormat::Markdown {
        return Ok(format!("{}\n", format_markdown_table(&rows)));
    }

    let mut lines = vec![
        "Codex weekly cycle current".to_string(),
        format!("Status: {}", report.status),
        format!(
            "Now: {} ({})",
            format_date_time(report.now),
            iso_string(report.now)
        ),
    ];
    append_context_lines(&mut lines, context);
    lines.push(String::new());

    if report.status == "unanchored" {
        lines.push("No weekly cycle anchors configured.".to_string());
        append_cycle_diagnostics(&mut lines, &report.diagnostics);
        return Ok(lines.join("\n"));
    }
    if report.current.is_none() {
        lines.push("No current weekly cycle could be resolved.".to_string());
        append_cycle_diagnostics(&mut lines, &report.diagnostics);
        return Ok(lines.join("\n"));
    }

    lines.push("Summary:".to_string());
    lines.push(format_cycle_table(&rows, 1));
    append_current_breakdown(&mut lines, "By day:", &report.by_day);
    append_current_breakdown(&mut lines, "By model:", &report.by_model);
    append_unpriced_notes(&mut lines, &report.totals);
    append_cycle_diagnostics(&mut lines, &report.diagnostics);
    Ok(lines.join("\n"))
}

fn format_weekly_cycle_history(
    report: &WeeklyCycleHistoryReport,
    format: StatFormat,
    context: &WeeklyCycleReportContext,
) -> Result<String, AppError> {
    if format == StatFormat::Json {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Json<'a> {
            #[serde(flatten)]
            context: &'a WeeklyCycleReportContext,
            status: &'static str,
            period_hours: i64,
            #[serde(skip_serializing_if = "Option::is_none")]
            start: Option<String>,
            end: String,
            rows: &'a [WeeklyCycleReportRow],
            totals: &'a WeeklyCycleUsageTotals,
            diagnostics: &'a WeeklyCycleDiagnostics,
        }
        let value = Json {
            context,
            status: report.status,
            period_hours: report.period_hours,
            start: report.start.map(iso_string),
            end: iso_string(report.end),
            rows: &report.rows,
            totals: &report.totals,
            diagnostics: &report.diagnostics,
        };
        return Ok(format!(
            "{}\n",
            to_pretty_json(&value).map_err(|error| AppError::new(error.to_string()))?
        ));
    }

    let mut rows = vec![history_headers()];
    rows.extend(report.rows.iter().map(history_row));
    rows.push(history_total_row(&report.totals));
    if format == StatFormat::Csv {
        return Ok(format!("{}\n", format_csv(&rows)));
    }
    if format == StatFormat::Markdown {
        return Ok(format!("{}\n", format_markdown_table(&rows)));
    }

    let mut lines = vec![
        "Codex weekly cycle history".to_string(),
        format!("Status: {}", report.status),
        format!(
            "Range: {} to {}",
            report
                .start
                .map_or_else(|| "beginning".to_string(), format_date_time),
            format_date_time(report.end)
        ),
    ];
    append_context_lines(&mut lines, context);
    lines.push(String::new());
    if report.status == "unanchored" {
        lines.push("No weekly cycle anchors configured.".to_string());
        append_cycle_diagnostics(&mut lines, &report.diagnostics);
        return Ok(lines.join("\n"));
    }
    if report.rows.is_empty() {
        lines.push("No weekly cycle usage found in this range.".to_string());
        append_cycle_diagnostics(&mut lines, &report.diagnostics);
        return Ok(lines.join("\n"));
    }
    lines.push(format_cycle_table(&rows, report.rows.len()));
    append_unpriced_notes(&mut lines, &report.totals);
    append_cycle_diagnostics(&mut lines, &report.diagnostics);
    Ok(lines.join("\n"))
}

fn format_weekly_cycle_detail(
    report: &WeeklyCycleDetailReport,
    format: StatFormat,
    context: &WeeklyCycleReportContext,
) -> Result<String, AppError> {
    if format == StatFormat::Json {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Json<'a> {
            #[serde(flatten)]
            context: &'a WeeklyCycleReportContext,
            status: &'static str,
            cycle_id: &'a str,
            period_hours: i64,
            #[serde(skip_serializing_if = "Option::is_none")]
            history_start: Option<String>,
            history_end: String,
            cycle: &'a WeeklyCycleReportRow,
            by_day: &'a [WeeklyCycleBreakdownRow],
            by_model: &'a [WeeklyCycleBreakdownRow],
            totals: &'a WeeklyCycleUsageTotals,
            diagnostics: &'a WeeklyCycleDiagnostics,
        }
        let value = Json {
            context,
            status: report.status,
            cycle_id: &report.cycle_id,
            period_hours: report.period_hours,
            history_start: report.start.map(iso_string),
            history_end: iso_string(report.end),
            cycle: &report.row,
            by_day: &report.by_day,
            by_model: &report.by_model,
            totals: &report.totals,
            diagnostics: &report.diagnostics,
        };
        return Ok(format!(
            "{}\n",
            to_pretty_json(&value).map_err(|error| AppError::new(error.to_string()))?
        ));
    }

    let rows = vec![detail_headers(), detail_row(&report.row)];
    if format == StatFormat::Csv {
        return Ok(format!("{}\n", format_csv(&rows)));
    }
    if format == StatFormat::Markdown {
        return Ok(format!("{}\n", format_markdown_table(&rows)));
    }

    let mut lines = vec![
        "Codex weekly cycle detail".to_string(),
        format!("Cycle ID: {}", report.cycle_id),
        format!("Status: {}", report.status),
        format!(
            "Cycle: {} to {}",
            format_date_time(parse_iso_timestamp(&report.row.start).expect("row start")),
            format_date_time(parse_iso_timestamp(&report.row.reset_at).expect("row reset"))
        ),
        format!(
            "History range: {} to {}",
            report
                .start
                .map_or_else(|| "beginning".to_string(), format_date_time),
            format_date_time(report.end)
        ),
    ];
    append_context_lines(&mut lines, context);
    lines.push(String::new());
    lines.push("Summary:".to_string());
    lines.push(format_cycle_table(&rows, 1));
    append_current_breakdown(&mut lines, "By day:", &report.by_day);
    append_current_breakdown(&mut lines, "By model:", &report.by_model);
    append_unpriced_notes(&mut lines, &report.totals);
    append_cycle_diagnostics(&mut lines, &report.diagnostics);
    Ok(lines.join("\n"))
}

fn parse_cycle_cli_options(
    args: &[String],
    help: &str,
    mode: CycleParseMode,
) -> Result<(Vec<String>, CycleCliOptions), AppError> {
    let mut options = CycleCliOptions::default();
    let mut positionals = Vec::new();
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "-n" | "--note" if mode == CycleParseMode::Add => {
                options.note = Some(read_string_value(args, &mut index, "--note")?);
            }
            "-A" | "--account-id" => {
                options.account_id = Some(read_string_value(args, &mut index, "--account-id")?);
            }
            "--auth-file" => {
                options.auth_file = Some(read_path_value(args, &mut index, "--auth-file")?);
            }
            "--codex-home" => {
                let path = read_raw_path_value(args, &mut index, "--codex-home")?;
                options.codex_home = Some(path.clone());
                options.stat.codex_home = Some(path);
            }
            "--cycle-file" => {
                options.cycle_file = Some(read_path_value(args, &mut index, "--cycle-file")?);
            }
            "--account-history-file" => {
                let path = read_path_value(args, &mut index, "--account-history-file")?;
                options.account_history_file = Some(path.clone());
                options.stat.account_history_file = Some(path);
            }
            "--sessions-dir"
                if matches!(mode, CycleParseMode::Current | CycleParseMode::History) =>
            {
                let path = read_raw_path_value(args, &mut index, "--sessions-dir")?;
                options.sessions_dir = Some(path.clone());
                options.stat.sessions_dir = Some(path);
            }
            "-f" | "--format" if mode.allows_format() => {
                options.format = Some(read_string_value(args, &mut index, "--format")?);
            }
            "-j" | "--json" if mode.allows_format() => {
                options.json = true;
                options.stat.json = true;
            }
            "-i" | "--select" if mode == CycleParseMode::History => options.select = true,
            "--estimate-before-anchor" if mode == CycleParseMode::History => {
                options.estimate_before_anchor = true;
            }
            "-s" | "--start" if mode == CycleParseMode::History => {
                options.stat.start = Some(read_string_value(args, &mut index, "--start")?);
            }
            "-e" | "--end" if mode == CycleParseMode::History => {
                options.stat.end = Some(read_string_value(args, &mut index, "--end")?);
            }
            "-L" | "--last" if mode == CycleParseMode::History => {
                options.stat.last = Some(read_string_value(args, &mut index, "--last")?);
            }
            "-a" | "--all" if mode == CycleParseMode::History => options.stat.all = true,
            "-t" | "--today" if mode == CycleParseMode::History => options.stat.today = true,
            "--yesterday" if mode == CycleParseMode::History => options.stat.yesterday = true,
            "-m" | "--month" if mode == CycleParseMode::History => options.stat.month = true,
            "-v" | "--verbose" if mode == CycleParseMode::History => options.stat.verbose = true,
            value if value.starts_with("--note=") && mode == CycleParseMode::Add => {
                options.note = Some(value["--note=".len()..].to_string());
            }
            value if value.starts_with("--account-id=") => {
                options.account_id = Some(value["--account-id=".len()..].to_string());
            }
            value if value.starts_with("--auth-file=") => {
                options.auth_file = Some(resolve_cli_path(&value["--auth-file=".len()..])?);
            }
            value if value.starts_with("--codex-home=") => {
                let path = PathBuf::from(&value["--codex-home=".len()..]);
                options.codex_home = Some(path.clone());
                options.stat.codex_home = Some(path);
            }
            value if value.starts_with("--cycle-file=") => {
                options.cycle_file = Some(resolve_cli_path(&value["--cycle-file=".len()..])?);
            }
            value if value.starts_with("--account-history-file=") => {
                let path = resolve_cli_path(&value["--account-history-file=".len()..])?;
                options.account_history_file = Some(path.clone());
                options.stat.account_history_file = Some(path);
            }
            value
                if value.starts_with("--sessions-dir=")
                    && matches!(mode, CycleParseMode::Current | CycleParseMode::History) =>
            {
                let path = PathBuf::from(&value["--sessions-dir=".len()..]);
                options.sessions_dir = Some(path.clone());
                options.stat.sessions_dir = Some(path);
            }
            value if value.starts_with("--format=") && mode.allows_format() => {
                options.format = Some(value["--format=".len()..].to_string());
            }
            value if value.starts_with("--start=") && mode == CycleParseMode::History => {
                options.stat.start = Some(value["--start=".len()..].to_string());
            }
            value if value.starts_with("--end=") && mode == CycleParseMode::History => {
                options.stat.end = Some(value["--end=".len()..].to_string());
            }
            value if value.starts_with("--last=") && mode == CycleParseMode::History => {
                options.stat.last = Some(value["--last=".len()..].to_string());
            }
            unknown if unknown.starts_with('-') => {
                return Err(AppError::invalid_input(format!(
                    "error: Unknown option: {unknown}\n\n{help}"
                )));
            }
            positional => positionals.push(positional.to_string()),
        }
        index += 1;
    }

    if mode == CycleParseMode::Remove && positionals.len() > 1 {
        return Err(AppError::invalid_input(format!(
            "error: Unexpected argument: {}\n\n{help}",
            positionals[1]
        )));
    }

    options.stat.auth_file = options.auth_file.clone();
    options.stat.account_id = options.account_id.clone();
    Ok((positionals, options))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CycleParseMode {
    Add,
    List,
    Remove,
    Current,
    History,
}

impl CycleParseMode {
    fn allows_format(self) -> bool {
        matches!(self, Self::List | Self::Current | Self::History)
    }
}

fn resolve_cycle_format(options: &CycleCliOptions) -> Result<StatFormat, AppError> {
    if options.json {
        return Ok(StatFormat::Json);
    }
    match options.format.as_deref().unwrap_or("table") {
        "table" => Ok(StatFormat::Table),
        "json" => Ok(StatFormat::Json),
        "csv" => Ok(StatFormat::Csv),
        "markdown" => Ok(StatFormat::Markdown),
        _ => Err(AppError::invalid_input(
            "Invalid format value. Expected one of: table, json, csv, markdown.",
        )),
    }
}

fn has_explicit_cycle_history_range(options: &CycleCliOptions) -> bool {
    options.stat.all
        || options.stat.today
        || options.stat.yesterday
        || options.stat.month
        || options.stat.last.is_some()
        || options.stat.start.is_some()
        || options.stat.end.is_some()
}

fn parse_cycle_add_times(parts: &[String]) -> Result<Vec<String>, AppError> {
    let tokens = parts
        .iter()
        .flat_map(|part| part.split(','))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    let mut times = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        let next = tokens.get(index + 1);
        if next.is_some_and(|next| is_date_only_token(token) && is_time_only_token(next)) {
            times.push(format!("{token} {}", next.expect("checked")));
            index += 2;
        } else {
            times.push(token.clone());
            index += 1;
        }
    }
    if times.is_empty() {
        return Err(AppError::new(
            "cycle add requires at least one weekly cycle start time.",
        ));
    }
    Ok(times)
}

fn is_date_only_token(value: &str) -> bool {
    value.len() == 10
        && value.as_bytes().get(4) == Some(&b'-')
        && value.as_bytes().get(7) == Some(&b'-')
}

fn is_time_only_token(value: &str) -> bool {
    let core = value
        .strip_suffix('Z')
        .or_else(|| {
            value.get(..value.len().saturating_sub(6)).filter(|_| {
                value.len() >= 6
                    && matches!(value.as_bytes()[value.len() - 6], b'+' | b'-')
                    && value.as_bytes()[value.len() - 3] == b':'
            })
        })
        .unwrap_or(value);
    let parts = core.split(':').collect::<Vec<_>>();
    (parts.len() == 2 || parts.len() == 3)
        && parts
            .iter()
            .all(|part| part.len() == 2 && part.chars().all(|ch| ch.is_ascii_digit()))
}

struct ParsedWeeklyCycleAnchorTime {
    at: DateTime<Utc>,
    at_iso: String,
    input: String,
    time_zone: String,
}

fn parse_weekly_cycle_anchor_time(input: &str) -> Result<ParsedWeeklyCycleAnchorTime, AppError> {
    let trimmed = input.trim();
    let (date_part, time_part, offset_part) = split_anchor_time(trimmed)?;
    let date = date_part.split('-').collect::<Vec<_>>();
    if date.len() != 3 {
        return Err(invalid_anchor_time(input));
    }
    let year = parse_date_part(date[0], "year", input)? as i32;
    let month = parse_date_part(date[1], "month", input)? as u32;
    let day = parse_date_part(date[2], "day", input)? as u32;
    let (hour, minute, second) = match time_part {
        Some(time) => {
            let parts = time.split(':').collect::<Vec<_>>();
            if parts.len() < 2 || parts.len() > 3 {
                return Err(invalid_anchor_time(input));
            }
            (
                parse_date_part(parts[0], "hour", input)? as u32,
                parse_date_part(parts[1], "minute", input)? as u32,
                match parts.get(2) {
                    Some(value) => parse_date_part(value, "second", input)? as u32,
                    None => 0,
                },
            )
        }
        None => (0, 0, 0),
    };
    let (at, time_zone) = match offset_part {
        Some(offset) => (
            build_offset_date(year, month, day, hour, minute, second, offset, input)?,
            format_offset_time_zone(offset),
        ),
        None => (
            build_local_date(year, month, day, hour, minute, second, input)?,
            local_time_zone(),
        ),
    };

    Ok(ParsedWeeklyCycleAnchorTime {
        at,
        at_iso: iso_string(at),
        input: trimmed.to_string(),
        time_zone,
    })
}

fn split_anchor_time(input: &str) -> Result<(&str, Option<&str>, Option<&str>), AppError> {
    let (date_part, rest) = if let Some((date, time)) = input.split_once('T') {
        (date, Some(time))
    } else if let Some((date, time)) = input.split_once(' ') {
        (date, Some(time))
    } else {
        (input, None)
    };
    let Some(rest) = rest else {
        return Ok((date_part, None, None));
    };
    if let Some(time) = rest.strip_suffix('Z') {
        return Ok((date_part, Some(time), Some("Z")));
    }
    if rest.len() >= 6 {
        let offset_index = rest.len() - 6;
        let offset = &rest[offset_index..];
        if matches!(offset.as_bytes()[0], b'+' | b'-') && offset.as_bytes()[3] == b':' {
            return Ok((date_part, Some(&rest[..offset_index]), Some(offset)));
        }
    }
    Ok((date_part, Some(rest), None))
}

fn invalid_anchor_time(input: &str) -> AppError {
    AppError::new(format!(
        "Invalid weekly cycle anchor time: {input}. Expected YYYY-MM-DD, YYYY-MM-DD HH:mm, or an ISO time with offset."
    ))
}

fn parse_date_part(value: &str, label: &str, input: &str) -> Result<i64, AppError> {
    value.parse::<i64>().map_err(|_| {
        AppError::new(format!(
            "Invalid {label} in weekly cycle anchor time: {input}."
        ))
    })
}

fn build_local_date(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    input: &str,
) -> Result<DateTime<Utc>, AppError> {
    let naive = chrono::NaiveDate::from_ymd_opt(year, month, day)
        .and_then(|date| date.and_hms_opt(hour, minute, second))
        .ok_or_else(|| {
            AppError::new(format!("Invalid local weekly cycle anchor time: {input}."))
        })?;
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(value) => Ok(value.with_timezone(&Utc)),
        LocalResult::Ambiguous(earliest, _) => Ok(earliest.with_timezone(&Utc)),
        LocalResult::None => Err(AppError::new(format!(
            "Invalid local weekly cycle anchor time: {input}."
        ))),
    }
}

fn build_offset_date(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    offset: &str,
    input: &str,
) -> Result<DateTime<Utc>, AppError> {
    let offset_minutes = parse_offset_minutes(offset)?;
    let offset = FixedOffset::east_opt(offset_minutes * 60)
        .ok_or_else(|| AppError::new(format!("Invalid timezone offset: {offset}.")))?;
    offset
        .with_ymd_and_hms(year, month, day, hour, minute, second)
        .single()
        .map(|date| date.with_timezone(&Utc))
        .ok_or_else(|| AppError::new(format!("Invalid offset weekly cycle anchor time: {input}.")))
}

fn parse_offset_minutes(offset: &str) -> Result<i32, AppError> {
    if offset == "Z" {
        return Ok(0);
    }
    if offset.len() != 6 || offset.as_bytes()[3] != b':' {
        return Err(AppError::new(format!("Invalid timezone offset: {offset}.")));
    }
    let sign = if offset.starts_with('-') { -1 } else { 1 };
    let hour = offset[1..3]
        .parse::<i32>()
        .map_err(|_| AppError::new(format!("Invalid timezone offset: {offset}.")))?;
    let minute = offset[4..6]
        .parse::<i32>()
        .map_err(|_| AppError::new(format!("Invalid timezone offset: {offset}.")))?;
    if hour > 23 || minute > 59 {
        return Err(AppError::new(format!("Invalid timezone offset: {offset}.")));
    }
    Ok(sign * (hour * 60 + minute))
}

fn format_offset_time_zone(offset: &str) -> String {
    if offset == "Z" {
        "UTC".to_string()
    } else {
        format!("UTC{offset}")
    }
}

fn local_time_zone() -> String {
    std::env::var("TZ")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            let offset = Local::now().offset().fix().local_minus_utc();
            if offset == 8 * 60 * 60 {
                "Asia/Shanghai".to_string()
            } else {
                "local".to_string()
            }
        })
}

fn anchor_headers() -> Vec<String> {
    [
        "Account",
        "ID",
        "Local time",
        "UTC at",
        "Source",
        "Note",
        "Created at",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn anchor_row(anchor: &WeeklyCycleAnchor, account_id: &str) -> Vec<String> {
    vec![
        account_id.to_string(),
        anchor.id.clone(),
        format_date_time(parse_iso_timestamp(&anchor.at).expect("anchor at")),
        anchor.at.clone(),
        anchor.source.clone(),
        anchor.note.clone(),
        anchor.created_at.clone(),
    ]
}

fn current_headers() -> Vec<String> {
    [
        "Status", "Start", "Reset at", "Source", "Sessions", "Calls", "Total", "Credits", "USD",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn current_row(row: &WeeklyCycleReportRow, status: &str) -> Vec<String> {
    vec![
        status.to_string(),
        format_date_time(parse_iso_timestamp(&row.start).expect("row start")),
        format_date_time(parse_iso_timestamp(&row.reset_at).expect("row reset")),
        row.source.to_string(),
        format_integer(row.sessions as i64),
        format_integer(row.calls),
        format_integer(row.usage.total_tokens),
        format_cycle_credits(row.credits),
        format_cycle_usd(row.usd),
    ]
}

fn current_breakdown_headers() -> Vec<String> {
    [
        "Group",
        "Sessions",
        "Calls",
        "Input",
        "Cached",
        "Output",
        "Reasoning",
        "Total",
        "Credits",
        "USD",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn current_breakdown_row(row: &WeeklyCycleBreakdownRow) -> Vec<String> {
    vec![
        row.key.clone(),
        format_integer(row.sessions as i64),
        format_integer(row.calls),
        format_integer(row.usage.input_tokens),
        format_integer(row.usage.cached_input_tokens),
        format_integer(row.usage.output_tokens),
        format_integer(row.usage.reasoning_output_tokens),
        format_integer(row.usage.total_tokens),
        format_cycle_credits(row.credits),
        format_cycle_usd(row.usd),
    ]
}

fn history_headers() -> Vec<String> {
    [
        "ID",
        "Start",
        "Reset at",
        "Source",
        "Sessions",
        "Calls",
        "Input",
        "Cached",
        "Output",
        "Reasoning",
        "Total",
        "Credits",
        "USD",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn history_row(row: &WeeklyCycleReportRow) -> Vec<String> {
    vec![
        row.id.clone(),
        format_date_time(parse_iso_timestamp(&row.start).expect("row start")),
        format_date_time(parse_iso_timestamp(&row.reset_at).expect("row reset")),
        row.source.to_string(),
        format_integer(row.sessions as i64),
        format_integer(row.calls),
        format_integer(row.usage.input_tokens),
        format_integer(row.usage.cached_input_tokens),
        format_integer(row.usage.output_tokens),
        format_integer(row.usage.reasoning_output_tokens),
        format_integer(row.usage.total_tokens),
        format_cycle_credits(row.credits),
        format_cycle_usd(row.usd),
    ]
}

fn history_total_row(totals: &WeeklyCycleUsageTotals) -> Vec<String> {
    vec![
        "Total".to_string(),
        String::new(),
        String::new(),
        String::new(),
        format_integer(totals.sessions as i64),
        format_integer(totals.calls),
        format_integer(totals.usage.input_tokens),
        format_integer(totals.usage.cached_input_tokens),
        format_integer(totals.usage.output_tokens),
        format_integer(totals.usage.reasoning_output_tokens),
        format_integer(totals.usage.total_tokens),
        format_cycle_credits(totals.credits),
        format_cycle_usd(totals.usd),
    ]
}

fn detail_headers() -> Vec<String> {
    [
        "ID", "Start", "Reset at", "Source", "Sessions", "Calls", "Total", "Credits", "USD",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn detail_row(row: &WeeklyCycleReportRow) -> Vec<String> {
    vec![
        row.id.clone(),
        format_date_time(parse_iso_timestamp(&row.start).expect("row start")),
        format_date_time(parse_iso_timestamp(&row.reset_at).expect("row reset")),
        row.source.to_string(),
        format_integer(row.sessions as i64),
        format_integer(row.calls),
        format_integer(row.usage.total_tokens),
        format_cycle_credits(row.credits),
        format_cycle_usd(row.usd),
    ]
}

fn append_context_lines(lines: &mut Vec<String>, context: &WeeklyCycleReportContext) {
    if let Some(account_id) = &context.account_id {
        lines.push(format!(
            "Account: {}",
            context.account_label.as_deref().unwrap_or(account_id)
        ));
    }
    if let Some(cycle_file) = &context.cycle_file {
        lines.push(format!("Cycle file: {cycle_file}"));
    }
}

fn append_current_breakdown(
    lines: &mut Vec<String>,
    title: &str,
    rows: &[WeeklyCycleBreakdownRow],
) {
    lines.push(String::new());
    lines.push(title.to_string());
    if rows.is_empty() {
        lines.push("No usage events in this cycle.".to_string());
        return;
    }
    let mut table_rows = vec![current_breakdown_headers()];
    table_rows.extend(rows.iter().map(current_breakdown_row));
    lines.push(format_cycle_table(&table_rows, rows.len()));
}

fn append_cycle_diagnostics(lines: &mut Vec<String>, diagnostics: &WeeklyCycleDiagnostics) {
    lines.push(String::new());
    lines.push("Diagnostics:".to_string());
    lines.push(format!(
        "  Anchors: {}",
        format_integer(diagnostics.anchors as i64)
    ));
    lines.push(format!(
        "  Windows: {}",
        format_integer(diagnostics.windows as i64)
    ));
    lines.push(format!(
        "  Derived windows: {}",
        format_integer(diagnostics.derived_windows as i64)
    ));
    lines.push(format!(
        "  Estimated windows: {}",
        format_integer(diagnostics.estimated_windows as i64)
    ));
    lines.push(format!(
        "  Usage records: {}",
        format_integer(diagnostics.usage_records as i64)
    ));
    lines.push(format!(
        "  Usage events included: {}",
        format_integer(diagnostics.included_usage_events)
    ));
    lines.push(format!(
        "  Ignored before anchor: {}",
        format_integer(diagnostics.ignored_before_anchor_events as i64)
    ));
}

fn append_unpriced_notes(lines: &mut Vec<String>, totals: &WeeklyCycleUsageTotals) {
    if totals.unpriced_calls == 0 {
        return;
    }
    lines.push(String::new());
    lines.push(format!(
        "Note: {} usage events had no credit price and are excluded from Credits.",
        format_integer(totals.unpriced_calls)
    ));
    lines.push("Unpriced models:".to_string());
    for row in &totals.unpriced_models {
        lines.push(format!(
            "  {}: {} calls, {} tokens",
            row.model,
            format_integer(row.calls),
            format_integer(row.total_tokens)
        ));
    }
}

fn format_cycle_table(rows: &[Vec<String>], body_rows: usize) -> String {
    let widths = column_widths(rows);
    let Some((header, body)) = rows.split_first() else {
        return String::new();
    };
    let mut lines = vec![
        format_cycle_table_row(header, &widths),
        format_cycle_table_separator(&widths),
    ];
    for (index, row) in body.iter().enumerate() {
        if index == body_rows {
            lines.push(format_cycle_table_separator(&widths));
        }
        lines.push(format_cycle_table_row(row, &widths));
    }
    lines.join("\n")
}

fn format_cycle_table_row(row: &[String], widths: &[usize]) -> String {
    row.iter()
        .enumerate()
        .map(|(index, cell)| {
            format!(
                "{cell:<width$}",
                width = widths.get(index).copied().unwrap_or(0)
            )
        })
        .collect::<Vec<_>>()
        .join("  ")
}

fn format_cycle_table_separator(widths: &[usize]) -> String {
    widths
        .iter()
        .map(|width| "-".repeat(*width))
        .collect::<Vec<_>>()
        .join("  ")
}

fn column_widths(rows: &[Vec<String>]) -> Vec<usize> {
    let width_count = rows.iter().map(Vec::len).max().unwrap_or(0);
    (0..width_count)
        .map(|index| {
            rows.iter()
                .map(|row| row.get(index).map(String::len).unwrap_or(0))
                .max()
                .unwrap_or(0)
        })
        .collect()
}

fn sort_anchors_with_dates(anchors: &[WeeklyCycleAnchor]) -> Vec<WeeklyCycleAnchorWithDate> {
    let mut output = anchors
        .iter()
        .filter_map(|anchor| {
            parse_iso_timestamp(&anchor.at).map(|at_date| WeeklyCycleAnchorWithDate {
                anchor: anchor.clone(),
                at_date,
            })
        })
        .collect::<Vec<_>>();
    output.sort_by(|left, right| {
        left.at_date
            .cmp(&right.at_date)
            .then_with(|| left.anchor.id.cmp(&right.anchor.id))
    });
    output
}

fn sort_usage_records(records: &mut [UsageRecord]) {
    records.sort_by(|left, right| {
        left.timestamp
            .cmp(&right.timestamp)
            .then_with(|| left.session_id.cmp(&right.session_id))
            .then_with(|| left.file_path.cmp(&right.file_path))
    });
}

fn compare_windows(
    left: &InternalWeeklyCycleWindow,
    right: &InternalWeeklyCycleWindow,
) -> Ordering {
    left.start
        .cmp(&right.start)
        .then_with(|| source_sort_key(left.source).cmp(&source_sort_key(right.source)))
        .then_with(|| {
            left.anchor_id
                .as_deref()
                .unwrap_or("")
                .cmp(right.anchor_id.as_deref().unwrap_or(""))
        })
}

fn source_sort_key(source: WeeklyCycleWindowSource) -> u8 {
    match source {
        WeeklyCycleWindowSource::Estimated => 0,
        WeeklyCycleWindowSource::Manual => 1,
        WeeklyCycleWindowSource::Derived => 2,
    }
}

fn window_overlaps_range(
    window: &InternalWeeklyCycleWindow,
    range_start: Option<DateTime<Utc>>,
    range_end: DateTime<Utc>,
) -> bool {
    window.start <= range_end && range_start.is_none_or(|start| window.exclusive_end > start)
}

fn add_unpriced_model(
    unpriced_models: &mut HashMap<String, WeeklyCycleUnpricedModelRow>,
    record: &UsageRecord,
) {
    let pricing_key = normalize_model_name(&record.model);
    let row = unpriced_models
        .entry(pricing_key.clone())
        .or_insert_with(|| WeeklyCycleUnpricedModelRow {
            model: record.model.clone(),
            pricing_key,
            calls: 0,
            total_tokens: 0,
            pricing_stub: format_pricing_stub(&record.model),
        });
    row.calls += 1;
    row.total_tokens += record.usage.total_tokens;
}

fn format_unpriced_models(
    unpriced_models: HashMap<String, WeeklyCycleUnpricedModelRow>,
) -> Vec<WeeklyCycleUnpricedModelRow> {
    let mut rows = unpriced_models.into_values().collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        right
            .calls
            .cmp(&left.calls)
            .then_with(|| right.total_tokens.cmp(&left.total_tokens))
            .then_with(|| left.pricing_key.cmp(&right.pricing_key))
    });
    rows
}

fn format_pricing_stub(model: &str) -> String {
    let key = normalize_model_name(model);
    format!(
        "\"{key}\": {{\n  label: \"{}\",\n  inputCreditsPerMillion: 0,\n  cachedInputCreditsPerMillion: 0,\n  outputCreditsPerMillion: 0\n}}",
        model.replace('\\', "\\\\").replace('"', "\\\"")
    )
}

fn empty_weekly_cycle_totals() -> WeeklyCycleUsageTotals {
    WeeklyCycleUsageTotals {
        sessions: 0,
        calls: 0,
        usage: TokenUsage::default(),
        credits: 0.0,
        usd: 0.0,
        priced_calls: 0,
        unpriced_calls: 0,
        unpriced_models: Vec::new(),
    }
}

fn weekly_cycle_window_id(window: &InternalWeeklyCycleWindow) -> String {
    if window.source == WeeklyCycleWindowSource::Manual {
        if let Some(anchor_id) = &window.anchor_id {
            return anchor_id.clone();
        }
    }
    let prefix = if window.source == WeeklyCycleWindowSource::Estimated {
        "est"
    } else {
        "cyc"
    };
    format!("{prefix}_{}", compact_iso_timestamp(window.start))
}

fn weekly_cycle_anchor_id(date: DateTime<Utc>) -> String {
    format!("anc_{}", compact_iso_timestamp(date))
}

fn compact_iso_timestamp(date: DateTime<Utc>) -> String {
    iso_string(date).replace(['-', ':'], "").replace('.', "")
}

fn earliest_anchor_date(anchors: &[WeeklyCycleAnchor]) -> Option<DateTime<Utc>> {
    sort_anchors_with_dates(anchors)
        .first()
        .map(|anchor| anchor.at_date)
}

fn sort_weekly_cycle_anchors(anchors: &mut [WeeklyCycleAnchor]) {
    anchors.sort_by(|left, right| left.at.cmp(&right.at).then_with(|| left.id.cmp(&right.id)));
}

fn normalize_required_id(value: &str, label: &str) -> Result<String, AppError> {
    let normalized = value.trim();
    if normalized.is_empty() {
        Err(AppError::new(format!(
            "Weekly cycle {label} cannot be empty."
        )))
    } else {
        Ok(normalized.to_string())
    }
}

fn normalize_optional_id(value: Option<&str>) -> Option<String> {
    let normalized = value?.trim();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized.to_string())
    }
}

fn resolve_cycle_file(options: &CycleCliOptions) -> PathBuf {
    resolve_storage_paths(&StorageOptions {
        codex_home: options.codex_home.clone(),
        cycle_file: options.cycle_file.clone(),
        ..StorageOptions::default()
    })
    .cycle_file
}

fn resolve_account_history_file(options: &CycleCliOptions) -> PathBuf {
    resolve_storage_paths(&StorageOptions {
        codex_home: options.codex_home.clone(),
        account_history_file: options.account_history_file.clone(),
        ..StorageOptions::default()
    })
    .account_history_file
}

fn auth_options(options: &CycleCliOptions) -> AuthCommandOptions {
    AuthCommandOptions {
        auth_file: options.auth_file.clone(),
        codex_home: options.codex_home.clone(),
        store_dir: None,
        account_history_file: options.account_history_file.clone(),
    }
}

fn read_string_value(args: &[String], index: &mut usize, name: &str) -> Result<String, AppError> {
    *index += 1;
    args.get(*index)
        .cloned()
        .filter(|value| !value.starts_with("--"))
        .ok_or_else(|| AppError::invalid_input(format!("error: Missing value for {name}")))
}

fn read_path_value(args: &[String], index: &mut usize, name: &str) -> Result<PathBuf, AppError> {
    resolve_cli_path(&read_string_value(args, index, name)?)
}

fn read_raw_path_value(
    args: &[String],
    index: &mut usize,
    name: &str,
) -> Result<PathBuf, AppError> {
    Ok(PathBuf::from(read_string_value(args, index, name)?))
}

fn resolve_cli_path(value: &str) -> Result<PathBuf, AppError> {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        Ok(path)
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .map_err(|error| AppError::new(error.to_string()))
    }
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn assert_iso_timestamp(value: &str, path: &str) -> Result<(), AppError> {
    let date = parse_iso_timestamp(value)
        .ok_or_else(|| AppError::new(format!("Expected {path} to be a UTC ISO timestamp.")))?;
    if iso_string(date) != value {
        return Err(AppError::new(format!(
            "Expected {path} to be a UTC ISO timestamp."
        )));
    }
    Ok(())
}

fn parse_iso_timestamp(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.with_timezone(&Utc))
}

fn iso_string(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn format_date_time(date: DateTime<Utc>) -> String {
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

fn local_date_key(date: DateTime<Utc>) -> String {
    let local = date.with_timezone(&Local);
    format!("{}-{:02}-{:02}", local.year(), local.month(), local.day())
}

fn round_credits(value: f64) -> f64 {
    ((value + f64::EPSILON) * 1_000_000.0).round() / 1_000_000.0
}

fn credits_to_usd(credits: f64) -> f64 {
    (((credits / 25.0) + f64::EPSILON) * 1_000_000.0).round() / 1_000_000.0
}

fn format_cycle_credits(value: f64) -> String {
    format!("{value:.6}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

fn format_cycle_usd(value: f64) -> String {
    format!("${}", format_cycle_credits(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multiple_cycle_add_times() {
        assert_eq!(
            parse_cycle_add_times(&[
                "2026-05-17".to_string(),
                "09:00".to_string(),
                "2026-05-18T00:00:00Z,2026-05-19".to_string()
            ])
            .expect("times"),
            vec!["2026-05-17 09:00", "2026-05-18T00:00:00Z", "2026-05-19"]
        );
    }

    #[test]
    fn derives_delayed_weekly_cycles() {
        let anchors = vec![WeeklyCycleAnchor {
            id: "anchor-may-01".to_string(),
            at: "2026-05-01T00:00:00.000Z".to_string(),
            input: "2026-05-01T00:00:00Z".to_string(),
            time_zone: "UTC".to_string(),
            source: "manual".to_string(),
            note: String::new(),
            created_at: "2026-05-01T00:00:00.000Z".to_string(),
        }];
        let records = vec![
            record("2026-05-01T01:00:00.000Z", "session-a", 100),
            record("2026-05-07T23:59:59.000Z", "session-a", 20),
            record("2026-05-09T08:00:00.000Z", "session-b", 50),
        ];
        let report = build_weekly_cycle_history_report(
            &anchors,
            records,
            None,
            parse_iso_timestamp("2026-05-10T00:00:00.000Z").expect("now"),
            false,
            None,
        );

        assert_eq!(report.status, "ok");
        assert_eq!(
            report.rows.iter().map(|row| row.source).collect::<Vec<_>>(),
            vec!["manual", "derived"]
        );
        assert_eq!(
            report
                .rows
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            vec!["anchor-may-01", "cyc_20260509T080000000Z"]
        );
        assert_eq!(report.rows[0].calls, 2);
        assert_eq!(report.rows[1].usage.total_tokens, 50);
        assert_eq!(report.totals.calls, 3);
    }

    fn record(timestamp: &str, session_id: &str, total_tokens: i64) -> UsageRecord {
        UsageRecord {
            timestamp: parse_iso_timestamp(timestamp).expect("timestamp"),
            session_id: session_id.to_string(),
            model: "gpt-5.5".to_string(),
            reasoning_effort: None,
            cwd: "/repo".to_string(),
            account_id: None,
            file_path: "/tmp/session.jsonl".to_string(),
            usage: TokenUsage {
                input_tokens: total_tokens,
                cached_input_tokens: 0,
                output_tokens: 0,
                reasoning_output_tokens: 0,
                total_tokens,
            },
        }
    }
}
