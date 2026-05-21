use super::cli::ResolvedStatOptions;
use super::events::{parse_usage_json_event, UsageJsonPayload};
use crate::account_history::UsageAccountHistory;
use crate::error::AppError;
use crate::storage::path_to_string;
use crate::time::{local_to_utc, local_to_utc_checked, DateRange};
use chrono::{DateTime, Duration, Utc};
use serde_json::Value;
use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::thread;

use super::reports::{TokenUsage, UsageDiagnostics, UsageRecordView};

const DEFAULT_FILE_READ_CONCURRENCY: i64 = 8;

#[cfg(target_env = "musl")]
const DEFAULT_MAX_FILE_SCAN_THREADS: usize = 1;

#[cfg(not(target_env = "musl"))]
const DEFAULT_MAX_FILE_SCAN_THREADS: usize = 8;

const FILE_SCAN_WORKER_MIN_FILES: usize = 64;
const SESSION_READ_BUFFER_SIZE: usize = 256 * 1024;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;
const BALANCED_SCAN_MIN_LOOKBACK_MS: i64 = 7 * DAY_MS;
const BALANCED_SCAN_MAX_LOOKBACK_MS: i64 = 30 * DAY_MS;

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
    tail_candidates: Vec<TailPrefilterCandidate>,
}

struct TailPrefilterCandidate {
    path: PathBuf,
    source: TailPrefilterSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TailPrefilterSource {
    Lookback,
    Mtime,
}

struct PreparedUsageScan {
    range: DateRange,
    files: Vec<PreparedUsageFile>,
    diagnostics: UsageDiagnostics,
}

#[derive(Clone)]
struct PreparedUsageFile {
    path: PathBuf,
    current_session_id: Option<String>,
    replay_prefix_lines: usize,
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
    TailPrefilter(TailPrefilterSource),
    MtimeCheck,
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

