// Legacy high-alignment `stat -a --format json` POC retained for benchmark
// traceability. The formal Rust binary starts at `src/main.rs`.

use chrono::{
    DateTime, Datelike, Duration, FixedOffset, Local, LocalResult, Offset, SecondsFormat, TimeZone,
    Timelike, Utc,
};
use serde::Serialize;
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::env;
use std::error::Error;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process;

const DEFAULT_FILE_READ_CONCURRENCY: usize = 8;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;
const BALANCED_SCAN_MIN_LOOKBACK_MS: i64 = 2 * DAY_MS;
const BALANCED_SCAN_MAX_LOOKBACK_MS: i64 = 7 * DAY_MS;
const FULL_SCAN_ACCURACY_NOTE: &str =
    "Note: This report used balanced scanning, not a full scan. It reads in-range files and checks a bounded lookback by last token_count timestamp. Use -F, --full-scan to check all pre-range rollout files for exact local token_count results.";

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let cli = CliOptions::parse()?;

    if cli.format != "json" {
        return Err(invalid_input("Rust stat POC only supports --format json"));
    }

    let range = if cli.all {
        DateRange {
            start: local_to_utc(1900, 1, 1, 0, 0, 0, 0),
            end: local_to_utc(9999, 12, 31, 23, 59, 59, 999),
        }
    } else {
        let end = Utc::now();
        DateRange {
            start: end - Duration::days(7),
            end,
        }
    };

    let report = read_usage_stats(&cli, range)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

#[derive(Debug)]
struct CliOptions {
    all: bool,
    format: String,
    sessions_dir: String,
    group_by: GroupBy,
    include_reasoning_effort: bool,
    sort_by: Option<SortBy>,
    limit: Option<usize>,
    scan_all_files: bool,
}

impl CliOptions {
    fn parse() -> Result<Self, Box<dyn Error>> {
        let args: Vec<String> = env::args().skip(1).collect();
        let mut index = 0;

        if args.get(index).map(String::as_str) == Some("stat") {
            index += 1;
        }

        let mut all = false;
        let mut format = String::from("table");
        let mut sessions_dir = default_sessions_dir();
        let mut group_by: Option<GroupBy> = None;
        let mut include_reasoning_effort = false;
        let mut sort_by = None;
        let mut limit = None;
        let mut scan_all_files = false;

        while index < args.len() {
            let arg = &args[index];
            match arg.as_str() {
                "-a" | "--all" => {
                    all = true;
                    index += 1;
                }
                "--json" => {
                    format = String::from("json");
                    index += 1;
                }
                "--reasoning-effort" => {
                    include_reasoning_effort = true;
                    index += 1;
                }
                "-F" | "--full-scan" => {
                    scan_all_files = true;
                    index += 1;
                }
                "--format" => {
                    format = read_option_value(&args, &mut index, "--format")?;
                }
                "--sessions-dir" => {
                    sessions_dir = read_option_value(&args, &mut index, "--sessions-dir")?;
                }
                "--group-by" => {
                    group_by = Some(GroupBy::parse(&read_option_value(
                        &args,
                        &mut index,
                        "--group-by",
                    )?)?);
                }
                "--sort" => {
                    sort_by = Some(SortBy::parse(&read_option_value(
                        &args, &mut index, "--sort",
                    )?)?);
                }
                "--limit" => {
                    let raw = read_option_value(&args, &mut index, "--limit")?;
                    let parsed = raw.parse::<usize>().map_err(|_| {
                        invalid_input("Invalid --limit value. Expected a positive integer.")
                    })?;
                    if parsed == 0 {
                        return Err(invalid_input(
                            "Invalid --limit value. Expected a positive integer.",
                        ));
                    }
                    limit = Some(parsed);
                }
                _ if arg.starts_with("--format=") => {
                    format = arg["--format=".len()..].to_string();
                    index += 1;
                }
                _ if arg.starts_with("--sessions-dir=") => {
                    sessions_dir = arg["--sessions-dir=".len()..].to_string();
                    index += 1;
                }
                _ if arg.starts_with("--group-by=") => {
                    group_by = Some(GroupBy::parse(&arg["--group-by=".len()..])?);
                    index += 1;
                }
                _ if arg.starts_with("--sort=") => {
                    sort_by = Some(SortBy::parse(&arg["--sort=".len()..])?);
                    index += 1;
                }
                _ if arg.starts_with("--limit=") => {
                    let raw = &arg["--limit=".len()..];
                    let parsed = raw.parse::<usize>().map_err(|_| {
                        invalid_input("Invalid --limit value. Expected a positive integer.")
                    })?;
                    if parsed == 0 {
                        return Err(invalid_input(
                            "Invalid --limit value. Expected a positive integer.",
                        ));
                    }
                    limit = Some(parsed);
                    index += 1;
                }
                _ => {
                    return Err(invalid_input(format!(
                        "Unsupported Rust stat POC argument: {arg}"
                    )));
                }
            }
        }

        Ok(Self {
            all,
            format,
            sessions_dir,
            group_by: group_by.unwrap_or(if all { GroupBy::Month } else { GroupBy::Day }),
            include_reasoning_effort,
            sort_by,
            limit,
            scan_all_files,
        })
    }
}

