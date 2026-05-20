use super::cli::ResolvedStatOptions;
use super::events::{parse_usage_json_event, UsageJsonPayload};
use crate::account_history::UsageAccountHistory;
use crate::error::AppError;
use crate::storage::path_to_string;
use crate::time::{local_to_utc, local_to_utc_checked, DateRange};
use chrono::{DateTime, Duration, Utc};
use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::thread;

use super::reports::{TokenUsage, UsageDiagnostics, UsageRecordView};

const DEFAULT_FILE_READ_CONCURRENCY: i64 = 8;
const DEFAULT_MAX_FILE_SCAN_THREADS: usize = 8;
const FILE_SCAN_WORKER_MIN_FILES: usize = 64;
const SESSION_READ_BUFFER_SIZE: usize = 256 * 1024;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;
const BALANCED_SCAN_MIN_LOOKBACK_MS: i64 = 2 * DAY_MS;
const BALANCED_SCAN_MAX_LOOKBACK_MS: i64 = 7 * DAY_MS;

pub(super) trait UsageRecordAccumulator: Send {
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
        let lookback_ms =
            (duration_ms / 2).clamp(BALANCED_SCAN_MIN_LOOKBACK_MS, BALANCED_SCAN_MAX_LOOKBACK_MS);

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

pub(super) fn process_usage_records<F>(
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

pub(super) fn process_usage_records_parallel<A>(
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

pub(super) fn partition_files_for_workers(
    files: &[PathBuf],
    worker_count: usize,
) -> Vec<Vec<PathBuf>> {
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
            return Ok(JsonlFileListing::default());
        }
        Err(error) => return Err(AppError::new(error.to_string())),
    };
    entries.sort_by_key(|entry| entry.file_name());

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

fn resolve_usage_account_id(
    timestamp: DateTime<Utc>,
    history: Option<&UsageAccountHistory>,
) -> Option<String> {
    history.and_then(|history| history.account_id_at(timestamp))
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