    for file in &prepared.files {
        let scan_diagnostics = read_usage_records_from_file(
            file,
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
    let archived_listing = list_archived_jsonl_files(
        &options.sessions_dir,
        range,
        options.scan_all_files,
        &mut diagnostics,
    )?;
    let mut files = listing.files;
    files.extend(archived_listing.files);
    let mut tail_candidates = listing.tail_candidates;
    tail_candidates.extend(archived_listing.tail_candidates);
    let prefiltered_files =
        prefilter_files_by_last_usage(&tail_candidates, range.start, &mut diagnostics)?;
    files.extend(prefiltered_files);
    files.sort();
    let files = prepare_usage_files(files, &options.sessions_dir, &mut diagnostics)?;
    diagnostics.read_files = files.len() as i64;

    Ok(PreparedUsageScan {
        range,
        files,
        diagnostics,
    })
}

fn list_archived_jsonl_files(
    sessions_dir: &Path,
    range: DateRange,
    scan_all_files: bool,
    diagnostics: &mut UsageDiagnostics,
) -> Result<JsonlFileListing, AppError> {
    let Some(archived_dir) = archived_sessions_dir_for(sessions_dir) else {
        return Ok(JsonlFileListing::default());
    };
    if !archived_dir
        .try_exists()
        .map_err(|error| AppError::new(error.to_string()))?
    {
        return Ok(JsonlFileListing::default());
    }

    list_jsonl_files(
        &archived_dir,
        range,
        scan_all_files,
        Some(Vec::new()),
        diagnostics,
    )
}

fn archived_sessions_dir_for(sessions_dir: &Path) -> Option<PathBuf> {
    let parent = sessions_dir.parent()?;
    let archived_dir = parent.join("archived_sessions");
    if archived_dir == sessions_dir {
        None
    } else {
        Some(archived_dir)
    }
}

fn prepare_usage_files(
    files: Vec<PathBuf>,
    sessions_dir: &Path,
    diagnostics: &mut UsageDiagnostics,
) -> Result<Vec<PreparedUsageFile>, AppError> {
    let mut metadata = Vec::with_capacity(files.len());
    let mut session_files = HashMap::new();

    for path in files {
        let lineage = read_leading_session_meta_ids(&path)?;
        let current_session_id = lineage.first().cloned();
        if let Some(session_id) = current_session_id.as_ref() {
            session_files
                .entry(session_id.clone())
                .or_insert_with(|| path.clone());
        }
        metadata.push(ForkFileMetadata {
            path,
            lineage,
            current_session_id,
            replay_prefix_lines: 0,
        });
    }

    let lookup_roots = usage_file_lookup_roots(sessions_dir);
    let mut fingerprint_cache: HashMap<PathBuf, Vec<String>> = HashMap::new();
    let mut parent_lookup_cache: HashMap<String, Option<PathBuf>> = HashMap::new();

    for item in &mut metadata {
        let Some(parent_session_id) = item.lineage.get(1).cloned() else {
            continue;
        };

        diagnostics.fork_files += 1;
        let parent_path = match session_files.get(&parent_session_id).cloned() {
            Some(path) => Some(path),
            None => match parent_lookup_cache.get(&parent_session_id) {
                Some(path) => path.clone(),
                None => {
                    let path = find_rollout_file_by_session_id(&lookup_roots, &parent_session_id)?;
                    parent_lookup_cache.insert(parent_session_id.clone(), path.clone());
                    path
                }
            },
        };

        let Some(parent_path) = parent_path else {
            diagnostics.fork_parent_missing += 1;
            continue;
        };

        let child_path = item.path.clone();
        let parent_fingerprints =
            normalized_event_fingerprints_cached(&parent_path, &mut fingerprint_cache)?;
        let child_fingerprints =
            normalized_event_fingerprints_cached(&child_path, &mut fingerprint_cache)?;
        let replay_lines = fork_replay_prefix_lines(&child_fingerprints, &parent_fingerprints);

        if replay_lines > 0 {
            diagnostics.fork_replay_lines += replay_lines.saturating_sub(1) as i64;
            item.replay_prefix_lines = replay_lines;
        }
    }

    Ok(metadata
        .into_iter()
        .map(|metadata| PreparedUsageFile {
            path: metadata.path,
            current_session_id: if metadata.lineage.len() > 1 {
                metadata.current_session_id
            } else {
                None
            },
            replay_prefix_lines: metadata.replay_prefix_lines,
        })
        .collect())
}

struct ForkFileMetadata {
    path: PathBuf,
    lineage: Vec<String>,
    current_session_id: Option<String>,
    replay_prefix_lines: usize,
}

fn read_leading_session_meta_ids(path: &Path) -> Result<Vec<String>, AppError> {
    let file = File::open(path).map_err(|error| AppError::new(error.to_string()))?;
    let mut reader = BufReader::with_capacity(SESSION_READ_BUFFER_SIZE, file);
    let mut line = String::new();
    let mut lineage = Vec::new();

    loop {
        line.clear();
        let bytes_read = reader
            .read_line(&mut line)
            .map_err(|error| AppError::new(error.to_string()))?;
        if bytes_read == 0 {
            break;
        }

        let event = match parse_usage_json_event(&line) {
            Ok(Some(event)) => event,
            Ok(None) | Err(_) => break,
        };

        if event.event_type() != Some("session_meta") {
            break;
        }

        let Some(id) = event.payload().and_then(UsageJsonPayload::id) else {
            break;
        };
        lineage.push(id.to_string());
    }

    Ok(lineage)
}

fn usage_file_lookup_roots(sessions_dir: &Path) -> Vec<PathBuf> {
    let mut roots = vec![sessions_dir.to_path_buf()];
    if let Some(archived_dir) = archived_sessions_dir_for(sessions_dir) {
        roots.push(archived_dir);
    }
    roots
}

fn find_rollout_file_by_session_id(
    roots: &[PathBuf],
    session_id: &str,
) -> Result<Option<PathBuf>, AppError> {
    let mut matches = Vec::new();

    for root in roots {
        collect_rollout_files_by_session_id(root, session_id, &mut matches)?;
    }

    matches.sort();
    Ok(matches.into_iter().next())
}

fn collect_rollout_files_by_session_id(
    root: &Path,
    session_id: &str,
    matches: &mut Vec<PathBuf>,
) -> Result<(), AppError> {
    let mut entries = match fs::read_dir(root) {
        Ok(entries) => entries
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| AppError::new(error.to_string()))?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(AppError::new(error.to_string())),
    };
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| AppError::new(error.to_string()))?;
        if file_type.is_dir() {
            collect_rollout_files_by_session_id(&path, session_id, matches)?;
        } else if file_type.is_file()
            && path.extension().and_then(|value| value.to_str()) == Some("jsonl")
            && path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.starts_with("rollout-") && name.contains(session_id))
        {
            matches.push(path);
        }
    }

    Ok(())
}

fn normalized_event_fingerprints_cached(
    path: &Path,
    cache: &mut HashMap<PathBuf, Vec<String>>,
) -> Result<Vec<String>, AppError> {
    if !cache.contains_key(path) {
        let fingerprints = read_normalized_event_fingerprints(path)?;
        cache.insert(path.to_path_buf(), fingerprints);
    }

    cache
        .get(path)
        .cloned()
        .ok_or_else(|| AppError::new("Failed to cache fork event fingerprints."))
}

fn read_normalized_event_fingerprints(path: &Path) -> Result<Vec<String>, AppError> {
    let file = File::open(path).map_err(|error| AppError::new(error.to_string()))?;
    let mut reader = BufReader::with_capacity(SESSION_READ_BUFFER_SIZE, file);
    let mut line = String::new();
    let mut fingerprints = Vec::new();

    loop {
        line.clear();
        let bytes_read = reader
            .read_line(&mut line)
            .map_err(|error| AppError::new(error.to_string()))?;
        if bytes_read == 0 {
            break;
        }

        fingerprints.push(normalized_event_fingerprint(&line));
    }

    Ok(fingerprints)
}

fn normalized_event_fingerprint(line: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<Value>(line) else {
        return format!("invalid-json:{}", line.len());
    };

    if let Value::Object(fields) = &mut value {
        fields.remove("timestamp");
    }

    canonical_json(&value)
}

