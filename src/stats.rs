use crate::auth::{read_codex_auth_status, AuthCommandOptions};
use crate::error::AppError;
use crate::format::{
    credits_to_usd, format_credits, format_csv, format_integer, format_markdown_table,
    format_plain_table, format_usd, round_credits, to_pretty_json,
};
use crate::pricing::{
    calculate_credit_cost, normalize_model_name, TokenUsage as PricingTokenUsage,
};
use crate::storage::{resolve_storage_paths, write_sensitive_file, StorageOptions};
use chrono::{
    DateTime, Datelike, Duration, FixedOffset, Local, LocalResult, Offset, SecondsFormat, TimeZone,
    Timelike, Utc,
};
use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::thread;

const DEFAULT_FILE_READ_CONCURRENCY: i64 = 8;
const DEFAULT_MAX_FILE_SCAN_THREADS: usize = 8;
const FILE_SCAN_WORKER_MIN_FILES: usize = 64;
const SESSION_READ_BUFFER_SIZE: usize = 256 * 1024;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;
const BALANCED_SCAN_MIN_LOOKBACK_MS: i64 = 2 * DAY_MS;
const BALANCED_SCAN_MAX_LOOKBACK_MS: i64 = 7 * DAY_MS;
const DEFAULT_SESSION_DETAIL_COMPACT_ROWS: usize = 20;
const AUTH_ACCOUNT_HISTORY_STORE_VERSION: u8 = 1;
const FULL_SCAN_ACCURACY_NOTE: &str =
    "Note: This report used balanced scanning, not a full scan. It reads in-range files and checks a bounded lookback by last token_count timestamp. Use -F, --full-scan to check all pre-range rollout files for exact local token_count results.";

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum StatFormat {
    Table,
    Json,
    Csv,
    Markdown,
}