fn read_option_value(
    args: &[String],
    index: &mut usize,
    name: &str,
) -> Result<String, Box<dyn Error>> {
    let value = args
        .get(*index + 1)
        .ok_or_else(|| invalid_input(format!("Missing value for {name}")))?;
    *index += 2;
    Ok(value.to_string())
}

fn default_sessions_dir() -> String {
    let codex_home = env::var("CODEX_HOME")
        .ok()
        .or_else(|| env::var("HOME").ok().map(|home| format!("{home}/.codex")))
        .unwrap_or_else(|| String::from(".codex"));
    format!("{codex_home}/sessions")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GroupBy {
    Hour,
    Day,
    Week,
    Month,
    Model,
    Cwd,
}

impl GroupBy {
    fn parse(value: &str) -> Result<Self, Box<dyn Error>> {
        match value {
            "hour" => Ok(Self::Hour),
            "day" => Ok(Self::Day),
            "week" => Ok(Self::Week),
            "month" => Ok(Self::Month),
            "model" => Ok(Self::Model),
            "cwd" => Ok(Self::Cwd),
            "account" => Err(invalid_input(
                "Rust stat POC does not support --group-by account.",
            )),
            _ => Err(invalid_input(
                "Invalid group-by value. Expected one of: hour, day, week, month, model, cwd.",
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
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum SortBy {
    Time,
    Tokens,
    Credits,
    Calls,
    Sessions,
}

impl SortBy {
    fn parse(value: &str) -> Result<Self, Box<dyn Error>> {
        match value {
            "time" => Ok(Self::Time),
            "tokens" => Ok(Self::Tokens),
            "credits" => Ok(Self::Credits),
            "calls" => Ok(Self::Calls),
            "sessions" => Ok(Self::Sessions),
            _ => Err(invalid_input(
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

#[derive(Clone, Copy)]
struct DateRange {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct TokenUsage {
    input_tokens: i64,
    cached_input_tokens: i64,
    output_tokens: i64,
    reasoning_output_tokens: i64,
    total_tokens: i64,
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
}

#[derive(Clone, Debug)]
struct UsageRecord {
    timestamp: DateTime<Utc>,
    session_id: String,
    model: String,
    reasoning_effort: Option<String>,
    cwd: String,
    usage: TokenUsage,
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UsageStatRow {
    key: String,
    sessions: usize,
    calls: i64,
    usage: TokenUsage,
    credits: f64,
    usd: f64,
    priced_calls: i64,
    unpriced_calls: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UsageUnpricedModelRow {
    model: String,
    pricing_key: String,
    calls: i64,
    total_tokens: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
    pricing_stub: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UsageDiagnostics {
    scan_all_files: bool,
    scanned_directories: i64,
    skipped_directories: i64,
    read_files: i64,
    skipped_files: i64,
    prefiltered_files: i64,
    read_lines: i64,
    invalid_json_lines: i64,
    token_count_events: i64,
    included_usage_events: i64,
    skipped_events: SkippedEvents,
    file_read_concurrency: i64,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct SkippedEvents {
    missing_metadata: i64,
    missing_usage: i64,
    empty_usage: i64,
    out_of_range: i64,
    account_mismatch: i64,
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UsageStatsReport {
    start: String,
    end: String,
    group_by: String,
    include_reasoning_effort: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    sort_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<usize>,
    sessions_dir: String,
    rows: Vec<UsageStatRow>,
    totals: UsageStatRow,
    unpriced_models: Vec<UsageUnpricedModelRow>,
    warnings: Vec<String>,
    diagnostics: UsageDiagnostics,
}

fn read_usage_stats(
    cli: &CliOptions,
    range: DateRange,
) -> Result<UsageStatsReport, Box<dyn Error>> {
    let mut diagnostics =
        UsageDiagnostics::new(DEFAULT_FILE_READ_CONCURRENCY as i64, cli.scan_all_files);
    let listing = list_jsonl_files(
        Path::new(&cli.sessions_dir),
        range,
        cli.scan_all_files,
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

    let mut rows: HashMap<String, MutableStatRow> = HashMap::new();
    let mut total_sessions: HashSet<String> = HashSet::new();
    let mut totals_usage = TokenUsage::default();
    let mut calls = 0;
    let mut unpriced_models: HashMap<String, UsageUnpricedModelRow> = HashMap::new();

    for file_path in files {
        let scan = read_usage_records_from_file(&file_path, range)?;
        diagnostics.merge_file_scan(&scan.diagnostics);

        for record in scan.records {
            let key = group_key(&record, cli.group_by, cli.include_reasoning_effort);
            let row = rows.entry(key).or_default();
            let cost = calculate_credit_cost(&record.model, &record.usage);

            row.sessions.insert(record.session_id.clone());
            row.calls += 1;
            row.usage.add(&record.usage);
            row.credits += cost.credits;

            if cost.priced {
                row.priced_calls += 1;
            } else {
                row.unpriced_calls += 1;
                add_unpriced_model(&mut unpriced_models, &record, &cost);
            }

            total_sessions.insert(record.session_id);
            totals_usage.add(&record.usage);
            calls += 1;
        }
    }

    let mut formatted_rows: Vec<UsageStatRow> = rows
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
        .collect();

    formatted_rows.sort_by(|left, right| compare_stat_rows(left, right, cli.sort_by, cli.group_by));

    let total_credits = formatted_rows.iter().map(|row| row.credits).sum::<f64>();
    let total_priced_calls = formatted_rows.iter().map(|row| row.priced_calls).sum();
    let total_unpriced_calls = formatted_rows.iter().map(|row| row.unpriced_calls).sum();

    let output_rows = match cli.limit {
        Some(limit) => formatted_rows.into_iter().take(limit).collect::<Vec<_>>(),
        None => formatted_rows,
    };

    let totals = UsageStatRow {
        key: String::from("Total"),
        sessions: total_sessions.len(),
        calls,
        usage: totals_usage,
        credits: round_credits(total_credits),
        usd: credits_to_usd(total_credits),
        priced_calls: total_priced_calls,
        unpriced_calls: total_unpriced_calls,
    };

    Ok(UsageStatsReport {
        start: iso_string(range.start),
        end: iso_string(range.end),
        group_by: cli.group_by.as_str().to_string(),
        include_reasoning_effort: cli.include_reasoning_effort,
        sort_by: cli.sort_by.map(|sort| sort.as_str().to_string()),
        limit: cli.limit,
        sessions_dir: cli.sessions_dir.clone(),
        rows: output_rows,
        totals,
        unpriced_models: format_unpriced_models(unpriced_models),
        warnings: usage_warnings(range, cli.scan_all_files),
        diagnostics,
    })
}

#[derive(Default)]
struct JsonlFileListing {
    files: Vec<PathBuf>,
    prefilter_candidates: Vec<PathBuf>,
}

fn list_jsonl_files(
    root: &Path,
    range: DateRange,
    scan_all_files: bool,
    date_parts: Option<Vec<String>>,
    diagnostics: &mut UsageDiagnostics,
) -> Result<JsonlFileListing, Box<dyn Error>> {
    diagnostics.scanned_directories += 1;

    let mut entries = match fs::read_dir(root) {
        Ok(entries) => entries.collect::<Result<Vec<_>, _>>()?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(JsonlFileListing::default())
        }
        Err(error) => return Err(Box::new(error)),
    };
    entries.sort_by(|left, right| left.file_name().cmp(&right.file_name()));

    let mut listing = JsonlFileListing::default();
    let policy = JsonlScanPolicy::new(range, scan_all_files);

    for entry in entries {
        let path = entry.path();
        let file_type = entry.file_type()?;

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
    let start = local_to_utc(year, month, day, 0, 0, 0, 0);
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
) -> Result<Vec<PathBuf>, Box<dyn Error>> {
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

fn read_last_token_count_timestamp(path: &Path) -> Result<Option<DateTime<Utc>>, Box<dyn Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut last = None;

    for line in reader.lines() {
        let line = line?;
        if !line.contains("\"token_count\"") {
            continue;
        }

        let Ok(event) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let payload = event.get("payload").and_then(Value::as_object);
        if read_string(event.get("type")).as_deref() == Some("event_msg")
            && payload
                .and_then(|payload| payload.get("type"))
                .and_then(|value| read_string(Some(value)))
                .as_deref()
                == Some("token_count")
        {
            last = read_date(event.get("timestamp"));
        }
    }

    Ok(last)
}

struct FileUsageScan {
    records: Vec<UsageRecord>,
    diagnostics: UsageDiagnostics,
}

fn read_usage_records_from_file(
    path: &Path,
    range: DateRange,
) -> Result<FileUsageScan, Box<dyn Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut diagnostics = UsageDiagnostics::new(0, false);
    let mut records = Vec::new();
    let mut session_id = session_id_from_path(path);
    let mut model = String::from("unknown");
    let mut reasoning_effort: Option<String> = None;
    let mut cwd = String::from("unknown");
    let mut previous_total: Option<TokenUsage> = None;

    for line in reader.lines() {
        let line = line?;
        diagnostics.read_lines += 1;

        if !line.contains("\"token_count\"")
            && !line.contains("\"session_meta\"")
            && !line.contains("\"turn_context\"")
        {
            continue;
        }

        let event = match serde_json::from_str::<Value>(&line) {
            Ok(value) => value,
            Err(_) => {
                diagnostics.invalid_json_lines += 1;
                continue;
            }
        };

        let event_type = read_string(event.get("type"));
        if event_type.as_deref() == Some("session_meta") {
            if let Some(payload) = event.get("payload").and_then(Value::as_object) {
                if let Some(id) = payload.get("id").and_then(|value| read_string(Some(value))) {
                    session_id = id;
                }
                if let Some(next_model) = payload
                    .get("model")
                    .and_then(|value| read_string(Some(value)))
                {
                    model = next_model;
                }
                if let Some(next_effort) = read_reasoning_effort(payload) {
                    reasoning_effort = Some(next_effort);
                }
                if let Some(next_cwd) = payload
                    .get("cwd")
                    .and_then(|value| read_string(Some(value)))
                {
                    cwd = next_cwd;
                }
            }
            continue;
        }

        if event_type.as_deref() == Some("turn_context") {
            if let Some(payload) = event.get("payload").and_then(Value::as_object) {
                if let Some(next_model) = payload
                    .get("model")
                    .and_then(|value| read_string(Some(value)))
                {
                    model = next_model;
                }
                if let Some(next_effort) = read_reasoning_effort(payload) {
                    reasoning_effort = Some(next_effort);
                }
                if let Some(next_cwd) = payload
                    .get("cwd")
                    .and_then(|value| read_string(Some(value)))
                {
                    cwd = next_cwd;
                }
            }
            continue;
        }

        let Some(payload) = event.get("payload").and_then(Value::as_object) else {
            continue;
        };

        if event_type.as_deref() != Some("event_msg")
            || payload
                .get("type")
                .and_then(|value| read_string(Some(value)))
                .as_deref()
                != Some("token_count")
        {
            continue;
        }

        diagnostics.token_count_events += 1;
        let timestamp = read_date(event.get("timestamp"));
        let info = payload.get("info").and_then(Value::as_object);

        let (Some(timestamp), Some(info)) = (timestamp, info) else {
            diagnostics.skipped_events.missing_metadata += 1;
            continue;
        };

        let total_usage = read_token_usage(info.get("total_token_usage"));
        let usage = read_token_usage(info.get("last_token_usage"))
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

        diagnostics.included_usage_events += 1;
        records.push(UsageRecord {
            timestamp,
            session_id: session_id.clone(),
            model: model.clone(),
            reasoning_effort: reasoning_effort.clone(),
            cwd: cwd.clone(),
            usage,
        });
    }

    Ok(FileUsageScan {
        records,
        diagnostics,
    })
}

fn group_key(record: &UsageRecord, group_by: GroupBy, include_reasoning_effort: bool) -> String {
    match group_by {
        GroupBy::Model => {
            if include_reasoning_effort {
                model_group_key(record)
            } else {
                record.model.clone()
            }
        }
        GroupBy::Cwd => record.cwd.clone(),
        GroupBy::Week => {
            let local = record.timestamp.with_timezone(&Local);
            let week = local.iso_week();
            format!("{}-W{:02}", week.year(), week.week())
        }
        GroupBy::Month => {
            let local = record.timestamp.with_timezone(&Local);
            format!("{}-{:02}", local.year(), local.month())
        }
        GroupBy::Hour => {
            let local = record.timestamp.with_timezone(&Local);
            format!(
                "{}-{:02}-{:02} {:02}:00",
                local.year(),
                local.month(),
                local.day(),
                local.hour()
            )
        }
        GroupBy::Day => {
            let local = record.timestamp.with_timezone(&Local);
            format!("{}-{:02}-{:02}", local.year(), local.month(), local.day())
        }
    }
}

fn model_group_key(record: &UsageRecord) -> String {
    let effort = record
        .reasoning_effort
        .as_deref()
        .and_then(normalize_reasoning_effort);

    if record.model == "unknown" || effort.is_none() {
        record.model.clone()
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
    sort_by: Option<SortBy>,
    group_by: GroupBy,
) -> Ordering {
    match sort_by {
        None if group_by == GroupBy::Model => {
            by_tokens_desc(left, right).then_with(|| left.key.cmp(&right.key))
        }
        None => left.key.cmp(&right.key),
        Some(SortBy::Time) => left.key.cmp(&right.key),
        Some(SortBy::Tokens) => by_tokens_desc(left, right).then_with(|| left.key.cmp(&right.key)),
        Some(SortBy::Credits) => {
            by_credits_desc(left, right).then_with(|| left.key.cmp(&right.key))
        }
        Some(SortBy::Calls) => right
            .calls
            .cmp(&left.calls)
            .then_with(|| left.key.cmp(&right.key)),
        Some(SortBy::Sessions) => right
            .sessions
            .cmp(&left.sessions)
            .then_with(|| left.key.cmp(&right.key)),
    }
}

fn by_tokens_desc(left: &UsageStatRow, right: &UsageStatRow) -> Ordering {
    right.usage.total_tokens.cmp(&left.usage.total_tokens)
}

fn by_credits_desc(left: &UsageStatRow, right: &UsageStatRow) -> Ordering {
    right
        .credits
        .partial_cmp(&left.credits)
        .unwrap_or(Ordering::Equal)
}

struct CreditCost {
    priced: bool,
    unpriced_reason: Option<String>,
    credits: f64,
}

fn calculate_credit_cost(model: &str, usage: &TokenUsage) -> CreditCost {
    let billable_cached = usage.cached_input_tokens.max(0).min(usage.input_tokens);
    let billable_input = (usage.input_tokens - billable_cached).max(0);

    if let Some(pricing) = model_pricing(model) {
        CreditCost {
            priced: true,
            unpriced_reason: None,
            credits: (billable_input as f64 * pricing.input_credits_per_million
                + billable_cached as f64 * pricing.cached_input_credits_per_million
                + usage.output_tokens as f64 * pricing.output_credits_per_million)
                / 1_000_000.0,
        }
    } else {
        CreditCost {
            priced: false,
            unpriced_reason: None,
            credits: 0.0,
        }
    }
}

struct ModelPricing {
    input_credits_per_million: f64,
    cached_input_credits_per_million: f64,
    output_credits_per_million: f64,
}

fn model_pricing(model: &str) -> Option<ModelPricing> {
    match pricing_key_for_model(model).as_str() {
        "gpt-5.5" => Some(ModelPricing {
            input_credits_per_million: 125.0,
            cached_input_credits_per_million: 12.5,
            output_credits_per_million: 750.0,
        }),
        "gpt-5.4" => Some(ModelPricing {
            input_credits_per_million: 62.5,
            cached_input_credits_per_million: 6.25,
            output_credits_per_million: 375.0,
        }),
        "gpt-5.4-mini" => Some(ModelPricing {
            input_credits_per_million: 18.75,
            cached_input_credits_per_million: 1.875,
            output_credits_per_million: 113.0,
        }),
        "gpt-5.3-codex" => Some(ModelPricing {
            input_credits_per_million: 43.75,
            cached_input_credits_per_million: 4.375,
            output_credits_per_million: 350.0,
        }),
        "gpt-5.2" => Some(ModelPricing {
            input_credits_per_million: 43.75,
            cached_input_credits_per_million: 4.375,
            output_credits_per_million: 350.0,
        }),
        "gpt-5.3-codex-spark" => Some(ModelPricing {
            input_credits_per_million: 0.0,
            cached_input_credits_per_million: 0.0,
            output_credits_per_million: 0.0,
        }),
        "gpt-image-2 (image)" => Some(ModelPricing {
            input_credits_per_million: 200.0,
            cached_input_credits_per_million: 50.0,
            output_credits_per_million: 750.0,
        }),
        "gpt-image-2 (text)" => Some(ModelPricing {
            input_credits_per_million: 125.0,
            cached_input_credits_per_million: 31.25,
            output_credits_per_million: 250.0,
        }),
        _ => None,
    }
}

fn pricing_key_for_model(model: &str) -> String {
    let normalized = normalize_model_name(model);
    match normalized.as_str() {
        "gpt-5.3-codex-spark" => String::from("gpt-5.3-codex-spark"),
        "gpt-5.4 mini" => String::from("gpt-5.4-mini"),
        "gpt-5.3 codex" => String::from("gpt-5.3-codex"),
        "gpt-image-2:image"
        | "gpt-image-2-image"
        | "gpt-image-2 image"
        | "gpt-image-2.0:image"
        | "gpt-image-2.0-image"
        | "gpt-image-2.0 image"
        | "gpt-image-2.0 (image)" => String::from("gpt-image-2 (image)"),
        "gpt-image-2:text"
        | "gpt-image-2-text"
        | "gpt-image-2 text"
        | "gpt-image-2.0:text"
        | "gpt-image-2.0-text"
        | "gpt-image-2.0 text"
        | "gpt-image-2.0 (text)" => String::from("gpt-image-2 (text)"),
        _ => normalized,
    }
}

fn normalize_model_name(model: &str) -> String {
    model
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn add_unpriced_model(
    unpriced_models: &mut HashMap<String, UsageUnpricedModelRow>,
    record: &UsageRecord,
    cost: &CreditCost,
) {
    let pricing_key = normalize_model_name(&record.model);
    let row = unpriced_models
        .entry(pricing_key.clone())
        .or_insert_with(|| UsageUnpricedModelRow {
            model: record.model.clone(),
            pricing_key,
            calls: 0,
            total_tokens: 0,
            note: cost.unpriced_reason.clone(),
            pricing_stub: format_pricing_stub(&record.model),
        });

    row.calls += 1;
    row.total_tokens += record.usage.total_tokens;
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

fn read_token_usage(value: Option<&Value>) -> Option<TokenUsage> {
    let value = value?.as_object()?;
    let input = read_number(
        value
            .get("input_tokens")
            .or_else(|| value.get("inputTokens")),
    );
    let output = read_number(
        value
            .get("output_tokens")
            .or_else(|| value.get("outputTokens")),
    );
    let total = read_number(
        value
            .get("total_tokens")
            .or_else(|| value.get("totalTokens")),
    );

    if input.is_none() && output.is_none() && total.is_none() {
        return None;
    }

    Some(TokenUsage {
        input_tokens: input.unwrap_or(0),
        cached_input_tokens: read_number(
            value
                .get("cached_input_tokens")
                .or_else(|| value.get("cachedInputTokens")),
        )
        .unwrap_or(0),
        output_tokens: output.unwrap_or(0),
        reasoning_output_tokens: read_number(
            value
                .get("reasoning_output_tokens")
                .or_else(|| value.get("reasoningOutputTokens")),
        )
        .unwrap_or(0),
        total_tokens: total.unwrap_or_else(|| input.unwrap_or(0) + output.unwrap_or(0)),
    })
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

fn read_reasoning_effort(payload: &serde_json::Map<String, Value>) -> Option<String> {
    read_string(
        payload
            .get("reasoning_effort")
            .or_else(|| payload.get("reasoningEffort")),
    )
    .or_else(|| {
        read_string(
            payload
                .get("model_reasoning_effort")
                .or_else(|| payload.get("modelReasoningEffort")),
        )
    })
    .or_else(|| {
        payload
            .get("model_config")
            .or_else(|| payload.get("modelConfig"))
            .and_then(Value::as_object)
            .and_then(|model_config| {
                read_string(
                    model_config
                        .get("reasoning_effort")
                        .or_else(|| model_config.get("reasoningEffort")),
                )
            })
    })
    .or_else(|| {
        payload
            .get("reasoning")
            .and_then(Value::as_object)
            .and_then(|reasoning| {
                read_string(
                    reasoning
                        .get("effort")
                        .or_else(|| reasoning.get("reasoning_effort"))
                        .or_else(|| reasoning.get("reasoningEffort")),
                )
            })
    })
    .or_else(|| {
        payload
            .get("collaboration_mode")
            .or_else(|| payload.get("collaborationMode"))
            .and_then(Value::as_object)
            .and_then(|collaboration| collaboration.get("settings"))
            .and_then(Value::as_object)
            .and_then(|settings| {
                read_string(
                    settings
                        .get("reasoning_effort")
                        .or_else(|| settings.get("reasoningEffort")),
                )
            })
    })
}

fn read_date(value: Option<&Value>) -> Option<DateTime<Utc>> {
    match value? {
        Value::String(text) => DateTime::parse_from_rfc3339(text)
            .ok()
            .map(|date| date.with_timezone(&Utc)),
        Value::Number(number) => {
            let millis = number
                .as_i64()
                .or_else(|| number.as_f64().map(|value| value as i64))?;
            Utc.timestamp_millis_opt(millis).single()
        }
        _ => None,
    }
}

fn read_number(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|value| value as i64)),
        _ => None,
    }
}

fn read_string(value: Option<&Value>) -> Option<String> {
    let value = value?.as_str()?;
    if value.trim().is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn session_id_from_path(path: &Path) -> String {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return path.display().to_string();
    };

    if name.starts_with("rollout-") && name.ends_with(".jsonl") && name.len() > 34 {
        return name[28..name.len() - ".jsonl".len()].to_string();
    }

    path.display().to_string()
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

    Some(local_to_utc(year, month, day, hour, minute, second, 0))
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
    let local_result = Local.with_ymd_and_hms(year, month, day, hour, minute, second);
    match local_result {
        LocalResult::Single(value) => value
            .with_nanosecond(millis * 1_000_000)
            .unwrap()
            .with_timezone(&Utc),
        LocalResult::Ambiguous(earliest, _) => earliest
            .with_nanosecond(millis * 1_000_000)
            .unwrap()
            .with_timezone(&Utc),
        LocalResult::None => {
            let offset_seconds = Local::now().offset().fix().local_minus_utc();
            let offset = FixedOffset::east_opt(offset_seconds).unwrap();
            offset
                .with_ymd_and_hms(year, month, day, hour, minute, second)
                .single()
                .unwrap()
                .with_nanosecond(millis * 1_000_000)
                .unwrap()
                .with_timezone(&Utc)
        }
    }
}

fn iso_string(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn usage_warnings(range: DateRange, scan_all_files: bool) -> Vec<String> {
    if scan_all_files || is_all_usage_range(range) {
        Vec::new()
    } else {
        vec![FULL_SCAN_ACCURACY_NOTE.to_string()]
    }
}

fn is_all_usage_range(range: DateRange) -> bool {
    range.start == local_to_utc(1900, 1, 1, 0, 0, 0, 0)
        && range.end == local_to_utc(9999, 12, 31, 23, 59, 59, 999)
}

fn round_credits(value: f64) -> f64 {
    ((value + f64::EPSILON) * 1_000_000.0).round() / 1_000_000.0
}

fn credits_to_usd(credits: f64) -> f64 {
    (((credits / 25.0) + f64::EPSILON) * 1_000_000.0).round() / 1_000_000.0
}

fn invalid_input(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(io::Error::new(io::ErrorKind::InvalidInput, message.into()))
}