fn canonical_json(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string()),
        Value::Array(values) => {
            let values = values.iter().map(canonical_json).collect::<Vec<_>>();
            format!("[{}]", values.join(","))
        }
        Value::Object(fields) => {
            let mut entries = fields.iter().collect::<Vec<_>>();
            entries.sort_by(|left, right| left.0.cmp(right.0));
            let entries = entries
                .into_iter()
                .map(|(key, value)| {
                    let key = serde_json::to_string(key).unwrap_or_else(|_| "\"\"".to_string());
                    format!("{key}:{}", canonical_json(value))
                })
                .collect::<Vec<_>>();
            format!("{{{}}}", entries.join(","))
        }
    }
}

fn fork_replay_prefix_lines(child: &[String], parent: &[String]) -> usize {
    if child.len() <= 1 || parent.is_empty() {
        return 0;
    }

    let mut matched = 0;
    for (child_fingerprint, parent_fingerprint) in child[1..].iter().zip(parent) {
        if child_fingerprint != parent_fingerprint {
            break;
        }
        matched += 1;
    }

    if matched == 0 {
        0
    } else {
        matched + 1
    }
}

fn scan_usage_files_into_accumulator<A>(
    files: &[PreparedUsageFile],
    range: DateRange,
    account_history: Option<&UsageAccountHistory>,
    account_id: Option<&str>,
    accumulator: &mut A,
) -> Result<UsageDiagnostics, AppError>
where
    A: UsageRecordAccumulator,
{
    let mut diagnostics = UsageDiagnostics::new(0, false);

    for file in files {
        let mut sink = AccumulatorRecordSink {
            accumulator: &mut *accumulator,
        };
        let scan_diagnostics =
            read_usage_records_from_file(file, range, account_history, account_id, &mut sink)?;
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

pub(super) fn partition_files_for_workers<T: Clone>(
    files: &[T],
    worker_count: usize,
) -> Vec<Vec<T>> {
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
                .tail_candidates
                .extend(child_listing.tail_candidates);
        } else if file_type.is_file()
            && path.extension().and_then(|value| value.to_str()) == Some("jsonl")
        {
            match classify_jsonl_file(&path, policy) {
                JsonlFileAction::Read => listing.files.push(path),
                JsonlFileAction::TailPrefilter(source) => listing
                    .tail_candidates
                    .push(TailPrefilterCandidate { path, source }),
                JsonlFileAction::MtimeCheck => {
                    diagnostics.mtime_read_files += 1;
                    if file_modified_at_or_after(&path, policy.start)? {
                        diagnostics.mtime_tail_hits += 1;
                        listing.tail_candidates.push(TailPrefilterCandidate {
                            path,
                            source: TailPrefilterSource::Mtime,
                        });
                    } else {
                        diagnostics.skipped_files += 1;
                    }
                }
                JsonlFileAction::Skip => diagnostics.skipped_files += 1,
            }
        }
    }

    listing.files.sort();
    listing
        .tail_candidates
        .sort_by(|left, right| left.path.cmp(&right.path));
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
    let Some((start, _end)) = date_path_range(parts) else {
        return false;
    };

    start > policy.end
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
        return JsonlFileAction::TailPrefilter(TailPrefilterSource::Lookback);
    }

    JsonlFileAction::MtimeCheck
}

fn file_modified_at_or_after(path: &Path, start: DateTime<Utc>) -> Result<bool, AppError> {
    let modified = fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .map_err(|error| AppError::new(error.to_string()))?;
    let modified_at = DateTime::<Utc>::from(modified);

    Ok(modified_at >= start)
}

fn prefilter_files_by_last_usage(
    files: &[TailPrefilterCandidate],
    start: DateTime<Utc>,
    diagnostics: &mut UsageDiagnostics,
) -> Result<Vec<PathBuf>, AppError> {
    let mut kept = Vec::new();

    for candidate in files {
        diagnostics.tail_read_files += 1;
        let last_usage_at = read_last_token_count_timestamp(&candidate.path)?;

        if last_usage_at.is_some_and(|timestamp| timestamp < start) {
            diagnostics.prefiltered_files += 1;
        } else {
            diagnostics.tail_read_hits += 1;
            if candidate.source == TailPrefilterSource::Mtime {
                diagnostics.mtime_read_hits += 1;
            }
            kept.push(candidate.path.clone());
        }
    }

    Ok(kept)
}