impl StatFormat {
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

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum StatGroupBy {
    Hour,
    Day,
    Week,
    Month,
    Model,
    Cwd,
    Account,
}

impl StatGroupBy {
    fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "hour" => Ok(Self::Hour),
            "day" => Ok(Self::Day),
            "week" => Ok(Self::Week),
            "month" => Ok(Self::Month),
            "model" => Ok(Self::Model),
            "cwd" => Ok(Self::Cwd),
            "account" => Ok(Self::Account),
            _ => Err(AppError::invalid_input(
                "Invalid group-by value. Expected one of: hour, day, week, month, model, cwd, account.",
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Hour => "hour",
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
            Self::Model => "model",
            Self::Cwd => "cwd",
            Self::Account => "account",
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum StatSort {
    Time,
    Tokens,
    Credits,
    Calls,
    Sessions,
}

impl StatSort {
    fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "time" => Ok(Self::Time),
            "tokens" => Ok(Self::Tokens),
            "credits" => Ok(Self::Credits),
            "calls" => Ok(Self::Calls),
            "sessions" => Ok(Self::Sessions),
            _ => Err(AppError::invalid_input(
                "Invalid sort value. Expected one of: time, tokens, credits, calls, sessions.",
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Time => "time",
            Self::Tokens => "tokens",
            Self::Credits => "credits",
            Self::Calls => "calls",
            Self::Sessions => "sessions",
        }
    }
}

#[derive(Debug, Clone, Default)]
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
struct ResolvedStatOptions {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    group_by: StatGroupBy,
    format: StatFormat,
    sessions_dir: PathBuf,
    sort_by: Option<StatSort>,
    limit: Option<usize>,
    include_reasoning_effort: bool,
    scan_all_files: bool,
    verbose: bool,
    account_id: Option<String>,
    account_history: Option<UsageAccountHistory>,
}

#[derive(Debug, Clone)]
pub struct ResolvedStatRangeOptions {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub format: StatFormat,
    pub sessions_dir: PathBuf,
    pub verbose: bool,
}

#[derive(Debug, Clone)]
pub struct UsageRecordsReadOptions {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub sessions_dir: PathBuf,
    pub scan_all_files: bool,
    pub account_history_file: Option<PathBuf>,
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

#[derive(Debug, Clone, Copy)]
struct DateRange {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
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
    fn add(&mut self, other: &TokenUsage) {
        self.input_tokens += other.input_tokens;
        self.cached_input_tokens += other.cached_input_tokens;
        self.output_tokens += other.output_tokens;
        self.reasoning_output_tokens += other.reasoning_output_tokens;
        self.total_tokens += other.total_tokens;
    }

    fn is_empty(&self) -> bool {
        self.input_tokens == 0
            && self.cached_input_tokens == 0
            && self.output_tokens == 0
            && self.reasoning_output_tokens == 0
            && self.total_tokens == 0
    }

    fn pricing_usage(&self) -> PricingTokenUsage {
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
struct UsageRecordView<'a> {
    timestamp: DateTime<Utc>,
    session_id: &'a str,
    model: &'a str,
    reasoning_effort: Option<&'a str>,
    cwd: &'a str,
    account_id: Option<&'a str>,
    file_path: &'a str,
    usage: &'a TokenUsage,
}

impl UsageRecordView<'_> {
    fn to_owned_record(self) -> UsageRecord {
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

fn parse_usage_json_event(line: &str) -> Result<Option<UsageJsonEvent<'_>>, serde_json::Error> {
    match serde_json::from_str::<UsageJsonEvent>(line) {
        Ok(event) => Ok(Some(event)),
        Err(error) => {
            if serde_json::from_str::<IgnoredAny>(line).is_ok() {
                Ok(None)
            } else {
                Err(error)
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum JsonObject<T> {
    Object(T),
    Other(IgnoredAny),
}

impl<T> JsonObject<T> {
    fn as_object(&self) -> Option<&T> {
        match self {
            Self::Object(value) => Some(value),
            Self::Other(_) => None,
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum JsonString<'a> {
    String(#[serde(borrow)] Cow<'a, str>),
    Other(IgnoredAny),
}

impl JsonString<'_> {
    fn as_non_empty_str(&self) -> Option<&str> {
        match self {
            Self::String(value) if !value.trim().is_empty() => Some(value.as_ref()),
            Self::String(_) | Self::Other(_) => None,
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum JsonI64 {
    I64(i64),
    U64(u64),
    F64(f64),
    Other(IgnoredAny),
}

impl JsonI64 {
    fn to_i64(&self) -> Option<i64> {
        match self {
            Self::I64(value) => Some(*value),
            Self::U64(value) => Some(*value as i64),
            Self::F64(value) => Some(*value as i64),
            Self::Other(_) => None,
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum JsonDate<'a> {
    String(#[serde(borrow)] Cow<'a, str>),
    I64(i64),
    U64(u64),
    F64(f64),
    Other(IgnoredAny),
}

impl JsonDate<'_> {
    fn to_utc(&self) -> Option<DateTime<Utc>> {
        match self {
            Self::String(value) => DateTime::parse_from_rfc3339(value.as_ref())
                .ok()
                .map(|date| date.with_timezone(&Utc)),
            Self::I64(value) => Utc.timestamp_millis_opt(*value).single(),
            Self::U64(value) => Utc.timestamp_millis_opt(*value as i64).single(),
            Self::F64(value) => Utc.timestamp_millis_opt(*value as i64).single(),
            Self::Other(_) => None,
        }
    }
}

#[derive(Deserialize)]
struct UsageJsonEvent<'a> {
    #[serde(rename = "type", default, borrow)]
    event_type: Option<JsonString<'a>>,
    #[serde(default, borrow)]
    timestamp: Option<JsonDate<'a>>,
    #[serde(default, borrow)]
    payload: Option<JsonObject<UsageJsonPayload<'a>>>,
}

impl<'a> UsageJsonEvent<'a> {
    fn event_type(&self) -> Option<&str> {
        self.event_type
            .as_ref()
            .and_then(JsonString::as_non_empty_str)
    }

    fn timestamp(&self) -> Option<DateTime<Utc>> {
        self.timestamp.as_ref().and_then(JsonDate::to_utc)
    }

    fn payload(&self) -> Option<&UsageJsonPayload<'a>> {
        self.payload.as_ref().and_then(JsonObject::as_object)
    }
}

#[derive(Deserialize)]
struct UsageJsonPayload<'a> {
    #[serde(rename = "type", default, borrow)]
    payload_type: Option<JsonString<'a>>,
    #[serde(default, borrow)]
    id: Option<JsonString<'a>>,
    #[serde(default, borrow)]
    model: Option<JsonString<'a>>,
    #[serde(default, borrow)]
    cwd: Option<JsonString<'a>>,
    #[serde(default, alias = "reasoningEffort", borrow)]
    reasoning_effort: Option<JsonString<'a>>,
    #[serde(default, alias = "modelReasoningEffort", borrow)]
    model_reasoning_effort: Option<JsonString<'a>>,
    #[serde(default, alias = "modelConfig", borrow)]
    model_config: Option<JsonObject<ReasoningJsonFields<'a>>>,
    #[serde(default, borrow)]
    reasoning: Option<JsonObject<ReasoningJsonFields<'a>>>,
    #[serde(default, alias = "collaborationMode", borrow)]
    collaboration_mode: Option<JsonObject<CollaborationModeJson<'a>>>,
    #[serde(default)]
    info: Option<JsonObject<TokenCountInfoJson>>,
}

impl<'a> UsageJsonPayload<'a> {
    fn payload_type(&self) -> Option<&str> {
        self.payload_type
            .as_ref()
            .and_then(JsonString::as_non_empty_str)
    }

    fn id(&self) -> Option<&str> {
        self.id.as_ref().and_then(JsonString::as_non_empty_str)
    }

    fn model(&self) -> Option<&str> {
        self.model.as_ref().and_then(JsonString::as_non_empty_str)
    }

    fn cwd(&self) -> Option<&str> {
        self.cwd.as_ref().and_then(JsonString::as_non_empty_str)
    }

    fn info(&self) -> Option<&TokenCountInfoJson> {
        self.info.as_ref().and_then(JsonObject::as_object)
    }

    fn reasoning_effort(&self) -> Option<&str> {
        self.reasoning_effort
            .as_ref()
            .and_then(JsonString::as_non_empty_str)
            .or_else(|| {
                self.model_reasoning_effort
                    .as_ref()
                    .and_then(JsonString::as_non_empty_str)
            })
            .or_else(|| {
                self.model_config
                    .as_ref()
                    .and_then(JsonObject::as_object)
                    .and_then(ReasoningJsonFields::reasoning_effort)
            })
            .or_else(|| {
                self.reasoning
                    .as_ref()
                    .and_then(JsonObject::as_object)
                    .and_then(ReasoningJsonFields::reasoning_effort)
            })
            .or_else(|| {
                self.collaboration_mode
                    .as_ref()
                    .and_then(JsonObject::as_object)
                    .and_then(CollaborationModeJson::reasoning_effort)
            })
    }
}

#[derive(Deserialize)]
struct CollaborationModeJson<'a> {
    #[serde(default, borrow)]
    settings: Option<JsonObject<ReasoningJsonFields<'a>>>,
}

impl CollaborationModeJson<'_> {
    fn reasoning_effort(&self) -> Option<&str> {
        self.settings
            .as_ref()
            .and_then(JsonObject::as_object)
            .and_then(ReasoningJsonFields::reasoning_effort)
    }
}

#[derive(Deserialize)]
struct ReasoningJsonFields<'a> {
    #[serde(default, borrow)]
    effort: Option<JsonString<'a>>,
    #[serde(default, alias = "reasoningEffort", borrow)]
    reasoning_effort: Option<JsonString<'a>>,
}

impl ReasoningJsonFields<'_> {
    fn reasoning_effort(&self) -> Option<&str> {
        self.effort
            .as_ref()
            .and_then(JsonString::as_non_empty_str)
            .or_else(|| {
                self.reasoning_effort
                    .as_ref()
                    .and_then(JsonString::as_non_empty_str)
            })
    }
}

#[derive(Deserialize)]
struct TokenCountInfoJson {
    #[serde(default, alias = "totalTokenUsage")]
    total_token_usage: Option<JsonObject<TokenUsageJson>>,
    #[serde(default, alias = "lastTokenUsage")]
    last_token_usage: Option<JsonObject<TokenUsageJson>>,
}

impl TokenCountInfoJson {
    fn total_token_usage(&self) -> Option<TokenUsage> {
        self.total_token_usage
            .as_ref()
            .and_then(JsonObject::as_object)
            .and_then(TokenUsageJson::to_token_usage)
    }

    fn last_token_usage(&self) -> Option<TokenUsage> {
        self.last_token_usage
            .as_ref()
            .and_then(JsonObject::as_object)
            .and_then(TokenUsageJson::to_token_usage)
    }
}

#[derive(Deserialize)]
struct TokenUsageJson {
    #[serde(default, alias = "inputTokens")]
    input_tokens: Option<JsonI64>,
    #[serde(default, alias = "cachedInputTokens")]
    cached_input_tokens: Option<JsonI64>,
    #[serde(default, alias = "outputTokens")]
    output_tokens: Option<JsonI64>,
    #[serde(default, alias = "reasoningOutputTokens")]
    reasoning_output_tokens: Option<JsonI64>,
    #[serde(default, alias = "totalTokens")]
    total_tokens: Option<JsonI64>,
}

impl TokenUsageJson {
    fn to_token_usage(&self) -> Option<TokenUsage> {
        let input = self.input_tokens.as_ref().and_then(JsonI64::to_i64);
        let output = self.output_tokens.as_ref().and_then(JsonI64::to_i64);
        let total = self.total_tokens.as_ref().and_then(JsonI64::to_i64);

        if input.is_none() && output.is_none() && total.is_none() {
            return None;
        }

        Some(TokenUsage {
            input_tokens: input.unwrap_or(0),
            cached_input_tokens: self
                .cached_input_tokens
                .as_ref()
                .and_then(JsonI64::to_i64)
                .unwrap_or(0),
            output_tokens: output.unwrap_or(0),
            reasoning_output_tokens: self
                .reasoning_output_tokens
                .as_ref()
                .and_then(JsonI64::to_i64)
                .unwrap_or(0),
            total_tokens: total.unwrap_or_else(|| input.unwrap_or(0) + output.unwrap_or(0)),
        })
    }
}

#[derive(Default)]
struct MutableStatRow {
    sessions: HashSet<String>,
    calls: i64,
    usage: TokenUsage,
    credits: f64,
    priced_calls: i64,
    unpriced_calls: i64,
}

#[derive(Default)]
struct MutableSession {
    session_id: String,
    model: String,
    cwd: String,
    first_seen: Option<DateTime<Utc>>,
    last_seen: Option<DateTime<Utc>>,
    calls: i64,
    usage: TokenUsage,
    credits: f64,
    priced_calls: i64,
    unpriced_calls: i64,
    file_path: String,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageStatRow {
    key: String,
    sessions: usize,
    calls: i64,
    usage: TokenUsage,
    credits: f64,
    usd: f64,
    priced_calls: i64,
    unpriced_calls: i64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageUnpricedModelRow {
    model: String,
    pricing_key: String,
    calls: i64,
    total_tokens: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
    pricing_stub: String,
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
    fn new(file_read_concurrency: i64, scan_all_files: bool) -> Self {
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

    fn merge_file_scan(&mut self, other: &UsageDiagnostics) {
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
pub struct UsageStatsReport {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    group_by: StatGroupBy,
    include_reasoning_effort: bool,
    sort_by: Option<StatSort>,
    limit: Option<usize>,
    sessions_dir: String,
    rows: Vec<UsageStatRow>,
    totals: UsageStatRow,
    unpriced_models: Vec<UsageUnpricedModelRow>,
    diagnostics: Option<UsageDiagnostics>,
}

#[derive(Clone, Debug)]
pub struct UsageSessionRow {
    session_id: String,
    model: String,
    cwd: String,
    first_seen: DateTime<Utc>,
    last_seen: DateTime<Utc>,
    calls: i64,
    usage: TokenUsage,
    credits: f64,
    usd: f64,
    priced_calls: i64,
    unpriced_calls: i64,
    file_path: String,
}

#[derive(Clone, Debug)]
pub struct UsageSessionEventRow {
    timestamp: DateTime<Utc>,
    model: String,
    reasoning_effort: Option<String>,
    cwd: String,
    usage: TokenUsage,
    credits: f64,
    usd: f64,
    priced: bool,
    file_path: String,
}

#[derive(Clone, Debug)]
struct UsageSessionCompactRow {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    events: usize,
    model: String,
    reasoning_effort: Option<String>,
    usage: TokenUsage,
    credits: f64,
    usd: f64,
    unpriced_calls: i64,
}

#[derive(Clone, Debug)]
pub struct UsageSessionsReport {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    sort_by: Option<StatSort>,
    limit: usize,
    sessions_dir: String,
    rows: Vec<UsageSessionRow>,
    totals: UsageStatRow,
    unpriced_models: Vec<UsageUnpricedModelRow>,
    diagnostics: Option<UsageDiagnostics>,
}

#[derive(Clone, Debug)]
pub struct UsageSessionDetailReport {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    session_id: String,
    limit: Option<usize>,
    sessions_dir: String,
    summary: Option<UsageSessionRow>,
    rows: Vec<UsageSessionEventRow>,
    by_model: Vec<UsageStatRow>,
    by_cwd: Vec<UsageStatRow>,
    by_reasoning_effort: Vec<UsageStatRow>,
    model_switches: i64,
    cwd_switches: i64,
    reasoning_effort_switches: i64,
    totals: UsageStatRow,
    unpriced_models: Vec<UsageUnpricedModelRow>,
    diagnostics: Option<UsageDiagnostics>,
}

#[derive(Debug, Clone)]
struct UsageAccountHistory {
    default_account_id: Option<String>,
    switches: Vec<UsageAccountSwitch>,
}

#[derive(Debug, Clone)]
struct UsageAccountSwitch {
    timestamp: DateTime<Utc>,
    to_account_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UsageStatsJson<'a> {
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
struct UsageSessionsJson<'a> {
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
struct UsageSessionRowJson<'a> {
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
struct UsageSessionDetailJson<'a> {
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
struct UsageSessionEventRowJson<'a> {
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

#[derive(Default)]
struct JsonlFileListing {
    files: Vec<PathBuf>,
    prefilter_candidates: Vec<PathBuf>,
}

struct PreparedUsageScan {
    range: DateRange,
    files: Vec<PathBuf>,
    diagnostics: UsageDiagnostics,
}

#[derive(Clone, Copy)]
struct JsonlScanPolicy {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    lookback_start: DateTime<Utc>,
    scan_all_files: bool,
}

impl JsonlScanPolicy {
    fn new(range: DateRange, scan_all_files: bool) -> Self {
        let duration_ms = (range.end - range.start).num_milliseconds().max(0);
        let lookback_ms = (duration_ms / 2)
            .max(BALANCED_SCAN_MIN_LOOKBACK_MS)
            .min(BALANCED_SCAN_MAX_LOOKBACK_MS);

        Self {
            start: range.start,
            end: range.end,
            lookback_start: range.start - Duration::milliseconds(lookback_ms),
            scan_all_files,
        }
    }
}

enum JsonlFileAction {
    Read,
    Prefilter,
    Skip,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthAccountHistoryStore {
    version: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_account: Option<AuthAccountHistoryAccount>,
    switches: Vec<AuthAccountSwitchEvent>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthAccountHistoryAccount {
    account_id: String,
    observed_at: String,
    source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    plan_type: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthAccountSwitchEvent {
    timestamp: String,
    from_account_id: String,
    to_account_id: String,
    source: String,
}

pub fn run_stat_command_from_args(
    args: &[String],
    help: &str,
    now: DateTime<Utc>,
) -> Result<String, AppError> {
    let (view, session, options) = parse_stat_cli_args(args, help)?;
    run_stat_command(view.as_deref(), session.as_deref(), options, now)
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
    let range = resolve_date_range(raw, now)?;
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
        Some(path) => read_optional_usage_account_history(path)?,
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

fn run_stat_command(
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

fn parse_stat_cli_args(
    args: &[String],
    help: &str,
) -> Result<(Option<String>, Option<String>, StatCommandOptions), AppError> {
    let mut options = StatCommandOptions::default();
    let mut positionals = Vec::new();
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "-g" | "--group-by" => {
                options.group_by = Some(read_string_value(args, &mut index, "--group-by")?);
            }
            "-S" | "--sort" => {
                options.sort = Some(read_string_value(args, &mut index, "--sort")?);
            }
            "-n" | "--limit" => {
                options.limit = Some(read_string_value(args, &mut index, "--limit")?);
            }
            "-T" | "--top" => {
                options.top = Some(read_string_value(args, &mut index, "--top")?);
            }
            "-A" | "--account-id" => {
                options.account_id = Some(read_string_value(args, &mut index, "--account-id")?);
            }
            "-s" | "--start" => {
                options.start = Some(read_string_value(args, &mut index, "--start")?);
            }
            "-e" | "--end" => {
                options.end = Some(read_string_value(args, &mut index, "--end")?);
            }
            "-L" | "--last" => {
                options.last = Some(read_string_value(args, &mut index, "--last")?);
            }
            "-f" | "--format" => {
                options.format = Some(read_string_value(args, &mut index, "--format")?);
            }
            "--codex-home" => {
                options.codex_home = Some(read_raw_path_value(args, &mut index, "--codex-home")?);
            }
            "--sessions-dir" => {
                options.sessions_dir =
                    Some(read_raw_path_value(args, &mut index, "--sessions-dir")?);
            }
            "--auth-file" => {
                options.auth_file = Some(read_path_value(args, &mut index, "--auth-file")?);
            }
            "--account-history-file" => {
                options.account_history_file =
                    Some(read_path_value(args, &mut index, "--account-history-file")?);
            }
            "-d" | "--detail" => options.detail = true,
            "-F" | "--full-scan" => options.full_scan = true,
            "-a" | "--all" => options.all = true,
            "-r" | "--reasoning-effort" => options.reasoning_effort = true,
            "-t" | "--today" => options.today = true,
            "--yesterday" => options.yesterday = true,
            "-m" | "--month" => options.month = true,
            "-j" | "--json" => options.json = true,
            "-v" | "--verbose" => options.verbose = true,
            value if value.starts_with("--group-by=") => {
                options.group_by = Some(value["--group-by=".len()..].to_string());
            }
            value if value.starts_with("--sort=") => {
                options.sort = Some(value["--sort=".len()..].to_string());
            }
            value if value.starts_with("--limit=") => {
                options.limit = Some(value["--limit=".len()..].to_string());
            }
            value if value.starts_with("--top=") => {
                options.top = Some(value["--top=".len()..].to_string());
            }
            value if value.starts_with("--account-id=") => {
                options.account_id = Some(value["--account-id=".len()..].to_string());
            }
            value if value.starts_with("--start=") => {
                options.start = Some(value["--start=".len()..].to_string());
            }
            value if value.starts_with("--end=") => {
                options.end = Some(value["--end=".len()..].to_string());
            }
            value if value.starts_with("--last=") => {
                options.last = Some(value["--last=".len()..].to_string());
            }
            value if value.starts_with("--format=") => {
                options.format = Some(value["--format=".len()..].to_string());
            }
            value if value.starts_with("--codex-home=") => {
                options.codex_home = Some(PathBuf::from(&value["--codex-home=".len()..]));
            }
            value if value.starts_with("--sessions-dir=") => {
                options.sessions_dir = Some(PathBuf::from(&value["--sessions-dir=".len()..]));
            }
            value if value.starts_with("--auth-file=") => {
                options.auth_file = Some(resolve_cli_path(&value["--auth-file=".len()..])?);
            }
            value if value.starts_with("--account-history-file=") => {
                options.account_history_file =
                    Some(resolve_cli_path(&value["--account-history-file=".len()..])?);
            }
            unknown if unknown.starts_with('-') => {
                return Err(AppError::invalid_input(format!(
                    "error: Unknown option: {unknown}\n\n{help}"
                )));
            }
            positional => {
                positionals.push(positional.to_string());
            }
        }

        index += 1;
    }

    if positionals.len() > 2 {
        return Err(AppError::invalid_input(format!(
            "error: Unexpected argument: {}\n\n{help}",
            positionals[2]
        )));
    }

    Ok((
        positionals.first().cloned(),
        positionals.get(1).cloned(),
        options,
    ))
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
    let range = resolve_date_range(raw, now)?;
    if range.start > range.end {
        return Err(AppError::new(
            "The stat start time must be earlier than or equal to the end time.",
        ));
    }

    let group_by = match raw.group_by.as_deref() {
        Some(value) => StatGroupBy::parse(value)?,
        None => resolve_group_by(raw, range)?,
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

trait UsageRecordAccumulator: Send {
    fn add_record(&mut self, record: UsageRecordView<'_>);
    fn empty_like(&self) -> Self;
    fn merge(&mut self, other: Self);
}

trait UsageRecordSink {
    fn on_record(&mut self, record: UsageRecordView<'_>);
}

impl<F> UsageRecordSink for F
where
    F: for<'a> FnMut(UsageRecordView<'a>),
{
    fn on_record(&mut self, record: UsageRecordView<'_>) {
        self(record);
    }
}

struct AccumulatorRecordSink<'a, A> {
    accumulator: &'a mut A,
}

impl<A> UsageRecordSink for AccumulatorRecordSink<'_, A>
where
    A: UsageRecordAccumulator,
{
    fn on_record(&mut self, record: UsageRecordView<'_>) {
        self.accumulator.add_record(record);
    }
}

struct UsageStatsAccumulator {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    group_by: StatGroupBy,
    sessions_dir: String,
    include_reasoning_effort: bool,
    sort_by: Option<StatSort>,
    limit: Option<usize>,
    rows: HashMap<String, MutableStatRow>,
    total_sessions: HashSet<String>,
    totals: TokenUsage,
    calls: i64,
    unpriced_models: HashMap<String, UsageUnpricedModelRow>,
}

impl UsageStatsAccumulator {
    fn new(
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        group_by: StatGroupBy,
        sessions_dir: String,
        include_reasoning_effort: bool,
        sort_by: Option<StatSort>,
        limit: Option<usize>,
    ) -> Self {
        Self {
            start,
            end,
            group_by,
            sessions_dir,
            include_reasoning_effort,
            sort_by,
            limit,
            rows: HashMap::new(),
            total_sessions: HashSet::new(),
            totals: TokenUsage::default(),
            calls: 0,
            unpriced_models: HashMap::new(),
        }
    }

    fn add(&mut self, record: UsageRecordView<'_>) {
        let key = group_key(&record, self.group_by, self.include_reasoning_effort);
        let row = self.rows.entry(key).or_default();
        let cost = calculate_credit_cost(record.model, record.usage.pricing_usage());

        if !row.sessions.contains(record.session_id) {
            row.sessions.insert(record.session_id.to_string());
        }
        row.calls += 1;
        row.usage.add(record.usage);
        row.credits += cost.credits;

        if cost.priced {
            row.priced_calls += 1;
        } else {
            row.unpriced_calls += 1;
            add_unpriced_model(
                &mut self.unpriced_models,
                record.model,
                record.usage,
                cost.unpriced_reason,
            );
        }

        if !self.total_sessions.contains(record.session_id) {
            self.total_sessions.insert(record.session_id.to_string());
        }
        self.totals.add(record.usage);
        self.calls += 1;
    }

    fn finish(self, diagnostics: Option<UsageDiagnostics>) -> UsageStatsReport {
        let mut formatted_rows = self
            .rows
            .into_iter()
            .map(|(key, row)| UsageStatRow {
                key,
                sessions: row.sessions.len(),
                calls: row.calls,
                usage: row.usage,
                credits: round_credits(row.credits),
                usd: credits_to_usd(row.credits),
                priced_calls: row.priced_calls,
                unpriced_calls: row.unpriced_calls,
            })
            .collect::<Vec<_>>();
        formatted_rows
            .sort_by(|left, right| compare_stat_rows(left, right, self.sort_by, self.group_by));

        let total_credits = formatted_rows.iter().map(|row| row.credits).sum::<f64>();
        let total_priced_calls = formatted_rows.iter().map(|row| row.priced_calls).sum();
        let total_unpriced_calls = formatted_rows.iter().map(|row| row.unpriced_calls).sum();
        let rows = match self.limit {
            Some(limit) => formatted_rows.into_iter().take(limit).collect(),
            None => formatted_rows,
        };

        UsageStatsReport {
            start: self.start,
            end: self.end,
            group_by: self.group_by,
            include_reasoning_effort: self.include_reasoning_effort,
            sort_by: self.sort_by,
            limit: self.limit,
            sessions_dir: self.sessions_dir,
            rows,
            totals: UsageStatRow {
                key: "Total".to_string(),
                sessions: self.total_sessions.len(),
                calls: self.calls,
                usage: self.totals,
                credits: round_credits(total_credits),
                usd: credits_to_usd(total_credits),
                priced_calls: total_priced_calls,
                unpriced_calls: total_unpriced_calls,
            },
            unpriced_models: format_unpriced_models(self.unpriced_models),
            diagnostics,
        }
    }
}

impl UsageRecordAccumulator for UsageStatsAccumulator {
    fn add_record(&mut self, record: UsageRecordView<'_>) {
        self.add(record);
    }

    fn empty_like(&self) -> Self {
        Self::new(
            self.start,
            self.end,
            self.group_by,
            self.sessions_dir.clone(),
            self.include_reasoning_effort,
            self.sort_by,
            self.limit,
        )
    }

    fn merge(&mut self, other: Self) {
        for (key, other_row) in other.rows {
            let row = self.rows.entry(key).or_default();
            row.sessions.extend(other_row.sessions);
            row.calls += other_row.calls;
            row.usage.add(&other_row.usage);
            row.credits += other_row.credits;
            row.priced_calls += other_row.priced_calls;
            row.unpriced_calls += other_row.unpriced_calls;
        }

        self.total_sessions.extend(other.total_sessions);
        self.totals.add(&other.totals);
        self.calls += other.calls;
        merge_unpriced_models(&mut self.unpriced_models, other.unpriced_models);
    }
}

struct UsageSessionsAccumulator {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    sessions_dir: String,
    sort_by: Option<StatSort>,
    limit: usize,
    sessions: HashMap<String, MutableSession>,
    totals: TokenUsage,
    calls: i64,
    unpriced_models: HashMap<String, UsageUnpricedModelRow>,
}

impl UsageSessionsAccumulator {
    fn new(
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        sessions_dir: String,
        sort_by: Option<StatSort>,
        limit: usize,
    ) -> Self {
        Self {
            start,
            end,
            sessions_dir,
            sort_by,
            limit,
            sessions: HashMap::new(),
            totals: TokenUsage::default(),
            calls: 0,
            unpriced_models: HashMap::new(),
        }
    }

    fn add(&mut self, record: UsageRecordView<'_>) {
        let session = if self.sessions.contains_key(record.session_id) {
            self.sessions
                .get_mut(record.session_id)
                .expect("session key was checked above")
        } else {
            self.sessions.insert(
                record.session_id.to_string(),
                MutableSession {
                    session_id: record.session_id.to_string(),
                    model: record.model.to_string(),
                    cwd: record.cwd.to_string(),
                    first_seen: Some(record.timestamp),
                    last_seen: Some(record.timestamp),
                    calls: 0,
                    usage: TokenUsage::default(),
                    credits: 0.0,
                    priced_calls: 0,
                    unpriced_calls: 0,
                    file_path: record.file_path.to_string(),
                },
            );
            self.sessions
                .get_mut(record.session_id)
                .expect("session was inserted above")
        };
        let cost = calculate_credit_cost(record.model, record.usage.pricing_usage());

        if record.model != "unknown" && session.model != record.model {
            session.model = record.model.to_string();
        }
        if record.cwd != "unknown" && session.cwd != record.cwd {
            session.cwd = record.cwd.to_string();
        }
        session.first_seen = Some(
            session
                .first_seen
                .unwrap_or(record.timestamp)
                .min(record.timestamp),
        );
        session.last_seen = Some(
            session
                .last_seen
                .unwrap_or(record.timestamp)
                .max(record.timestamp),
        );
        session.calls += 1;
        session.usage.add(record.usage);
        session.credits += cost.credits;

        if cost.priced {
            session.priced_calls += 1;
        } else {
            session.unpriced_calls += 1;
            add_unpriced_model(
                &mut self.unpriced_models,
                record.model,
                record.usage,
                cost.unpriced_reason,
            );
        }

        self.totals.add(record.usage);
        self.calls += 1;
    }

    fn finish(self, diagnostics: Option<UsageDiagnostics>) -> UsageSessionsReport {
        let total_sessions = self.sessions.len();
        let total_credits = self.sessions.values().map(|row| row.credits).sum::<f64>();
        let total_priced_calls = self.sessions.values().map(|row| row.priced_calls).sum();
        let total_unpriced_calls = self.sessions.values().map(|row| row.unpriced_calls).sum();
        let mut session_rows = self
            .sessions
            .into_values()
            .filter_map(|session| {
                Some(UsageSessionRow {
                    session_id: session.session_id,
                    model: session.model,
                    cwd: session.cwd,
                    first_seen: session.first_seen?,
                    last_seen: session.last_seen?,
                    calls: session.calls,
                    usage: session.usage,
                    credits: round_credits(session.credits),
                    usd: credits_to_usd(session.credits),
                    priced_calls: session.priced_calls,
                    unpriced_calls: session.unpriced_calls,
                    file_path: session.file_path,
                })
            })
            .collect::<Vec<_>>();
        session_rows.sort_by(|left, right| compare_session_rows(left, right, self.sort_by));
        let rows = session_rows
            .into_iter()
            .take(self.limit)
            .collect::<Vec<_>>();

        UsageSessionsReport {
            start: self.start,
            end: self.end,
            sort_by: self.sort_by,
            limit: self.limit,
            sessions_dir: self.sessions_dir,
            rows,
            totals: UsageStatRow {
                key: "Total".to_string(),
                sessions: total_sessions,
                calls: self.calls,
                usage: self.totals,
                credits: round_credits(total_credits),
                usd: credits_to_usd(total_credits),
                priced_calls: total_priced_calls,
                unpriced_calls: total_unpriced_calls,
            },
            unpriced_models: format_unpriced_models(self.unpriced_models),
            diagnostics,
        }
    }
}

impl UsageRecordAccumulator for UsageSessionsAccumulator {
    fn add_record(&mut self, record: UsageRecordView<'_>) {
        self.add(record);
    }

    fn empty_like(&self) -> Self {
        Self::new(
            self.start,
            self.end,
            self.sessions_dir.clone(),
            self.sort_by,
            self.limit,
        )
    }

    fn merge(&mut self, other: Self) {
        for (session_id, other_session) in other.sessions {
            if let Some(session) = self.sessions.get_mut(&session_id) {
                merge_mutable_session(session, other_session);
            } else {
                self.sessions.insert(session_id, other_session);
            }
        }

        self.totals.add(&other.totals);
        self.calls += other.calls;
        merge_unpriced_models(&mut self.unpriced_models, other.unpriced_models);
    }
}

struct UsageSessionDetailAccumulator {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    sessions_dir: String,
    limit: Option<usize>,
    session_id: String,
    rows: Vec<UsageSessionEventRow>,
    summary: Option<MutableSession>,
    totals: TokenUsage,
    calls: i64,
    credits: f64,
    priced_calls: i64,
    unpriced_calls: i64,
    unpriced_models: HashMap<String, UsageUnpricedModelRow>,
}

impl UsageSessionDetailAccumulator {
    fn new(
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        sessions_dir: String,
        limit: Option<usize>,
        session_id: String,
    ) -> Self {
        Self {
            start,
            end,
            sessions_dir,
            limit,
            session_id,
            rows: Vec::new(),
            summary: None,
            totals: TokenUsage::default(),
            calls: 0,
            credits: 0.0,
            priced_calls: 0,
            unpriced_calls: 0,
            unpriced_models: HashMap::new(),
        }
    }

    fn add(&mut self, record: UsageRecordView<'_>) {
        if record.session_id != self.session_id {
            return;
        }

        let cost = calculate_credit_cost(record.model, record.usage.pricing_usage());
        let summary = self.summary.get_or_insert_with(|| MutableSession {
            session_id: record.session_id.to_string(),
            model: record.model.to_string(),
            cwd: record.cwd.to_string(),
            first_seen: Some(record.timestamp),
            last_seen: Some(record.timestamp),
            calls: 0,
            usage: TokenUsage::default(),
            credits: 0.0,
            priced_calls: 0,
            unpriced_calls: 0,
            file_path: record.file_path.to_string(),
        });

        if record.model != "unknown" && summary.model != record.model {
            summary.model = record.model.to_string();
        }
        if record.cwd != "unknown" && summary.cwd != record.cwd {
            summary.cwd = record.cwd.to_string();
        }
        summary.first_seen = Some(
            summary
                .first_seen
                .unwrap_or(record.timestamp)
                .min(record.timestamp),
        );
        summary.last_seen = Some(
            summary
                .last_seen
                .unwrap_or(record.timestamp)
                .max(record.timestamp),
        );
        summary.calls += 1;
        summary.usage.add(record.usage);
        summary.credits += cost.credits;

        self.calls += 1;
        self.credits += cost.credits;
        self.totals.add(record.usage);

        if cost.priced {
            self.priced_calls += 1;
            summary.priced_calls += 1;
        } else {
            self.unpriced_calls += 1;
            summary.unpriced_calls += 1;
            add_unpriced_model(
                &mut self.unpriced_models,
                record.model,
                record.usage,
                cost.unpriced_reason.clone(),
            );
        }

        self.rows.push(UsageSessionEventRow {
            timestamp: record.timestamp,
            model: record.model.to_string(),
            reasoning_effort: record.reasoning_effort.map(str::to_string),
            cwd: record.cwd.to_string(),
            usage: record.usage.clone(),
            credits: round_credits(cost.credits),
            usd: credits_to_usd(cost.credits),
            priced: cost.priced,
            file_path: record.file_path.to_string(),
        });
    }

    fn finish(mut self, diagnostics: Option<UsageDiagnostics>) -> UsageSessionDetailReport {
        self.rows.sort_by(|left, right| {
            left.timestamp
                .cmp(&right.timestamp)
                .then_with(|| left.model.cmp(&right.model))
                .then_with(|| left.file_path.cmp(&right.file_path))
        });
        let all_rows = self.rows;
        let output_rows = match self.limit {
            Some(limit) => all_rows.iter().take(limit).cloned().collect(),
            None => all_rows.clone(),
        };
        let by_model = build_session_event_breakdown(&all_rows, |row| row.model.clone());
        let by_cwd = build_session_event_breakdown(&all_rows, |row| row.cwd.clone());
        let by_reasoning_effort = build_session_event_breakdown(&all_rows, |row| {
            row.reasoning_effort
                .clone()
                .unwrap_or_else(|| "unknown".to_string())
        });
        let summary = self.summary.and_then(|summary| {
            Some(UsageSessionRow {
                session_id: summary.session_id,
                model: summary.model,
                cwd: summary.cwd,
                first_seen: summary.first_seen?,
                last_seen: summary.last_seen?,
                calls: summary.calls,
                usage: summary.usage,
                credits: round_credits(summary.credits),
                usd: credits_to_usd(summary.credits),
                priced_calls: summary.priced_calls,
                unpriced_calls: summary.unpriced_calls,
                file_path: summary.file_path,
            })
        });

        UsageSessionDetailReport {
            start: self.start,
            end: self.end,
            session_id: self.session_id,
            limit: self.limit,
            sessions_dir: self.sessions_dir,
            summary,
            rows: output_rows,
            by_model,
            by_cwd,
            by_reasoning_effort,
            model_switches: count_value_switches(&all_rows, |row| row.model.as_str()),
            cwd_switches: count_value_switches(&all_rows, |row| row.cwd.as_str()),
            reasoning_effort_switches: count_value_switches(&all_rows, |row| {
                row.reasoning_effort.as_deref().unwrap_or("unknown")
            }),
            totals: UsageStatRow {
                key: "Total".to_string(),
                sessions: if self.calls == 0 { 0 } else { 1 },
                calls: self.calls,
                usage: self.totals,
                credits: round_credits(self.credits),
                usd: credits_to_usd(self.credits),
                priced_calls: self.priced_calls,
                unpriced_calls: self.unpriced_calls,
            },
            unpriced_models: format_unpriced_models(self.unpriced_models),
            diagnostics,
        }
    }
}

impl UsageRecordAccumulator for UsageSessionDetailAccumulator {
    fn add_record(&mut self, record: UsageRecordView<'_>) {
        self.add(record);
    }

    fn empty_like(&self) -> Self {
        Self::new(
            self.start,
            self.end,
            self.sessions_dir.clone(),
            self.limit,
            self.session_id.clone(),
        )
    }

    fn merge(&mut self, other: Self) {
        if let Some(other_summary) = other.summary {
            if let Some(summary) = self.summary.as_mut() {
                merge_mutable_session(summary, other_summary);
            } else {
                self.summary = Some(other_summary);
            }
        }

        self.rows.extend(other.rows);
        self.totals.add(&other.totals);
        self.calls += other.calls;
        self.credits += other.credits;
        self.priced_calls += other.priced_calls;
        self.unpriced_calls += other.unpriced_calls;
        merge_unpriced_models(&mut self.unpriced_models, other.unpriced_models);
    }
}

fn process_usage_records<F>(
    options: &ResolvedStatOptions,
    mut on_record: F,
) -> Result<UsageDiagnostics, AppError>
where
    F: for<'a> FnMut(UsageRecordView<'a>),
{
    let mut prepared = prepare_usage_scan(options)?;

    for file_path in &prepared.files {
        let scan_diagnostics = read_usage_records_from_file(
            file_path,
            prepared.range,
            options.account_history.as_ref(),
            options.account_id.as_deref(),
            &mut on_record,
        )?;
        prepared.diagnostics.merge_file_scan(&scan_diagnostics);
    }

    Ok(prepared.diagnostics)
}

fn process_usage_records_parallel<A>(
    options: &ResolvedStatOptions,
    mut accumulator: A,
) -> Result<(A, UsageDiagnostics), AppError>
where
    A: UsageRecordAccumulator,
{
    let mut prepared = prepare_usage_scan(options)?;
    let worker_count = resolve_file_scan_worker_count(prepared.files.len())?;

    if worker_count <= 1 {
        let scan_diagnostics = scan_usage_files_into_accumulator(
            &prepared.files,
            prepared.range,
            options.account_history.as_ref(),
            options.account_id.as_deref(),
            &mut accumulator,
        )?;
        prepared.diagnostics.merge_file_scan(&scan_diagnostics);
        return Ok((accumulator, prepared.diagnostics));
    }

    let partitions = partition_files_for_workers(&prepared.files, worker_count);
    let range = prepared.range;
    let account_history = options.account_history.as_ref();
    let account_id = options.account_id.as_deref();
    let mut partial_results = thread::scope(|scope| {
        let mut handles = Vec::with_capacity(partitions.len());

        for partition in partitions {
            let mut partial_accumulator = accumulator.empty_like();
            handles.push(scope.spawn(move || {
                let diagnostics = scan_usage_files_into_accumulator(
                    &partition,
                    range,
                    account_history,
                    account_id,
                    &mut partial_accumulator,
                )?;
                Ok::<_, AppError>((partial_accumulator, diagnostics))
            }));
        }

        let mut results = Vec::with_capacity(handles.len());
        for handle in handles {
            let result = handle
                .join()
                .map_err(|_| AppError::new("Rust stat file scan worker panicked."))??;
            results.push(result);
        }
        Ok::<_, AppError>(results)
    })?;

    for (partial_accumulator, diagnostics) in partial_results.drain(..) {
        prepared.diagnostics.merge_file_scan(&diagnostics);
        accumulator.merge(partial_accumulator);
    }

    Ok((accumulator, prepared.diagnostics))
}

fn prepare_usage_scan(options: &ResolvedStatOptions) -> Result<PreparedUsageScan, AppError> {
    let range = DateRange {
        start: options.start,
        end: options.end,
    };
    let mut diagnostics =
        UsageDiagnostics::new(DEFAULT_FILE_READ_CONCURRENCY, options.scan_all_files);
    let listing = list_jsonl_files(
        &options.sessions_dir,
        range,
        options.scan_all_files,
        Some(Vec::new()),
        &mut diagnostics,
    )?;
    let prefiltered_files = prefilter_files_by_last_usage(
        &listing.prefilter_candidates,
        range.start,
        &mut diagnostics,
    )?;
    let mut files = listing.files;
    files.extend(prefiltered_files);
    files.sort();
    diagnostics.read_files = files.len() as i64;

    Ok(PreparedUsageScan {
        range,
        files,
        diagnostics,
    })
}

fn scan_usage_files_into_accumulator<A>(
    files: &[PathBuf],
    range: DateRange,
    account_history: Option<&UsageAccountHistory>,
    account_id: Option<&str>,
    accumulator: &mut A,
) -> Result<UsageDiagnostics, AppError>
where
    A: UsageRecordAccumulator,
{
    let mut diagnostics = UsageDiagnostics::new(0, false);

    for file_path in files {
        let mut sink = AccumulatorRecordSink {
            accumulator: &mut *accumulator,
        };
        let scan_diagnostics =
            read_usage_records_from_file(file_path, range, account_history, account_id, &mut sink)?;
        diagnostics.merge_file_scan(&scan_diagnostics);
    }

    Ok(diagnostics)
}

fn resolve_file_scan_worker_count(file_count: usize) -> Result<usize, AppError> {
    if file_count <= 1 {
        return Ok(1);
    }

    if let Some(configured) = configured_file_scan_worker_count()? {
        return Ok(if configured == 0 {
            1
        } else {
            configured.min(file_count)
        });
    }

    if file_count < FILE_SCAN_WORKER_MIN_FILES {
        return Ok(1);
    }

    let available = thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(1);
    Ok(available
        .min(DEFAULT_MAX_FILE_SCAN_THREADS)
        .min(file_count)
        .max(1))
}

fn configured_file_scan_worker_count() -> Result<Option<usize>, AppError> {
    let Ok(raw) = env::var("CODEX_OPS_STAT_WORKERS") else {
        return Ok(None);
    };
    let trimmed = raw.trim();

    if trimmed.is_empty() {
        return Ok(None);
    }

    trimmed.parse::<usize>().map(Some).map_err(|_| {
        AppError::new("Invalid CODEX_OPS_STAT_WORKERS. Expected a non-negative integer.")
    })
}

fn partition_files_for_workers(files: &[PathBuf], worker_count: usize) -> Vec<Vec<PathBuf>> {
    if files.is_empty() {
        return Vec::new();
    }

    let partition_count = worker_count.max(1).min(files.len());
    let chunk_size = files.len().div_ceil(partition_count);
    files
        .chunks(chunk_size)
        .map(|chunk| chunk.to_vec())
        .collect::<Vec<_>>()
}

fn list_jsonl_files(
    root: &Path,
    range: DateRange,
    scan_all_files: bool,
    date_parts: Option<Vec<String>>,
    diagnostics: &mut UsageDiagnostics,
) -> Result<JsonlFileListing, AppError> {
    diagnostics.scanned_directories += 1;

    let mut entries = match fs::read_dir(root) {
        Ok(entries) => entries
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| AppError::new(error.to_string()))?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(JsonlFileListing::default())
        }
        Err(error) => return Err(AppError::new(error.to_string())),
    };
    entries.sort_by(|left, right| left.file_name().cmp(&right.file_name()));

    let mut listing = JsonlFileListing::default();
    let policy = JsonlScanPolicy::new(range, scan_all_files);

    for entry in entries {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| AppError::new(error.to_string()))?;

        if file_type.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            let next_date_parts = append_date_path_part(date_parts.as_ref(), &name);

            if let Some(parts) = next_date_parts.as_ref() {
                if should_skip_date_directory(parts, policy) {
                    diagnostics.skipped_directories += 1;
                    continue;
                }
            }

            let child_listing =
                list_jsonl_files(&path, range, scan_all_files, next_date_parts, diagnostics)?;
            listing.files.extend(child_listing.files);
            listing
                .prefilter_candidates
                .extend(child_listing.prefilter_candidates);
        } else if file_type.is_file()
            && path.extension().and_then(|value| value.to_str()) == Some("jsonl")
        {
            match classify_jsonl_file(&path, policy) {
                JsonlFileAction::Read => listing.files.push(path),
                JsonlFileAction::Prefilter => listing.prefilter_candidates.push(path),
                JsonlFileAction::Skip => diagnostics.skipped_files += 1,
            }
        }
    }

    listing.files.sort();
    listing.prefilter_candidates.sort();
    Ok(listing)
}

fn append_date_path_part(parts: Option<&Vec<String>>, name: &str) -> Option<Vec<String>> {
    let parts = parts?;

    if parts.len() >= 3 {
        return Some(parts.clone());
    }

    if parts.is_empty() && name.len() == 4 && name.chars().all(|ch| ch.is_ascii_digit()) {
        return Some(vec![name.to_string()]);
    }

    if (parts.len() == 1 || parts.len() == 2)
        && name.len() == 2
        && name.chars().all(|ch| ch.is_ascii_digit())
    {
        let mut next = parts.clone();
        next.push(name.to_string());
        return Some(next);
    }

    None
}

fn should_skip_date_directory(parts: &[String], policy: JsonlScanPolicy) -> bool {
    let Some((start, end)) = date_path_range(parts) else {
        return false;
    };

    if start > policy.end {
        return true;
    }

    !policy.scan_all_files && end < policy.lookback_start
}

fn date_path_range(parts: &[String]) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let year = parts.first()?.parse::<i32>().ok()?;

    if parts.len() == 1 {
        return Some((
            local_to_utc(year, 1, 1, 0, 0, 0, 0),
            local_to_utc(year + 1, 1, 1, 0, 0, 0, 0) - Duration::milliseconds(1),
        ));
    }

    let month = parts.get(1)?.parse::<u32>().ok()?;
    if !(1..=12).contains(&month) {
        return None;
    }

    if parts.len() == 2 {
        let (next_year, next_month) = if month == 12 {
            (year + 1, 1)
        } else {
            (year, month + 1)
        };
        return Some((
            local_to_utc(year, month, 1, 0, 0, 0, 0),
            local_to_utc(next_year, next_month, 1, 0, 0, 0, 0) - Duration::milliseconds(1),
        ));
    }

    let day = parts.get(2)?.parse::<u32>().ok()?;
    let start = local_to_utc_checked(year, month, day, 0, 0, 0, 0)?;
    Some((start, start + Duration::days(1) - Duration::milliseconds(1)))
}

fn classify_jsonl_file(path: &Path, policy: JsonlScanPolicy) -> JsonlFileAction {
    let Some(timestamp) = rollout_timestamp_from_file_name(path) else {
        return JsonlFileAction::Read;
    };

    if timestamp > policy.end {
        return JsonlFileAction::Skip;
    }

    if timestamp >= policy.start {
        return JsonlFileAction::Read;
    }

    if policy.scan_all_files || timestamp >= policy.lookback_start {
        return JsonlFileAction::Prefilter;
    }

    JsonlFileAction::Skip
}

fn prefilter_files_by_last_usage(
    files: &[PathBuf],
    start: DateTime<Utc>,
    diagnostics: &mut UsageDiagnostics,
) -> Result<Vec<PathBuf>, AppError> {
    let mut kept = Vec::new();

    for file in files {
        let last_usage_at = read_last_token_count_timestamp(file)?;

        if last_usage_at.is_some_and(|timestamp| timestamp < start) {
            diagnostics.prefiltered_files += 1;
        } else {
            kept.push(file.clone());
        }
    }

    Ok(kept)
}

fn read_last_token_count_timestamp(path: &Path) -> Result<Option<DateTime<Utc>>, AppError> {
    let file = File::open(path).map_err(|error| AppError::new(error.to_string()))?;
    let mut reader = BufReader::with_capacity(SESSION_READ_BUFFER_SIZE, file);
    let mut line = String::new();
    let mut last = None;

    loop {
        line.clear();
        let bytes_read = reader
            .read_line(&mut line)
            .map_err(|error| AppError::new(error.to_string()))?;
        if bytes_read == 0 {
            break;
        }

        if !line.contains("\"token_count\"") {
            continue;
        }

        let Ok(event) = parse_usage_json_event(&line) else {
            continue;
        };
        let Some(event) = event else {
            continue;
        };
        if event.event_type() == Some("event_msg")
            && event.payload().and_then(UsageJsonPayload::payload_type) == Some("token_count")
        {
            last = event.timestamp();
        }
    }

    Ok(last)
}

fn read_usage_records_from_file<F>(
    path: &Path,
    range: DateRange,
    account_history: Option<&UsageAccountHistory>,
    account_id_filter: Option<&str>,
    on_record: &mut F,
) -> Result<UsageDiagnostics, AppError>
where
    F: UsageRecordSink + ?Sized,
{
    let file = File::open(path).map_err(|error| AppError::new(error.to_string()))?;
    let mut reader = BufReader::with_capacity(SESSION_READ_BUFFER_SIZE, file);
    let mut line = String::new();
    let mut diagnostics = UsageDiagnostics::new(0, false);
    let mut session_id = session_id_from_path(path);
    let mut model = String::from("unknown");
    let mut reasoning_effort: Option<String> = None;
    let mut cwd = String::from("unknown");
    let mut previous_total: Option<TokenUsage> = None;
    let file_path = path_to_string(path);

    loop {
        line.clear();
        let bytes_read = reader
            .read_line(&mut line)
            .map_err(|error| AppError::new(error.to_string()))?;
        if bytes_read == 0 {
            break;
        }

        diagnostics.read_lines += 1;

        if !line.contains("\"token_count\"")
            && !line.contains("\"session_meta\"")
            && !line.contains("\"turn_context\"")
        {
            continue;
        }

        let event = match parse_usage_json_event(&line) {
            Ok(value) => value,
            Err(_) => {
                diagnostics.invalid_json_lines += 1;
                continue;
            }
        };
        let Some(event) = event else {
            continue;
        };

        let event_type = event.event_type();
        if event_type == Some("session_meta") {
            if let Some(payload) = event.payload() {
                if let Some(id) = payload.id() {
                    session_id = id.to_string();
                }
                if let Some(next_model) = payload.model() {
                    model = next_model.to_string();
                }
                if let Some(next_effort) = payload.reasoning_effort() {
                    reasoning_effort = Some(next_effort.to_string());
                }
                if let Some(next_cwd) = payload.cwd() {
                    cwd = next_cwd.to_string();
                }
            }
            continue;
        }

        if event_type == Some("turn_context") {
            if let Some(payload) = event.payload() {
                if let Some(next_model) = payload.model() {
                    model = next_model.to_string();
                }
                if let Some(next_effort) = payload.reasoning_effort() {
                    reasoning_effort = Some(next_effort.to_string());
                }
                if let Some(next_cwd) = payload.cwd() {
                    cwd = next_cwd.to_string();
                }
            }
            continue;
        }

        let Some(payload) = event.payload() else {
            continue;
        };

        if event_type != Some("event_msg") || payload.payload_type() != Some("token_count") {
            continue;
        }

        diagnostics.token_count_events += 1;
        let timestamp = event.timestamp();
        let info = payload.info();

        let (Some(timestamp), Some(info)) = (timestamp, info) else {
            diagnostics.skipped_events.missing_metadata += 1;
            continue;
        };

        let total_usage = info.total_token_usage();
        let usage = info
            .last_token_usage()
            .or_else(|| diff_usage(total_usage.as_ref(), previous_total.as_ref()));

        if let Some(total_usage) = total_usage {
            previous_total = Some(total_usage);
        }

        let Some(usage) = usage else {
            diagnostics.skipped_events.missing_usage += 1;
            continue;
        };

        if usage.is_empty() {
            diagnostics.skipped_events.empty_usage += 1;
            continue;
        }

        if timestamp < range.start || timestamp > range.end {
            diagnostics.skipped_events.out_of_range += 1;
            continue;
        }

        let account_id = resolve_usage_account_id(timestamp, account_history);
        if let Some(filter) = account_id_filter {
            if account_id.as_deref() != Some(filter) {
                diagnostics.skipped_events.account_mismatch += 1;
                continue;
            }
        }

        diagnostics.included_usage_events += 1;
        let record = UsageRecordView {
            timestamp,
            session_id: &session_id,
            model: &model,
            reasoning_effort: reasoning_effort.as_deref(),
            cwd: &cwd,
            account_id: account_id.as_deref(),
            file_path: &file_path,
            usage: &usage,
        };
        on_record.on_record(record);
    }

    Ok(diagnostics)
}

fn group_key(
    record: &UsageRecordView<'_>,
    group_by: StatGroupBy,
    include_reasoning_effort: bool,
) -> String {
    match group_by {
        StatGroupBy::Model => {
            if include_reasoning_effort {
                model_group_key(record)
            } else {
                record.model.to_string()
            }
        }
        StatGroupBy::Cwd => record.cwd.to_string(),
        StatGroupBy::Account => record
            .account_id
            .map(str::to_string)
            .unwrap_or_else(|| "unknown".to_string()),
        StatGroupBy::Week => {
            let local = record.timestamp.with_timezone(&Local);
            let week = local.iso_week();
            format!("{}-W{:02}", week.year(), week.week())
        }
        StatGroupBy::Month => {
            let local = record.timestamp.with_timezone(&Local);
            format!("{}-{:02}", local.year(), local.month())
        }
        StatGroupBy::Hour => {
            let local = record.timestamp.with_timezone(&Local);
            format!(
                "{}-{:02}-{:02} {:02}:00",
                local.year(),
                local.month(),
                local.day(),
                local.hour()
            )
        }
        StatGroupBy::Day => {
            let local = record.timestamp.with_timezone(&Local);
            format!("{}-{:02}-{:02}", local.year(), local.month(), local.day())
        }
    }
}

fn model_group_key(record: &UsageRecordView<'_>) -> String {
    let effort = record.reasoning_effort.and_then(normalize_reasoning_effort);

    if record.model == "unknown" || effort.is_none() {
        record.model.to_string()
    } else {
        format!("{}-{}", record.model, effort.unwrap())
    }
}

fn normalize_reasoning_effort(value: &str) -> Option<String> {
    let mut output = String::new();
    let mut previous_dash = false;

    for ch in value.trim().chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            output.push(ch);
            previous_dash = false;
        } else if !previous_dash {
            output.push('-');
            previous_dash = true;
        }
    }

    let normalized = output.trim_matches('-').to_string();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn compare_stat_rows(
    left: &UsageStatRow,
    right: &UsageStatRow,
    sort_by: Option<StatSort>,
    group_by: StatGroupBy,
) -> Ordering {
    match sort_by {
        None if group_by == StatGroupBy::Model => {
            by_tokens_desc(left, right).then_with(|| left.key.cmp(&right.key))
        }
        None => left.key.cmp(&right.key),
        Some(StatSort::Time) => left.key.cmp(&right.key),
        Some(StatSort::Tokens) => {
            by_tokens_desc(left, right).then_with(|| left.key.cmp(&right.key))
        }
        Some(StatSort::Credits) => {
            by_credits_desc(left.credits, right.credits).then_with(|| left.key.cmp(&right.key))
        }
        Some(StatSort::Calls) => right
            .calls
            .cmp(&left.calls)
            .then_with(|| left.key.cmp(&right.key)),
        Some(StatSort::Sessions) => right
            .sessions
            .cmp(&left.sessions)
            .then_with(|| left.key.cmp(&right.key)),
    }
}

fn compare_session_rows(
    left: &UsageSessionRow,
    right: &UsageSessionRow,
    sort_by: Option<StatSort>,
) -> Ordering {
    match sort_by {
        Some(StatSort::Time) => right
            .last_seen
            .cmp(&left.last_seen)
            .then_with(|| left.session_id.cmp(&right.session_id)),
        Some(StatSort::Tokens) => {
            by_session_tokens_desc(left, right).then_with(|| left.session_id.cmp(&right.session_id))
        }
        Some(StatSort::Credits) | None => by_credits_desc(left.credits, right.credits)
            .then_with(|| by_session_tokens_desc(left, right))
            .then_with(|| left.session_id.cmp(&right.session_id)),
        Some(StatSort::Calls) => right
            .calls
            .cmp(&left.calls)
            .then_with(|| left.session_id.cmp(&right.session_id)),
        Some(StatSort::Sessions) => left.session_id.cmp(&right.session_id),
    }
}

fn by_tokens_desc(left: &UsageStatRow, right: &UsageStatRow) -> Ordering {
    right.usage.total_tokens.cmp(&left.usage.total_tokens)
}

fn by_session_tokens_desc(left: &UsageSessionRow, right: &UsageSessionRow) -> Ordering {
    right.usage.total_tokens.cmp(&left.usage.total_tokens)
}

fn by_credits_desc(left: f64, right: f64) -> Ordering {
    right.partial_cmp(&left).unwrap_or(Ordering::Equal)
}

fn build_session_event_breakdown(
    rows: &[UsageSessionEventRow],
    key_for_row: impl Fn(&UsageSessionEventRow) -> String,
) -> Vec<UsageStatRow> {
    let mut grouped: HashMap<String, Vec<&UsageSessionEventRow>> = HashMap::new();
    for row in rows {
        grouped.entry(key_for_row(row)).or_default().push(row);
    }

    let mut output = grouped
        .into_iter()
        .map(|(key, group_rows)| {
            let mut usage = TokenUsage::default();
            let mut credits = 0.0;
            let mut priced_calls = 0;
            let mut unpriced_calls = 0;
            for row in group_rows.iter() {
                usage.add(&row.usage);
                credits += row.credits;
                if row.priced {
                    priced_calls += 1;
                } else {
                    unpriced_calls += 1;
                }
            }
            UsageStatRow {
                key,
                sessions: 1,
                calls: group_rows.len() as i64,
                usage,
                credits: round_credits(credits),
                usd: credits_to_usd(credits),
                priced_calls,
                unpriced_calls,
            }
        })
        .collect::<Vec<_>>();
    output.sort_by(|left, right| {
        by_credits_desc(left.credits, right.credits)
            .then_with(|| by_tokens_desc(left, right))
            .then_with(|| left.key.cmp(&right.key))
    });
    output
}

fn count_value_switches<'a, T>(rows: &'a [T], value_for_row: impl Fn(&'a T) -> &'a str) -> i64 {
    let mut switches = 0;
    let mut previous: Option<&str> = None;
    for row in rows {
        let value = value_for_row(row);
        if previous.is_some_and(|previous| previous != value) {
            switches += 1;
        }
        previous = Some(value);
    }
    switches
}

fn merge_mutable_session(session: &mut MutableSession, other: MutableSession) {
    if other.model != "unknown" {
        session.model = other.model;
    }
    if other.cwd != "unknown" {
        session.cwd = other.cwd;
    }

    session.first_seen = match (session.first_seen, other.first_seen) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (None, Some(right)) => Some(right),
        (left, None) => left,
    };
    session.last_seen = match (session.last_seen, other.last_seen) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (None, Some(right)) => Some(right),
        (left, None) => left,
    };
    session.calls += other.calls;
    session.usage.add(&other.usage);
    session.credits += other.credits;
    session.priced_calls += other.priced_calls;
    session.unpriced_calls += other.unpriced_calls;
}

fn merge_unpriced_models(
    target: &mut HashMap<String, UsageUnpricedModelRow>,
    source: HashMap<String, UsageUnpricedModelRow>,
) {
    for (key, source_row) in source {
        if let Some(target_row) = target.get_mut(&key) {
            target_row.calls += source_row.calls;
            target_row.total_tokens += source_row.total_tokens;
        } else {
            target.insert(key, source_row);
        }
    }
}

fn add_unpriced_model(
    unpriced_models: &mut HashMap<String, UsageUnpricedModelRow>,
    model: &str,
    usage: &TokenUsage,
    note: Option<String>,
) {
    let pricing_key = normalize_model_name(model);
    let row = unpriced_models
        .entry(pricing_key.clone())
        .or_insert_with(|| UsageUnpricedModelRow {
            model: model.to_string(),
            pricing_key,
            calls: 0,
            total_tokens: 0,
            note,
            pricing_stub: format_pricing_stub(model),
        });

    row.calls += 1;
    row.total_tokens += usage.total_tokens;
}

fn format_unpriced_models(
    unpriced_models: HashMap<String, UsageUnpricedModelRow>,
) -> Vec<UsageUnpricedModelRow> {
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
        escape_double_quoted(model)
    )
}

fn escape_double_quoted(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn format_usage_stats(
    report: &UsageStatsReport,
    format: StatFormat,
    verbose: bool,
) -> Result<String, AppError> {
    if format == StatFormat::Json {
        return Ok(format!(
            "{}\n",
            to_pretty_json(&to_usage_stats_json(report))
                .map_err(|error| AppError::new(error.to_string()))?
        ));
    }

    let mut rows = vec![usage_headers()];
    rows.extend(report.rows.iter().map(usage_row));
    rows.push(usage_row(&report.totals));

    if format == StatFormat::Csv {
        return Ok(format!("{}\n", format_csv(&rows)));
    }

    if format == StatFormat::Markdown {
        let mut lines = vec![format_markdown_table(&rows)];
        append_usage_notes(&mut lines, report, verbose);
        return Ok(format!("{}\n", lines.join("\n")));
    }

    let mut lines = vec![
        "Codex usage".to_string(),
        format!("Range: {}", format_report_range(report.start, report.end)),
        format!("Grouped by: {}", format_group_by(report)),
        format!("Sessions dir: {}", report.sessions_dir),
        String::new(),
    ];

    if report.rows.is_empty() {
        lines.push("No token usage records found in this range.".to_string());
        append_usage_notes(&mut lines, report, verbose);
        return Ok(lines.join("\n"));
    }

    lines.push(format_plain_table(&rows));
    append_usage_notes(&mut lines, report, verbose);
    Ok(lines.join("\n"))
}

fn format_usage_sessions(
    report: &UsageSessionsReport,
    format: StatFormat,
    verbose: bool,
) -> Result<String, AppError> {
    if format == StatFormat::Json {
        return Ok(format!(
            "{}\n",
            to_pretty_json(&to_usage_sessions_json(report))
                .map_err(|error| AppError::new(error.to_string()))?
        ));
    }

    let mut rows = vec![session_headers()];
    rows.extend(report.rows.iter().map(session_row));

    if format == StatFormat::Csv {
        return Ok(format!("{}\n", format_csv(&rows)));
    }

    if format == StatFormat::Markdown {
        let mut lines = vec![format_markdown_table(&rows)];
        append_usage_notes(&mut lines, report, verbose);
        return Ok(format!("{}\n", lines.join("\n")));
    }

    let mut lines = vec![
        "Codex usage sessions".to_string(),
        format!("Range: {}", format_report_range(report.start, report.end)),
        format!("Sessions dir: {}", report.sessions_dir),
        String::new(),
    ];

    if report.rows.is_empty() {
        lines.push("No token usage records found in this range.".to_string());
        append_usage_notes(&mut lines, report, verbose);
        return Ok(lines.join("\n"));
    }

    lines.push(format_plain_table(&rows));
    append_usage_notes(&mut lines, report, verbose);
    Ok(lines.join("\n"))
}

fn format_usage_session_detail(
    report: &UsageSessionDetailReport,
    format: StatFormat,
    verbose: bool,
    detail: bool,
) -> Result<String, AppError> {
    if format == StatFormat::Json {
        return Ok(format!(
            "{}\n",
            to_pretty_json(&to_usage_session_detail_json(report))
                .map_err(|error| AppError::new(error.to_string()))?
        ));
    }

    let compact_rows =
        build_usage_session_compact_rows(&report.rows, DEFAULT_SESSION_DETAIL_COMPACT_ROWS);
    let mut rows = if detail {
        let mut rows = vec![session_detail_headers()];
        rows.extend(report.rows.iter().map(session_detail_row));
        rows
    } else {
        let mut rows = vec![session_compact_headers()];
        rows.extend(compact_rows.iter().map(session_compact_row));
        rows
    };

    if format == StatFormat::Csv {
        return Ok(format!("{}\n", format_csv(&rows)));
    }

    if format == StatFormat::Markdown {
        let mut lines = vec![format_markdown_table(&rows)];
        append_usage_notes(&mut lines, report, verbose);
        return Ok(format!("{}\n", lines.join("\n")));
    }

    let mut lines = vec![
        "Codex usage session detail".to_string(),
        format!("Session: {}", report.session_id),
        format!("Range: {}", format_report_range(report.start, report.end)),
        format!("Sessions dir: {}", report.sessions_dir),
        String::new(),
    ];

    if let Some(summary) = &report.summary {
        lines.extend([
            format!("Model: {}", summary.model),
            format!("CWD: {}", summary.cwd),
            format!("First seen: {}", format_date_time(summary.first_seen)),
            format!("Last seen: {}", format_date_time(summary.last_seen)),
            format!(
                "Changes: model {}, cwd {}, reasoning effort {}",
                format_integer(report.model_switches),
                format_integer(report.cwd_switches),
                format_integer(report.reasoning_effort_switches)
            ),
            String::new(),
        ]);
    }

    if report.rows.is_empty() {
        lines.push("No token usage records found for this session in this range.".to_string());
        append_usage_notes(&mut lines, report, verbose);
        return Ok(lines.join("\n"));
    }

    if detail {
        rows.push(session_detail_total_row(&report.totals));
        lines.push(format_plain_table(&rows));
    } else {
        rows.push(session_compact_total_row(&report.totals));
        lines.push(format_plain_table(&rows));
        if report.rows.len() > DEFAULT_SESSION_DETAIL_COMPACT_ROWS {
            lines.push(String::new());
            lines.push(format!(
                "Compact view: {} row(s) from {} event(s). Use --detail for full event-level rows.",
                format_integer(compact_rows.len() as i64),
                format_integer(report.rows.len() as i64)
            ));
        }
    }

    append_session_detail_breakdown(&mut lines, "By model:", &report.by_model);
    append_session_detail_breakdown(&mut lines, "By cwd:", &report.by_cwd);
    append_session_detail_breakdown(
        &mut lines,
        "By reasoning effort:",
        &report.by_reasoning_effort,
    );
    append_usage_notes(&mut lines, report, verbose);
    Ok(lines.join("\n"))
}

trait UsageReportNotes {
    fn start(&self) -> DateTime<Utc>;
    fn end(&self) -> DateTime<Utc>;
    fn totals(&self) -> &UsageStatRow;
    fn unpriced_models(&self) -> &[UsageUnpricedModelRow];
    fn diagnostics(&self) -> Option<&UsageDiagnostics>;
}

impl UsageReportNotes for UsageStatsReport {
    fn start(&self) -> DateTime<Utc> {
        self.start
    }
    fn end(&self) -> DateTime<Utc> {
        self.end
    }
    fn totals(&self) -> &UsageStatRow {
        &self.totals
    }
    fn unpriced_models(&self) -> &[UsageUnpricedModelRow] {
        &self.unpriced_models
    }
    fn diagnostics(&self) -> Option<&UsageDiagnostics> {
        self.diagnostics.as_ref()
    }
}

impl UsageReportNotes for UsageSessionsReport {
    fn start(&self) -> DateTime<Utc> {
        self.start
    }
    fn end(&self) -> DateTime<Utc> {
        self.end
    }
    fn totals(&self) -> &UsageStatRow {
        &self.totals
    }
    fn unpriced_models(&self) -> &[UsageUnpricedModelRow] {
        &self.unpriced_models
    }
    fn diagnostics(&self) -> Option<&UsageDiagnostics> {
        self.diagnostics.as_ref()
    }
}

impl UsageReportNotes for UsageSessionDetailReport {
    fn start(&self) -> DateTime<Utc> {
        self.start
    }
    fn end(&self) -> DateTime<Utc> {
        self.end
    }
    fn totals(&self) -> &UsageStatRow {
        &self.totals
    }
    fn unpriced_models(&self) -> &[UsageUnpricedModelRow] {
        &self.unpriced_models
    }
    fn diagnostics(&self) -> Option<&UsageDiagnostics> {
        self.diagnostics.as_ref()
    }
}

fn append_usage_notes<T: UsageReportNotes>(lines: &mut Vec<String>, report: &T, verbose: bool) {
    if report.totals().unpriced_calls > 0 {
        lines.push(String::new());
        lines.push(format!(
            "Note: {} usage events had no credit price and are excluded from Credits.",
            format_integer(report.totals().unpriced_calls)
        ));

        if !report.unpriced_models().is_empty() {
            lines.push("Unpriced models:".to_string());
            for row in report.unpriced_models() {
                lines.push(format!(
                    "  {}: {} calls, {} tokens",
                    row.model,
                    format_integer(row.calls),
                    format_integer(row.total_tokens)
                ));
            }
            lines.push("Pricing stubs for src/pricing.rs:".to_string());
            for row in report.unpriced_models() {
                lines.push(indent_block(&row.pricing_stub, "  "));
            }
        }
    }

    if verbose {
        if let Some(diagnostics) = report.diagnostics() {
            lines.push(String::new());
            lines.push("Diagnostics:".to_string());
            lines.push(format!(
                "  Full file scan: {}",
                if diagnostics.scan_all_files {
                    "yes"
                } else {
                    "no"
                }
            ));
            lines.push(format!(
                "  Directories scanned: {}",
                format_integer(diagnostics.scanned_directories)
            ));
            lines.push(format!(
                "  Directories skipped by date: {}",
                format_integer(diagnostics.skipped_directories)
            ));
            lines.push(format!(
                "  Files read: {}",
                format_integer(diagnostics.read_files)
            ));
            lines.push(format!(
                "  Files skipped by date: {}",
                format_integer(diagnostics.skipped_files)
            ));
            lines.push(format!(
                "  Files skipped by last-usage prefilter: {}",
                format_integer(diagnostics.prefiltered_files)
            ));
            lines.push(format!(
                "  File read concurrency: {}",
                format_integer(diagnostics.file_read_concurrency)
            ));
            lines.push(format!(
                "  Lines read: {}",
                format_integer(diagnostics.read_lines)
            ));
            lines.push(format!(
                "  Invalid JSON lines: {}",
                format_integer(diagnostics.invalid_json_lines)
            ));
            lines.push(format!(
                "  Token count events: {}",
                format_integer(diagnostics.token_count_events)
            ));
            lines.push(format!(
                "  Usage events included: {}",
                format_integer(diagnostics.included_usage_events)
            ));
            lines.push(format!(
                "  Skipped events: missing metadata {}, missing usage {}, empty usage {}, out of range {}, account mismatch {}",
                format_integer(diagnostics.skipped_events.missing_metadata),
                format_integer(diagnostics.skipped_events.missing_usage),
                format_integer(diagnostics.skipped_events.empty_usage),
                format_integer(diagnostics.skipped_events.out_of_range),
                format_integer(diagnostics.skipped_events.account_mismatch)
            ));
        }
    }

    if should_suggest_full_scan(report.start(), report.end(), report.diagnostics()) {
        lines.push(String::new());
        lines.push(FULL_SCAN_ACCURACY_NOTE.to_string());
    }
}

fn append_session_detail_breakdown(lines: &mut Vec<String>, label: &str, rows: &[UsageStatRow]) {
    if rows.is_empty() {
        return;
    }

    let mut table_rows = vec![usage_headers()];
    table_rows.extend(rows.iter().map(usage_row));
    lines.push(String::new());
    lines.push(label.to_string());
    lines.push(format_plain_table(&table_rows));
}

fn to_usage_stats_json(report: &UsageStatsReport) -> UsageStatsJson<'_> {
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

fn to_usage_sessions_json(report: &UsageSessionsReport) -> UsageSessionsJson<'_> {
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

fn to_usage_session_detail_json(report: &UsageSessionDetailReport) -> UsageSessionDetailJson<'_> {
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

fn usage_headers() -> Vec<String> {
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

fn usage_row(row: &UsageStatRow) -> Vec<String> {
    vec![
        row.key.clone(),
        format_integer(row.sessions as i64),
        format_integer(row.calls),
        format_integer(row.usage.input_tokens),
        format_integer(row.usage.cached_input_tokens),
        format_integer(row.usage.output_tokens),
        format_integer(row.usage.reasoning_output_tokens),
        format_integer(row.usage.total_tokens),
        format_credits(row.credits),
        format_usd(row.usd),
    ]
}

fn session_headers() -> Vec<String> {
    [
        "Session",
        "Model",
        "CWD",
        "First seen",
        "Last seen",
        "Calls",
        "Input",
        "Cached",
        "Output",
        "Total",
        "Credits",
        "USD",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn session_row(row: &UsageSessionRow) -> Vec<String> {
    vec![
        row.session_id.clone(),
        row.model.clone(),
        row.cwd.clone(),
        format_date_time(row.first_seen),
        format_date_time(row.last_seen),
        format_integer(row.calls),
        format_integer(row.usage.input_tokens),
        format_integer(row.usage.cached_input_tokens),
        format_integer(row.usage.output_tokens),
        format_integer(row.usage.total_tokens),
        format_credits(row.credits),
        format_usd(row.usd),
    ]
}

fn session_detail_headers() -> Vec<String> {
    [
        "Time",
        "Model",
        "Effort",
        "CWD",
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

fn session_compact_headers() -> Vec<String> {
    [
        "Range",
        "Events",
        "Model",
        "Effort",
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

fn session_detail_row(row: &UsageSessionEventRow) -> Vec<String> {
    vec![
        format_date_time(row.timestamp),
        row.model.clone(),
        row.reasoning_effort.clone().unwrap_or_default(),
        row.cwd.clone(),
        format_integer(row.usage.input_tokens),
        format_integer(row.usage.cached_input_tokens),
        format_integer(row.usage.output_tokens),
        format_integer(row.usage.reasoning_output_tokens),
        format_integer(row.usage.total_tokens),
        if row.priced {
            format_credits(row.credits)
        } else {
            "unpriced".to_string()
        },
        if row.priced {
            format_usd(row.usd)
        } else {
            "unpriced".to_string()
        },
    ]
}

fn session_compact_row(row: &UsageSessionCompactRow) -> Vec<String> {
    vec![
        format_compact_range(row),
        format_integer(row.events as i64),
        row.model.clone(),
        row.reasoning_effort.clone().unwrap_or_default(),
        format_integer(row.usage.input_tokens),
        format_integer(row.usage.cached_input_tokens),
        format_integer(row.usage.output_tokens),
        format_integer(row.usage.reasoning_output_tokens),
        format_integer(row.usage.total_tokens),
        if row.unpriced_calls == 0 {
            format_credits(row.credits)
        } else {
            "partial".to_string()
        },
        if row.unpriced_calls == 0 {
            format_usd(row.usd)
        } else {
            "partial".to_string()
        },
    ]
}

fn session_detail_total_row(row: &UsageStatRow) -> Vec<String> {
    vec![
        "Total".to_string(),
        String::new(),
        String::new(),
        String::new(),
        format_integer(row.usage.input_tokens),
        format_integer(row.usage.cached_input_tokens),
        format_integer(row.usage.output_tokens),
        format_integer(row.usage.reasoning_output_tokens),
        format_integer(row.usage.total_tokens),
        format_credits(row.credits),
        format_usd(row.usd),
    ]
}

fn session_compact_total_row(row: &UsageStatRow) -> Vec<String> {
    vec![
        "Total".to_string(),
        format_integer(row.calls),
        String::new(),
        String::new(),
        format_integer(row.usage.input_tokens),
        format_integer(row.usage.cached_input_tokens),
        format_integer(row.usage.output_tokens),
        format_integer(row.usage.reasoning_output_tokens),
        format_integer(row.usage.total_tokens),
        format_credits(row.credits),
        format_usd(row.usd),
    ]
}

fn build_usage_session_compact_rows(
    rows: &[UsageSessionEventRow],
    max_rows: usize,
) -> Vec<UsageSessionCompactRow> {
    if rows.is_empty() {
        return Vec::new();
    }

    let safe_max_rows = max_rows.max(1);
    let runs = split_session_rows_by_model_and_effort(rows);

    if rows.len() <= safe_max_rows {
        return rows
            .iter()
            .map(|row| aggregate_session_compact_rows(&[row.clone()]))
            .collect();
    }

    if runs.len() >= safe_max_rows {
        return runs
            .iter()
            .map(|run| aggregate_session_compact_rows(run))
            .collect();
    }

    let bucket_counts = allocate_compact_buckets(&runs, safe_max_rows);
    runs.iter()
        .enumerate()
        .flat_map(|(index, run)| split_session_run(run, bucket_counts[index]))
        .collect()
}

fn split_session_rows_by_model_and_effort(
    rows: &[UsageSessionEventRow],
) -> Vec<Vec<UsageSessionEventRow>> {
    let mut runs: Vec<Vec<UsageSessionEventRow>> = Vec::new();

    for row in rows {
        let should_start = runs
            .last()
            .and_then(|run| run.last())
            .is_none_or(|previous| {
                previous.model != row.model || previous.reasoning_effort != row.reasoning_effort
            });
        if should_start {
            runs.push(vec![row.clone()]);
        } else if let Some(run) = runs.last_mut() {
            run.push(row.clone());
        }
    }

    runs
}

fn allocate_compact_buckets(runs: &[Vec<UsageSessionEventRow>], max_rows: usize) -> Vec<usize> {
    let total_events = runs.iter().map(Vec::len).sum::<usize>();
    let mut buckets = vec![1; runs.len()];
    let mut remaining = max_rows.saturating_sub(runs.len());

    while remaining > 0 {
        let mut best_index = None;
        let mut best_deficit = f64::NEG_INFINITY;

        for (index, run) in runs.iter().enumerate() {
            let bucket = buckets[index];
            if bucket >= run.len() {
                continue;
            }

            let desired = (run.len() as f64 / total_events as f64) * max_rows as f64;
            let deficit = desired - bucket as f64;
            if deficit > best_deficit {
                best_deficit = deficit;
                best_index = Some(index);
            }
        }

        let Some(best_index) = best_index else {
            break;
        };
        buckets[best_index] += 1;
        remaining -= 1;
    }

    buckets
}

fn split_session_run(
    rows: &[UsageSessionEventRow],
    bucket_count: usize,
) -> Vec<UsageSessionCompactRow> {
    let safe_bucket_count = bucket_count.max(1).min(rows.len());
    let mut buckets = Vec::new();

    for bucket_index in 0..safe_bucket_count {
        let start = (bucket_index * rows.len()) / safe_bucket_count;
        let end = ((bucket_index + 1) * rows.len()) / safe_bucket_count;
        let chunk = &rows[start..end.max(start + 1)];
        buckets.push(aggregate_session_compact_rows(chunk));
    }

    buckets
}

fn aggregate_session_compact_rows(rows: &[UsageSessionEventRow]) -> UsageSessionCompactRow {
    let first = rows.first().expect("non-empty compact rows");
    let last = rows.last().expect("non-empty compact rows");
    let mut usage = TokenUsage::default();
    let mut credits = 0.0;
    let mut unpriced_calls = 0;

    for row in rows {
        usage.add(&row.usage);
        credits += row.credits;
        if !row.priced {
            unpriced_calls += 1;
        }
    }

    UsageSessionCompactRow {
        start: first.timestamp,
        end: last.timestamp,
        events: rows.len(),
        model: first.model.clone(),
        reasoning_effort: first.reasoning_effort.clone(),
        usage,
        credits: round_credits(credits),
        usd: credits_to_usd(credits),
        unpriced_calls,
    }
}

fn format_compact_range(row: &UsageSessionCompactRow) -> String {
    let start = format_date_time(row.start);
    let end = format_date_time(row.end);
    if start == end {
        start
    } else {
        format!("{start} -> {end}")
    }
}

fn resolve_date_range(raw: &StatCommandOptions, now: DateTime<Utc>) -> Result<DateRange, AppError> {
    let quick_ranges = [
        raw.all,
        raw.today,
        raw.yesterday,
        raw.month,
        raw.last.is_some(),
    ]
    .into_iter()
    .filter(|enabled| *enabled)
    .count();

    if quick_ranges > 1 {
        return Err(AppError::new(
            "Use only one quick range option: --all, --today, --yesterday, --month, or --last.",
        ));
    }

    if quick_ranges == 1 && (raw.start.is_some() || raw.end.is_some()) {
        return Err(AppError::new(
            "Quick range options cannot be combined with --start or --end.",
        ));
    }

    if raw.all {
        return Ok(DateRange {
            start: local_to_utc(1900, 1, 1, 0, 0, 0, 0),
            end: local_to_utc(9999, 12, 31, 23, 59, 59, 999),
        });
    }

    if raw.today {
        let local = now.with_timezone(&Local);
        return Ok(DateRange {
            start: local_to_utc(local.year(), local.month(), local.day(), 0, 0, 0, 0),
            end: now,
        });
    }

    if raw.yesterday {
        let local = now.with_timezone(&Local);
        let start_today = local_to_utc(local.year(), local.month(), local.day(), 0, 0, 0, 0);
        let start = start_today - Duration::days(1);
        return Ok(DateRange {
            start,
            end: start + Duration::days(1) - Duration::milliseconds(1),
        });
    }

    if raw.month {
        let local = now.with_timezone(&Local);
        return Ok(DateRange {
            start: local_to_utc(local.year(), local.month(), 1, 0, 0, 0, 0),
            end: now,
        });
    }

    if let Some(last) = &raw.last {
        return Ok(DateRange {
            start: now - Duration::milliseconds(parse_duration_ms(last)?),
            end: now,
        });
    }

    let end = match &raw.end {
        Some(end) => parse_date_bound(end, DateBound::End)?,
        None => now,
    };
    let start = match &raw.start {
        Some(start) => parse_date_bound(start, DateBound::Start)?,
        None => end - Duration::days(7),
    };

    Ok(DateRange { start, end })
}

#[derive(Clone, Copy)]
enum DateBound {
    Start,
    End,
}

fn parse_date_bound(value: &str, bound: DateBound) -> Result<DateTime<Utc>, AppError> {
    if value.len() == 10 {
        let parts = value.split('-').collect::<Vec<_>>();
        if parts.len() == 3 {
            if let (Ok(year), Ok(month), Ok(day)) = (
                parts[0].parse::<i32>(),
                parts[1].parse::<u32>(),
                parts[2].parse::<u32>(),
            ) {
                return match bound {
                    DateBound::Start => Ok(local_to_utc(year, month, day, 0, 0, 0, 0)),
                    DateBound::End => Ok(local_to_utc(year, month, day, 23, 59, 59, 999)),
                };
            }
        }
    }

    if let Ok(date) = DateTime::parse_from_rfc3339(value) {
        return Ok(date.with_timezone(&Utc));
    }

    for pattern in [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
    ] {
        if let Ok(date) = chrono::NaiveDateTime::parse_from_str(value, pattern) {
            return local_naive_to_utc(date, value);
        }
    }

    let name = match bound {
        DateBound::Start => "start",
        DateBound::End => "end",
    };
    Err(AppError::new(format!("Invalid {name} time: {value}")))
}

fn parse_duration_ms(value: &str) -> Result<i64, AppError> {
    let trimmed = value.trim();
    let digits = trimmed
        .chars()
        .take_while(|char| char.is_ascii_digit())
        .collect::<String>();
    let unit = &trimmed[digits.len()..];

    if digits.is_empty() || !matches!(unit, "h" | "d" | "w" | "mo") {
        return Err(AppError::new(
            "Invalid --last value. Use a duration like 12h, 7d, 2w, or 1mo.",
        ));
    }

    let amount = digits
        .parse::<i64>()
        .map_err(|_| AppError::new("Invalid --last value. Duration must be a positive integer."))?;
    if amount <= 0 {
        return Err(AppError::new(
            "Invalid --last value. Duration must be a positive integer.",
        ));
    }

    let hours = match unit {
        "h" => amount,
        "d" => amount * 24,
        "w" => amount * 7 * 24,
        "mo" => amount * 30 * 24,
        _ => unreachable!("validated unit"),
    };

    Ok(hours * 60 * 60 * 1000)
}

fn resolve_group_by(raw: &StatCommandOptions, range: DateRange) -> Result<StatGroupBy, AppError> {
    if raw.all {
        return Ok(StatGroupBy::Month);
    }

    if raw.month {
        return Ok(StatGroupBy::Day);
    }

    let duration = range.end - range.start;
    if duration <= Duration::hours(48) {
        return Ok(StatGroupBy::Hour);
    }

    if duration <= Duration::days(31) {
        return Ok(StatGroupBy::Day);
    }

    if range.end <= add_months_local(range.start, 6)? {
        return Ok(StatGroupBy::Week);
    }

    Ok(StatGroupBy::Month)
}

fn add_months_local(date: DateTime<Utc>, months: i32) -> Result<DateTime<Utc>, AppError> {
    let local = date.with_timezone(&Local);
    let month_zero = local.month0() as i32 + months;
    let year = local.year() + month_zero.div_euclid(12);
    let month = month_zero.rem_euclid(12) as u32 + 1;
    let day = local.day().min(days_in_month(year, month));
    local_to_utc_checked(
        year,
        month,
        day,
        local.hour(),
        local.minute(),
        local.second(),
        local.timestamp_subsec_millis(),
    )
    .ok_or_else(|| AppError::new("Invalid local time: month adjustment"))
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let next = chrono::NaiveDate::from_ymd_opt(next_year, next_month, 1).expect("valid next month");
    (next - Duration::days(1)).day()
}

fn local_naive_to_utc(date: chrono::NaiveDateTime, value: &str) -> Result<DateTime<Utc>, AppError> {
    match Local.from_local_datetime(&date) {
        LocalResult::Single(value) => Ok(value.with_timezone(&Utc)),
        LocalResult::Ambiguous(earliest, _) => Ok(earliest.with_timezone(&Utc)),
        LocalResult::None => Err(AppError::new(format!("Invalid local time: {value}"))),
    }
}

fn local_to_utc(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    millis: u32,
) -> DateTime<Utc> {
    local_to_utc_checked(year, month, day, hour, minute, second, millis).expect("valid local date")
}

fn local_to_utc_checked(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    millis: u32,
) -> Option<DateTime<Utc>> {
    let local_result = Local.with_ymd_and_hms(year, month, day, hour, minute, second);
    match local_result {
        LocalResult::Single(value) => value
            .with_nanosecond(millis * 1_000_000)
            .map(|value| value.with_timezone(&Utc)),
        LocalResult::Ambiguous(earliest, _) => earliest
            .with_nanosecond(millis * 1_000_000)
            .map(|value| value.with_timezone(&Utc)),
        LocalResult::None => {
            let offset_seconds = Local::now().offset().fix().local_minus_utc();
            let offset = FixedOffset::east_opt(offset_seconds)?;
            offset
                .with_ymd_and_hms(year, month, day, hour, minute, second)
                .single()?
                .with_nanosecond(millis * 1_000_000)
                .map(|value| value.with_timezone(&Utc))
        }
    }
}

fn ensure_usage_account_history(
    account_history_file: &Path,
    raw: &StatCommandOptions,
    now: DateTime<Utc>,
) -> Result<UsageAccountHistory, AppError> {
    let mut store = read_auth_account_history_store(account_history_file)?;
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
        store.default_account = Some(AuthAccountHistoryAccount {
            account_id,
            observed_at: iso_string(now),
            source: "auth.json".to_string(),
            name: report.summary.name.clone(),
            email: report.summary.email.clone(),
            plan_type: report.summary.plan_type.clone(),
        });
        write_auth_account_history_store(account_history_file, &store)?;
    }
    to_usage_account_history(store)
}

fn read_auth_account_history_store(path: &Path) -> Result<AuthAccountHistoryStore, AppError> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(AuthAccountHistoryStore {
                version: AUTH_ACCOUNT_HISTORY_STORE_VERSION,
                default_account: None,
                switches: Vec::new(),
            })
        }
        Err(error) => return Err(AppError::new(error.to_string())),
    };
    let store: AuthAccountHistoryStore = serde_json::from_str(&content).map_err(|error| {
        AppError::new(format!(
            "Failed to parse {}: {}",
            path_to_string(path),
            error
        ))
    })?;
    if store.version != AUTH_ACCOUNT_HISTORY_STORE_VERSION {
        return Err(AppError::new(format!(
            "Unsupported auth account history version in {}: {}.",
            path_to_string(path),
            store.version
        )));
    }
    Ok(store)
}

fn write_auth_account_history_store(
    path: &Path,
    store: &AuthAccountHistoryStore,
) -> Result<(), AppError> {
    let content =
        serde_json::to_string_pretty(store).map_err(|error| AppError::new(error.to_string()))?;
    write_sensitive_file(path, &format!("{content}\n"))
        .map_err(|error| AppError::new(error.to_string()))
}

fn read_optional_usage_account_history(
    account_history_file: &Path,
) -> Result<Option<UsageAccountHistory>, AppError> {
    if !account_history_file.exists() {
        return Ok(None);
    }

    let store = read_auth_account_history_store(account_history_file)?;
    if store.default_account.is_none() && store.switches.is_empty() {
        return Ok(None);
    }

    to_usage_account_history(store).map(Some)
}

fn to_usage_account_history(
    mut store: AuthAccountHistoryStore,
) -> Result<UsageAccountHistory, AppError> {
    let default_account_id = store
        .default_account
        .take()
        .and_then(|account| normalize_optional_account_id(Some(&account.account_id)));
    let mut switches = store
        .switches
        .into_iter()
        .filter_map(|entry| {
            let timestamp = DateTime::parse_from_rfc3339(&entry.timestamp)
                .ok()?
                .with_timezone(&Utc);
            let to_account_id = normalize_optional_account_id(Some(&entry.to_account_id))?;
            Some(UsageAccountSwitch {
                timestamp,
                to_account_id,
            })
        })
        .collect::<Vec<_>>();
    switches.sort_by(|left, right| left.timestamp.cmp(&right.timestamp));
    Ok(UsageAccountHistory {
        default_account_id,
        switches,
    })
}

fn resolve_usage_account_id(
    timestamp: DateTime<Utc>,
    history: Option<&UsageAccountHistory>,
) -> Option<String> {
    let history = history?;
    let mut account_id = history.default_account_id.clone();
    for entry in &history.switches {
        if entry.timestamp > timestamp {
            break;
        }
        account_id = Some(entry.to_account_id.clone());
    }
    account_id
}

fn normalize_optional_account_id(value: Option<&str>) -> Option<String> {
    let normalized = value?.trim();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized.to_string())
    }
}

fn diff_usage(current: Option<&TokenUsage>, previous: Option<&TokenUsage>) -> Option<TokenUsage> {
    let current = current?;
    let Some(previous) = previous else {
        return Some(current.clone());
    };

    Some(TokenUsage {
        input_tokens: (current.input_tokens - previous.input_tokens).max(0),
        cached_input_tokens: (current.cached_input_tokens - previous.cached_input_tokens).max(0),
        output_tokens: (current.output_tokens - previous.output_tokens).max(0),
        reasoning_output_tokens: (current.reasoning_output_tokens
            - previous.reasoning_output_tokens)
            .max(0),
        total_tokens: (current.total_tokens - previous.total_tokens).max(0),
    })
}

fn session_id_from_path(path: &Path) -> String {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return path_to_string(path);
    };

    if let Some(rest) = name.strip_prefix("rollout-") {
        if let Some(id) = rest.strip_suffix(".jsonl").and_then(|rest| rest.get(20..)) {
            return id.to_string();
        }
    }

    path_to_string(path)
}

fn rollout_timestamp_from_file_name(path: &Path) -> Option<DateTime<Utc>> {
    let name = path.file_name()?.to_str()?;

    if !name.starts_with("rollout-") || !name.ends_with(".jsonl") || name.len() < 28 {
        return None;
    }

    let year = name.get(8..12)?.parse::<i32>().ok()?;
    let month = name.get(13..15)?.parse::<u32>().ok()?;
    let day = name.get(16..18)?.parse::<u32>().ok()?;
    let hour = name.get(19..21)?.parse::<u32>().ok()?;
    let minute = name.get(22..24)?.parse::<u32>().ok()?;
    let second = name.get(25..27)?.parse::<u32>().ok()?;

    local_to_utc_checked(year, month, day, hour, minute, second, 0)
}

fn usage_warnings(
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

fn should_suggest_full_scan(
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    diagnostics: Option<&UsageDiagnostics>,
) -> bool {
    diagnostics
        .is_some_and(|diagnostics| !diagnostics.scan_all_files && !is_all_usage_range(start, end))
}

fn is_all_usage_range(start: DateTime<Utc>, end: DateTime<Utc>) -> bool {
    start == local_to_utc(1900, 1, 1, 0, 0, 0, 0)
        && end == local_to_utc(9999, 12, 31, 23, 59, 59, 999)
}

fn format_report_range(start: DateTime<Utc>, end: DateTime<Utc>) -> String {
    if is_all_usage_range(start, end) {
        "all".to_string()
    } else {
        format!("{} to {}", format_date_time(start), format_date_time(end))
    }
}

fn format_group_by(report: &UsageStatsReport) -> String {
    if report.group_by == StatGroupBy::Model && report.include_reasoning_effort {
        "model + reasoning_effort".to_string()
    } else {
        report.group_by.as_str().to_string()
    }
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

fn iso_string(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
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

fn read_string_value(args: &[String], index: &mut usize, name: &str) -> Result<String, AppError> {
    *index += 1;
    args.get(*index)
        .cloned()
        .filter(|value| !value.starts_with("--"))
        .ok_or_else(|| AppError::invalid_input(format!("error: Missing value for {name}")))
}

fn read_path_value(args: &[String], index: &mut usize, name: &str) -> Result<PathBuf, AppError> {
    let value = read_string_value(args, index, name)?;
    resolve_cli_path(&value)
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

fn indent_block(value: &str, prefix: &str) -> String {
    value
        .split('\n')
        .map(|line| format!("{prefix}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stat_short_options() {
        let args = vec![
            "-g".to_string(),
            "model".to_string(),
            "-S".to_string(),
            "credits".to_string(),
            "-n".to_string(),
            "1".to_string(),
            "-r".to_string(),
            "-a".to_string(),
            "-F".to_string(),
            "-v".to_string(),
            "-j".to_string(),
            "--sessions-dir".to_string(),
            "/tmp/sessions".to_string(),
        ];
        let (_, _, options) = parse_stat_cli_args(&args, "help").expect("parse");
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

    #[test]
    fn compacts_session_runs_by_model_and_effort() {
        let rows = (0..30)
            .map(|index| UsageSessionEventRow {
                timestamp: Utc
                    .with_ymd_and_hms(2026, 5, 10, 10, index, 0)
                    .single()
                    .expect("time"),
                model: if index < 15 { "gpt-5.5" } else { "gpt-5.4" }.to_string(),
                reasoning_effort: if index < 10 {
                    Some("high".to_string())
                } else if index < 20 {
                    Some("xhigh".to_string())
                } else {
                    None
                },
                cwd: "/repo".to_string(),
                usage: TokenUsage {
                    input_tokens: 10,
                    cached_input_tokens: 1,
                    output_tokens: 2,
                    reasoning_output_tokens: 1,
                    total_tokens: 12,
                },
                credits: 0.0,
                usd: 0.0,
                priced: true,
                file_path: "/tmp/session.jsonl".to_string(),
            })
            .collect::<Vec<_>>();
        let compact = build_usage_session_compact_rows(&rows, 20);

        assert!(compact.len() <= 20);
        assert!(compact
            .iter()
            .any(|row| row.model == "gpt-5.5" && row.reasoning_effort.as_deref() == Some("high")));
        assert!(compact
            .iter()
            .any(|row| row.model == "gpt-5.4" && row.reasoning_effort.is_none()));
    }

    #[test]
    fn partitions_files_for_workers_in_stable_order() {
        let files = (0..10)
            .map(|index| PathBuf::from(format!("file-{index}.jsonl")))
            .collect::<Vec<_>>();
        let partitions = partition_files_for_workers(&files, 3);

        assert_eq!(
            partitions.iter().map(Vec::len).collect::<Vec<_>>(),
            vec![4, 4, 2]
        );
        assert_eq!(partitions.into_iter().flatten().collect::<Vec<_>>(), files);
        assert!(partition_files_for_workers(&[], 8).is_empty());
        assert_eq!(partition_files_for_workers(&files[..2], 8).len(), 2);
    }

    #[test]
    fn merges_stats_accumulators_without_losing_totals() {
        let start = utc_time(2026, 5, 10, 0);
        let end = utc_time(2026, 5, 10, 2);
        let mut left = UsageStatsAccumulator::new(
            start,
            end,
            StatGroupBy::Model,
            "/sessions".to_string(),
            false,
            None,
            None,
        );
        let mut right = left.empty_like();
        let left_usage = usage(10, 2, 12);
        let right_usage = usage(20, 3, 23);

        left.add(test_record(
            utc_time(2026, 5, 10, 0),
            "session-a",
            "gpt-5.5",
            "/repo-a",
            "/tmp/a.jsonl",
            &left_usage,
        ));
        right.add(test_record(
            utc_time(2026, 5, 10, 1),
            "session-b",
            "gpt-5.4",
            "/repo-b",
            "/tmp/b.jsonl",
            &right_usage,
        ));

        left.merge(right);
        let report = left.finish(None);

        assert_eq!(report.totals.calls, 2);
        assert_eq!(report.totals.sessions, 2);
        assert_eq!(report.totals.usage.input_tokens, 30);
        assert_eq!(report.totals.usage.output_tokens, 5);
        assert_eq!(report.totals.usage.total_tokens, 35);
        assert_eq!(report.rows.len(), 2);
    }

    #[test]
    fn merges_session_accumulators_in_file_partition_order() {
        let start = utc_time(2026, 5, 10, 0);
        let end = utc_time(2026, 5, 10, 2);
        let mut left = UsageSessionsAccumulator::new(start, end, "/sessions".to_string(), None, 10);
        let mut right = left.empty_like();
        let left_usage = usage(10, 2, 12);
        let right_usage = usage(20, 3, 23);

        left.add(test_record(
            utc_time(2026, 5, 10, 1),
            "session-a",
            "gpt-5.5",
            "/repo-a",
            "/tmp/a.jsonl",
            &left_usage,
        ));
        right.add(test_record(
            utc_time(2026, 5, 10, 0),
            "session-a",
            "gpt-5.4",
            "/repo-b",
            "/tmp/b.jsonl",
            &right_usage,
        ));

        left.merge(right);
        let report = left.finish(None);
        let row = report.rows.first().expect("merged session row");

        assert_eq!(report.totals.calls, 2);
        assert_eq!(report.totals.sessions, 1);
        assert_eq!(row.session_id, "session-a");
        assert_eq!(row.model, "gpt-5.4");
        assert_eq!(row.cwd, "/repo-b");
        assert_eq!(row.file_path, "/tmp/a.jsonl");
        assert_eq!(row.first_seen, utc_time(2026, 5, 10, 0));
        assert_eq!(row.last_seen, utc_time(2026, 5, 10, 1));
    }

    fn test_record<'a>(
        timestamp: DateTime<Utc>,
        session_id: &'a str,
        model: &'a str,
        cwd: &'a str,
        file_path: &'a str,
        usage: &'a TokenUsage,
    ) -> UsageRecordView<'a> {
        UsageRecordView {
            timestamp,
            session_id,
            model,
            reasoning_effort: None,
            cwd,
            account_id: None,
            file_path,
            usage,
        }
    }

    fn usage(input_tokens: i64, output_tokens: i64, total_tokens: i64) -> TokenUsage {
        TokenUsage {
            input_tokens,
            cached_input_tokens: 0,
            output_tokens,
            reasoning_output_tokens: 0,
            total_tokens,
        }
    }

    fn utc_time(year: i32, month: u32, day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, 0, 0)
            .single()
            .expect("utc time")
    }
}