fn read_last_token_count_timestamp(path: &Path) -> Result<Option<DateTime<Utc>>, AppError> {
    let mut file = File::open(path).map_err(|error| AppError::new(error.to_string()))?;
    let mut position = file
        .seek(SeekFrom::End(0))
        .map_err(|error| AppError::new(error.to_string()))?;
    let mut buffer = vec![0_u8; SESSION_READ_BUFFER_SIZE];
    let mut carry = Vec::new();

    while position > 0 {
        let read_len = (position as usize).min(buffer.len());
        position -= read_len as u64;
        file.seek(SeekFrom::Start(position))
            .map_err(|error| AppError::new(error.to_string()))?;
        file.read_exact(&mut buffer[..read_len])
            .map_err(|error| AppError::new(error.to_string()))?;

        let mut combined = Vec::with_capacity(read_len + carry.len());
        combined.extend_from_slice(&buffer[..read_len]);
        combined.extend_from_slice(&carry);

        if position > 0 {
            let Some(newline_index) = combined.iter().position(|byte| *byte == b'\n') else {
                carry = combined;
                continue;
            };

            if let Some(timestamp) = last_token_count_timestamp_in_lines(
                combined
                    .get(newline_index + 1..)
                    .expect("newline index is within combined bytes"),
            ) {
                return Ok(Some(timestamp));
            }

            carry.clear();
            carry.extend_from_slice(
                combined
                    .get(..newline_index)
                    .expect("newline index is within combined bytes"),
            );
        } else if let Some(timestamp) = last_token_count_timestamp_in_lines(&combined) {
            return Ok(Some(timestamp));
        }
    }

    Ok(None)
}

fn last_token_count_timestamp_in_lines(bytes: &[u8]) -> Option<DateTime<Utc>> {
    for line in bytes.split(|byte| *byte == b'\n').rev() {
        let line = trim_line_end_bytes(line);
        if line.is_empty() || !line_contains_bytes(line, b"\"token_count\"") {
            continue;
        };

        let Ok(line) = std::str::from_utf8(line) else {
            continue;
        };
        let Ok(event) = parse_usage_json_event(line) else {
            continue;
        };
        let Some(event) = event else {
            continue;
        };
        if event.event_type() == Some("event_msg")
            && event.payload().and_then(UsageJsonPayload::payload_type) == Some("token_count")
        {
            if let Some(timestamp) = event.timestamp() {
                return Some(timestamp);
            }
        }
    }

    None
}

fn trim_line_end_bytes(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\r").unwrap_or(line)
}

fn line_contains_bytes(line: &[u8], needle: &[u8]) -> bool {
    line.windows(needle.len()).any(|window| window == needle)
}

fn read_usage_records_from_file<F>(
    usage_file: &PreparedUsageFile,
    range: DateRange,
    account_history: Option<&UsageAccountHistory>,
    account_id_filter: Option<&str>,
    on_record: &mut F,
) -> Result<UsageDiagnostics, AppError>
where
    F: UsageRecordSink + ?Sized,
{
    let path = &usage_file.path;
    let file_handle = File::open(path).map_err(|error| AppError::new(error.to_string()))?;
    let mut reader = BufReader::with_capacity(SESSION_READ_BUFFER_SIZE, file_handle);
    let mut line = String::new();
    let mut diagnostics = UsageDiagnostics::new(0, false);
    let mut session_id = usage_file
        .current_session_id
        .clone()
        .unwrap_or_else(|| session_id_from_path(path));
    let mut model = String::from("unknown");
    let mut reasoning_effort: Option<String> = None;
    let mut cwd = String::from("unknown");
    let mut previous_total: Option<TokenUsage> = None;
    let file_path = path_to_string(path);
    let mut line_number = 0_usize;

    loop {
        line.clear();
        let bytes_read = reader
            .read_line(&mut line)
            .map_err(|error| AppError::new(error.to_string()))?;
        if bytes_read == 0 {
            break;
        }

        line_number += 1;
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
        let is_fork_replay_line = usage_file.replay_prefix_lines > 0
            && line_number > 1
            && line_number <= usage_file.replay_prefix_lines;
        if event_type == Some("session_meta") {
            if is_fork_replay_line {
                continue;
            }
            if let Some(payload) = event.payload() {
                if usage_file.current_session_id.is_none() {
                    if let Some(id) = payload.id() {
                        session_id = id.to_string();
                    }
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
            if is_fork_replay_line {
                continue;
            }
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

        if is_fork_replay_line {
            if let Some(info) = info {
                if let Some(total_usage) = info.total_token_usage() {
                    previous_total = Some(total_usage);
                }
            }
            diagnostics.skipped_events.fork_replay += 1;
            continue;
        }

        let Some(info) = info else {
            diagnostics.skipped_events.missing_metadata += 1;
            continue;
        };

        let total_usage = info.total_token_usage();
        let Some(timestamp) = timestamp else {
            if let Some(total_usage) = total_usage {
                previous_total = Some(total_usage);
            }
            diagnostics.skipped_events.missing_metadata += 1;
            continue;
        };

        let usage = diff_usage(total_usage.as_ref(), previous_total.as_ref())
            .or_else(|| info.last_token_usage());

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
    use crate::stats::{read_usage_records_report, UsageRecord, UsageRecordsReadOptions};
    use chrono::TimeZone;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, MutexGuard};
    use tempfile::TempDir;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn env_lock() -> MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner())
    }

    #[test]
    fn balanced_scan_lookback_clamps_between_seven_and_thirty_days() {
        let start = utc_time(2026, 5, 21, 0);

        let short_policy = JsonlScanPolicy::new(
            DateRange {
                start,
                end: start + Duration::days(1),
            },
            false,
        );
        assert_eq!(short_policy.lookback_start, start - Duration::days(7));

        let long_policy = JsonlScanPolicy::new(
            DateRange {
                start,
                end: start + Duration::days(120),
            },
            false,
        );
        assert_eq!(long_policy.lookback_start, start - Duration::days(30));
    }

    #[test]
    fn usage_scan_includes_flat_archived_sessions_sibling() {
        let temp = TempDir::new().expect("tempdir");
        let codex_home = temp.path().join("codex-home");
        let sessions_dir = codex_home.join("sessions");
        let archived_dir = codex_home.join("archived_sessions");
        std::fs::create_dir_all(&sessions_dir).expect("create sessions dir");
        let archived_file = write_flat_session_file(
            &archived_dir,
            "rollout-2026-05-21T01-00-00-archived-session.jsonl",
            &[token_count_line("2026-05-21T01:00:01.000Z")],
        );

        let report = read_usage_records_report(&UsageRecordsReadOptions {
            start: utc_time(2026, 5, 21, 0),
            end: utc_time(2026, 5, 21, 2),
            sessions_dir,
            scan_all_files: false,
            account_history_file: None,
            account_id: None,
        })
        .expect("read usage report");

        assert_eq!(report.records.len(), 1);
        assert_eq!(report.records[0].file_path, path_to_string(archived_file));
        assert_eq!(report.records[0].usage.total_tokens, 2);
        assert_eq!(report.diagnostics.read_files, 1);
    }

    #[test]
    fn account_filter_without_history_does_not_include_unattributed_usage() {
        let temp = TempDir::new().expect("tempdir");
        let sessions_dir = temp.path().join("sessions");
        write_session_file(
            &sessions_dir,
            2026,
            5,
            21,
            "rollout-2026-05-21T00-00-00-account-filter.jsonl",
            &[token_count_line("2026-05-21T00:00:01.000Z")],
        );

        let report = read_usage_records_report(&UsageRecordsReadOptions {
            start: utc_time(2026, 5, 21, 0),
            end: utc_time(2026, 5, 21, 1),
            sessions_dir,
            scan_all_files: false,
            account_history_file: Some(temp.path().join("missing-history.json")),
            account_id: Some("account-fixture".to_string()),
        })
        .expect("read usage report");

        assert!(report.records.is_empty());
        assert_eq!(report.diagnostics.included_usage_events, 0);
        assert_eq!(report.diagnostics.skipped_events.account_mismatch, 1);
    }

    #[test]
    fn old_files_before_lookback_use_mtime_before_tail_prefilter() {
        let temp = TempDir::new().expect("tempdir");
        let start = utc_time(2000, 1, 1, 0);
        let end = utc_time(2000, 1, 2, 0);
        let file = write_session_file(
            temp.path(),
            1999,
            1,
            1,
            "rollout-1999-01-01T00-00-00-active.jsonl",
            &[token_count_line("2000-01-01T00:01:00.000Z")],
        );
        let mut diagnostics = UsageDiagnostics::new(0, false);

        let listing = list_jsonl_files(
            temp.path(),
            DateRange { start, end },
            false,
            Some(Vec::new()),
            &mut diagnostics,
        )
        .expect("list files");

        assert!(listing.files.is_empty());
        assert_eq!(listing.tail_candidates.len(), 1);
        assert_eq!(listing.tail_candidates[0].path, file);
        assert_eq!(
            listing.tail_candidates[0].source,
            TailPrefilterSource::Mtime
        );
        assert_eq!(diagnostics.mtime_read_files, 1);
        assert_eq!(diagnostics.mtime_tail_hits, 1);
        assert_eq!(diagnostics.skipped_directories, 0);
    }

    #[test]
    fn old_files_before_lookback_with_old_mtime_are_skipped() {
        let temp = TempDir::new().expect("tempdir");
        let start = utc_time(2999, 1, 1, 0);
        let end = utc_time(2999, 1, 2, 0);
        write_session_file(
            temp.path(),
            2020,
            1,
            1,
            "rollout-2020-01-01T00-00-00-inactive.jsonl",
            &[token_count_line("2999-01-01T00:01:00.000Z")],
        );
        let mut diagnostics = UsageDiagnostics::new(0, false);

        let listing = list_jsonl_files(
            temp.path(),
            DateRange { start, end },
            false,
            Some(Vec::new()),
            &mut diagnostics,
        )
        .expect("list files");

        assert!(listing.files.is_empty());
        assert!(listing.tail_candidates.is_empty());
        assert_eq!(diagnostics.mtime_read_files, 1);
        assert_eq!(diagnostics.mtime_tail_hits, 0);
        assert_eq!(diagnostics.skipped_files, 1);
        assert_eq!(diagnostics.skipped_directories, 0);
    }

    #[test]
    fn tail_prefilter_tracks_hits_and_mtime_final_hits() {
        let temp = TempDir::new().expect("tempdir");
        let start = utc_time(2026, 5, 21, 0);
        let stale = write_session_file(
            temp.path(),
            2026,
            5,
            20,
            "rollout-2026-05-20T00-00-00-stale.jsonl",
            &[token_count_line("2026-05-20T23:59:59.000Z")],
        );
        let active = write_session_file(
            temp.path(),
            2026,
            5,
            19,
            "rollout-2026-05-19T00-00-00-active.jsonl",
            &[token_count_line("2026-05-21T00:00:01.000Z")],
        );
        let unknown = write_session_file(
            temp.path(),
            2026,
            5,
            18,
            "rollout-2026-05-18T00-00-00-unknown.jsonl",
            &["{\"type\":\"session_meta\"}".to_string()],
        );
        let candidates = vec![
            TailPrefilterCandidate {
                path: stale,
                source: TailPrefilterSource::Lookback,
            },
            TailPrefilterCandidate {
                path: active.clone(),
                source: TailPrefilterSource::Mtime,
            },
            TailPrefilterCandidate {
                path: unknown.clone(),
                source: TailPrefilterSource::Lookback,
            },
        ];
        let mut diagnostics = UsageDiagnostics::new(0, false);

        let kept = prefilter_files_by_last_usage(&candidates, start, &mut diagnostics)
            .expect("prefilter files");

        assert_eq!(kept, vec![active, unknown]);
        assert_eq!(diagnostics.tail_read_files, 3);
        assert_eq!(diagnostics.tail_read_hits, 2);
        assert_eq!(diagnostics.prefiltered_files, 1);
        assert_eq!(diagnostics.mtime_read_hits, 1);
    }

    #[test]
    fn usage_scan_deduplicates_recursive_and_sibling_fork_replay() {
        let temp = TempDir::new().expect("tempdir");
        let sessions_dir = temp.path().join("sessions");
        let session_a = "019e48e7-545e-7253-a8aa-cbd9fec62ce3";
        let session_b = "019e48e8-13cb-7242-9e25-744399563084";
        let session_c = "019e48ee-61c3-7f52-9a56-97e7543a0fdc";
        let session_d = "019e48ee-sibling-7253-a8aa-cbd9fec62ce3";

        let session_a_lines = vec![
            session_meta_line("2026-05-21T00:00:00.000Z", session_a),
            token_count_total_line("2026-05-21T00:00:01.000Z", 10, 10),
            token_count_total_line("2026-05-21T00:00:02.000Z", 10, 20),
        ];
        let mut session_b_lines = vec![session_meta_line("2026-05-21T00:10:00.000Z", session_b)];
        session_b_lines.extend(retimestamp_lines(
            &session_a_lines,
            "2026-05-21T00:10:00.001Z",
        ));
        session_b_lines.push(token_count_total_line("2026-05-21T00:10:01.000Z", 20, 20));
        session_b_lines.push(token_count_total_line("2026-05-21T00:10:02.000Z", 15, 35));

        let mut session_c_lines = vec![session_meta_line("2026-05-21T00:20:00.000Z", session_c)];
        session_c_lines.extend(retimestamp_lines(
            &session_b_lines,
            "2026-05-21T00:20:00.001Z",
        ));
        session_c_lines.push(token_count_total_line("2026-05-21T00:20:01.000Z", 35, 35));
        session_c_lines.push(token_count_total_line("2026-05-21T00:20:02.000Z", 7, 42));

        let mut session_d_lines = vec![session_meta_line("2026-05-21T00:30:00.000Z", session_d)];
        session_d_lines.extend(retimestamp_lines(
            &session_b_lines,
            "2026-05-21T00:30:00.001Z",
        ));
        session_d_lines.push(token_count_total_line("2026-05-21T00:30:01.000Z", 35, 35));
        session_d_lines.push(token_count_total_line("2026-05-21T00:30:02.000Z", 4, 39));

        write_session_file(
            &sessions_dir,
            2026,
            5,
            21,
            &format!("rollout-2026-05-21T00-00-00-{session_a}.jsonl"),
            &session_a_lines,
        );
        write_session_file(
            &sessions_dir,
            2026,
            5,
            21,
            &format!("rollout-2026-05-21T00-10-00-{session_b}.jsonl"),
            &session_b_lines,
        );
        write_session_file(
            &sessions_dir,
            2026,
            5,
            21,
            &format!("rollout-2026-05-21T00-20-00-{session_c}.jsonl"),
            &session_c_lines,
        );
        write_session_file(
            &sessions_dir,
            2026,
            5,
            21,
            &format!("rollout-2026-05-21T00-30-00-{session_d}.jsonl"),
            &session_d_lines,
        );

        let report = read_usage_records_report(&UsageRecordsReadOptions {
            start: utc_time(2026, 5, 21, 0),
            end: utc_time(2026, 5, 21, 1),
            sessions_dir,
            scan_all_files: false,
            account_history_file: None,
            account_id: None,
        })
        .expect("read usage report");

        assert_eq!(usage_total_for_session(&report.records, session_a), 20);
        assert_eq!(usage_total_for_session(&report.records, session_b), 15);
        assert_eq!(usage_total_for_session(&report.records, session_c), 7);
        assert_eq!(usage_total_for_session(&report.records, session_d), 4);
        assert_eq!(
            report
                .records
                .iter()
                .map(|record| record.usage.total_tokens)
                .sum::<i64>(),
            46
        );
        assert_eq!(report.diagnostics.fork_files, 3);
        assert_eq!(report.diagnostics.fork_parent_missing, 0);
        assert_eq!(report.diagnostics.fork_replay_lines, 15);
        assert_eq!(report.diagnostics.skipped_events.fork_replay, 10);
        assert_eq!(report.diagnostics.skipped_events.empty_usage, 3);
    }

    #[test]
    fn usage_scan_advances_total_for_timestampless_token_count() {
        let temp = TempDir::new().expect("tempdir");
        let sessions_dir = temp.path().join("sessions");
        let session_id = "019e50f5-0e02-78c7-a834-df3ad725e25f";

        write_session_file(
            &sessions_dir,
            2026,
            5,
            21,
            &format!("rollout-2026-05-21T00-00-00-{session_id}.jsonl"),
            &[
                session_meta_line("2026-05-21T00:00:00.000Z", session_id),
                token_count_total_line("2026-05-21T00:00:01.000Z", 10, 10),
                token_count_total_line_without_timestamp(20, 30),
                token_count_total_line("2026-05-21T00:00:03.000Z", 15, 45),
            ],
        );

        let report = read_usage_records_report(&UsageRecordsReadOptions {
            start: utc_time(2026, 5, 21, 0),
            end: utc_time(2026, 5, 21, 1),
            sessions_dir,
            scan_all_files: false,
            account_history_file: None,
            account_id: None,
        })
        .expect("read usage report");

        let session_usage = report
            .records
            .iter()
            .filter(|record| record.session_id == session_id)
            .map(|record| record.usage.total_tokens)
            .collect::<Vec<_>>();

        assert_eq!(session_usage, vec![10, 15]);
        assert_eq!(usage_total_for_session(&report.records, session_id), 25);
        assert_eq!(report.diagnostics.skipped_events.missing_metadata, 1);
    }

    #[test]
    fn usage_scan_ignores_fork_replayed_metadata() {
        let temp = TempDir::new().expect("tempdir");
        let sessions_dir = temp.path().join("sessions");
        let parent_session = "019e5110-1f92-7280-b02a-0876af32b81f";
        let child_session = "019e5111-2720-73d0-8519-4c80dffbe80e";

        let parent_lines = vec![
            session_meta_line_with_metadata(
                "2026-05-21T00:00:00.000Z",
                parent_session,
                "parent-session-model",
                "/parent-session",
            ),
            turn_context_line(
                "2026-05-21T00:00:00.500Z",
                "parent-context-model",
                "/parent-context",
            ),
            token_count_total_line("2026-05-21T00:00:01.000Z", 10, 10),
        ];
        let mut child_lines = vec![session_meta_line_with_metadata(
            "2026-05-21T00:10:00.000Z",
            child_session,
            "child-model",
            "/child",
        )];
        child_lines.extend(retimestamp_lines(&parent_lines, "2026-05-21T00:10:00.001Z"));
        child_lines.push(token_count_total_line("2026-05-21T00:10:01.000Z", 15, 25));

        write_session_file(
            &sessions_dir,
            2026,
            5,
            21,
            &format!("rollout-2026-05-21T00-00-00-{parent_session}.jsonl"),
            &parent_lines,
        );
        write_session_file(
            &sessions_dir,
            2026,
            5,
            21,
            &format!("rollout-2026-05-21T00-10-00-{child_session}.jsonl"),
            &child_lines,
        );

        let report = read_usage_records_report(&UsageRecordsReadOptions {
            start: utc_time(2026, 5, 21, 0),
            end: utc_time(2026, 5, 21, 1),
            sessions_dir,
            scan_all_files: false,
            account_history_file: None,
            account_id: None,
        })
        .expect("read usage report");

        let child_records = report
            .records
            .iter()
            .filter(|record| record.session_id == child_session)
            .collect::<Vec<_>>();

        assert_eq!(child_records.len(), 1);
        assert_eq!(child_records[0].usage.total_tokens, 15);
        assert_eq!(child_records[0].model, "child-model");
        assert_eq!(child_records[0].cwd, "/child");
        assert_eq!(report.diagnostics.skipped_events.fork_replay, 1);
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
        assert!(partition_files_for_workers::<PathBuf>(&[], 8).is_empty());
        assert_eq!(partition_files_for_workers(&files[..2], 8).len(), 2);
    }

    #[test]
    fn default_file_scan_worker_limit_matches_target_env() {
        if cfg!(target_env = "musl") {
            assert_eq!(DEFAULT_MAX_FILE_SCAN_THREADS, 1);
        } else {
            assert_eq!(DEFAULT_MAX_FILE_SCAN_THREADS, 8);
        }
    }

    #[test]
    fn file_scan_worker_count_respects_small_file_sets() {
        let _guard = env_lock();
        env::remove_var("CODEX_OPS_STAT_WORKERS");

        assert_eq!(resolve_file_scan_worker_count(0).unwrap(), 1);
        assert_eq!(resolve_file_scan_worker_count(1).unwrap(), 1);
        assert_eq!(
            resolve_file_scan_worker_count(FILE_SCAN_WORKER_MIN_FILES - 1).unwrap(),
            1
        );
    }

    #[test]
    fn file_scan_worker_count_respects_env_override() {
        let _guard = env_lock();
        env::set_var("CODEX_OPS_STAT_WORKERS", "4");

        assert_eq!(resolve_file_scan_worker_count(100).unwrap(), 4);
        assert_eq!(resolve_file_scan_worker_count(2).unwrap(), 2);

        env::set_var("CODEX_OPS_STAT_WORKERS", "0");
        assert_eq!(resolve_file_scan_worker_count(100).unwrap(), 1);

        env::remove_var("CODEX_OPS_STAT_WORKERS");
    }

    fn write_session_file(
        root: &Path,
        year: i32,
        month: u32,
        day: u32,
        file_name: &str,
        lines: &[String],
    ) -> PathBuf {
        let dir = root
            .join(format!("{year:04}"))
            .join(format!("{month:02}"))
            .join(format!("{day:02}"));
        std::fs::create_dir_all(&dir).expect("create session dir");
        let path = dir.join(file_name);
        let mut file = std::fs::File::create(&path).expect("create session file");
        for line in lines {
            writeln!(file, "{line}").expect("write session line");
        }
        path
    }

    fn write_flat_session_file(root: &Path, file_name: &str, lines: &[String]) -> PathBuf {
        std::fs::create_dir_all(root).expect("create session dir");
        let path = root.join(file_name);
        let mut file = std::fs::File::create(&path).expect("create session file");
        for line in lines {
            writeln!(file, "{line}").expect("write session line");
        }
        path
    }

    fn usage_total_for_session(records: &[UsageRecord], session_id: &str) -> i64 {
        records
            .iter()
            .filter(|record| record.session_id == session_id)
            .map(|record| record.usage.total_tokens)
            .sum()
    }

    fn session_meta_line(timestamp: &str, session_id: &str) -> String {
        session_meta_line_with_metadata(timestamp, session_id, "gpt-5.5", "/workspace/fork-test")
    }

    fn session_meta_line_with_metadata(
        timestamp: &str,
        session_id: &str,
        model: &str,
        cwd: &str,
    ) -> String {
        serde_json::json!({
            "timestamp": timestamp,
            "type": "session_meta",
            "payload": {
                "id": session_id,
                "model": model,
                "cwd": cwd
            }
        })
        .to_string()
    }

    fn turn_context_line(timestamp: &str, model: &str, cwd: &str) -> String {
        serde_json::json!({
            "timestamp": timestamp,
            "type": "turn_context",
            "payload": {
                "model": model,
                "cwd": cwd
            }
        })
        .to_string()
    }

    fn token_count_line(timestamp: &str) -> String {
        format!(
            r#"{{"timestamp":"{timestamp}","type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1,"reasoning_output_tokens":0,"total_tokens":2}}}}}}}}"#
        )
    }

    fn token_count_total_line(timestamp: &str, last_total: i64, total: i64) -> String {
        serde_json::json!({
            "timestamp": timestamp,
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": {
                    "last_token_usage": {
                        "input_tokens": last_total,
                        "cached_input_tokens": 0,
                        "output_tokens": 0,
                        "reasoning_output_tokens": 0,
                        "total_tokens": last_total
                    },
                    "total_token_usage": {
                        "input_tokens": total,
                        "cached_input_tokens": 0,
                        "output_tokens": 0,
                        "reasoning_output_tokens": 0,
                        "total_tokens": total
                    }
                }
            }
        })
        .to_string()
    }

    fn token_count_total_line_without_timestamp(last_total: i64, total: i64) -> String {
        serde_json::json!({
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": {
                    "last_token_usage": {
                        "input_tokens": last_total,
                        "cached_input_tokens": 0,
                        "output_tokens": 0,
                        "reasoning_output_tokens": 0,
                        "total_tokens": last_total
                    },
                    "total_token_usage": {
                        "input_tokens": total,
                        "cached_input_tokens": 0,
                        "output_tokens": 0,
                        "reasoning_output_tokens": 0,
                        "total_tokens": total
                    }
                }
            }
        })
        .to_string()
    }

    fn retimestamp_lines(lines: &[String], timestamp: &str) -> Vec<String> {
        lines
            .iter()
            .map(|line| {
                let mut value = serde_json::from_str::<Value>(line).expect("json line");
                if let Value::Object(fields) = &mut value {
                    fields.insert(
                        "timestamp".to_string(),
                        Value::String(timestamp.to_string()),
                    );
                }
                serde_json::to_string(&value).expect("json string")
            })
            .collect()
    }

    fn utc_time(year: i32, month: u32, day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, 0, 0)
            .single()
            .expect("utc time")
    }
}
