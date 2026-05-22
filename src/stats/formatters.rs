use super::reports::{
    format_date_time, format_group_by, format_report_range, to_limit_usage_json,
    to_usage_session_detail_json, to_usage_sessions_json, to_usage_stats_json, usage_warnings,
    LimitUsageReport, LimitUsageRow, TokenUsage, UsageDiagnostics, UsageSessionCompactRow,
    UsageSessionDetailReport, UsageSessionEventRow, UsageSessionRow, UsageSessionsReport,
    UsageStatRow, UsageStatsReport, UsageUnpricedModelRow,
};
use super::StatFormat;
use crate::error::AppError;
use crate::format::{
    credits_to_usd, format_credits, format_csv, format_integer, format_markdown_table,
    format_plain_table, format_usd, round_credits, to_pretty_json,
};
use chrono::{DateTime, Utc};

const DEFAULT_SESSION_DETAIL_COMPACT_ROWS: usize = 20;

pub(super) fn format_usage_stats(
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

pub(super) fn format_limit_usage(
    report: &LimitUsageReport,
    format: StatFormat,
    verbose: bool,
) -> Result<String, AppError> {
    if format == StatFormat::Json {
        return Ok(format!(
            "{}\n",
            to_pretty_json(&to_limit_usage_json(report))
                .map_err(|error| AppError::new(error.to_string()))?
        ));
    }

    let mut rows = vec![limit_usage_headers()];
    rows.extend(report.rows.iter().map(limit_usage_row));
    rows.push(limit_usage_total_row(&report.totals));

    if format == StatFormat::Csv {
        return Ok(format!("{}\n", format_csv(&rows)));
    }

    if format == StatFormat::Markdown {
        let mut lines = vec![format_markdown_table(&rows)];
        append_limit_usage_notes(&mut lines, report, verbose);
        return Ok(format!("{}\n", lines.join("\n")));
    }

    let mut lines = vec![
        "Codex usage by rate-limit window".to_string(),
        format!("Range: {}", format_report_range(report.start, report.end)),
        format!("Limit window: {}", report.limit_window),
        format!("Grouped by: {}", report.group_by.as_str()),
        format!("Sessions dir: {}", report.sessions_dir),
        String::new(),
    ];

    if report.rows.is_empty() {
        lines.push("No token usage records found in this range.".to_string());
        append_limit_usage_notes(&mut lines, report, verbose);
        return Ok(lines.join("\n"));
    }

    lines.push(format_plain_table(&rows));
    append_limit_usage_notes(&mut lines, report, verbose);
    Ok(lines.join("\n"))
}

pub(super) fn format_usage_sessions(
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

pub(super) fn format_usage_session_detail(
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
            lines.push("Pricing stubs for data/codex-rate-card.json:".to_string());
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
                "  Full files read: {}",
                format_integer(diagnostics.read_files)
            ));
            lines.push(format!(
                "  Files skipped by date/mtime: {}",
                format_integer(diagnostics.skipped_files)
            ));
            lines.push(format!(
                "  Files skipped by tail prefilter: {}",
                format_integer(diagnostics.prefiltered_files)
            ));
            lines.push(format!(
                "  Tail token reads: {}",
                format_integer(diagnostics.tail_read_files)
            ));
            lines.push(format!(
                "  Tail token read hits: {}",
                format_integer(diagnostics.tail_read_hits)
            ));
            lines.push(format!(
                "  File mtimes read: {}",
                format_integer(diagnostics.mtime_read_files)
            ));
            lines.push(format!(
                "  File mtime hits requiring tail read: {}",
                format_integer(diagnostics.mtime_tail_hits)
            ));
            lines.push(format!(
                "  File mtime hits requiring full read: {}",
                format_integer(diagnostics.mtime_read_hits)
            ));
            lines.push(format!(
                "  Fork files detected: {}",
                format_integer(diagnostics.fork_files)
            ));
            lines.push(format!(
                "  Fork parent files missing: {}",
                format_integer(diagnostics.fork_parent_missing)
            ));
            lines.push(format!(
                "  Fork replay lines skipped: {}",
                format_integer(diagnostics.fork_replay_lines)
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
                "  Skipped events: missing metadata {}, missing usage {}, empty usage {}, out of range {}, account mismatch {}, fork replay {}",
                format_integer(diagnostics.skipped_events.missing_metadata),
                format_integer(diagnostics.skipped_events.missing_usage),
                format_integer(diagnostics.skipped_events.empty_usage),
                format_integer(diagnostics.skipped_events.out_of_range),
                format_integer(diagnostics.skipped_events.account_mismatch),
                format_integer(diagnostics.skipped_events.fork_replay)
            ));
        }
    }

    let warnings = usage_warnings(report.start(), report.end(), report.diagnostics());
    if !warnings.is_empty() {
        lines.push(String::new());
        lines.extend(warnings);
    }
}

fn append_limit_usage_notes(lines: &mut Vec<String>, report: &LimitUsageReport, verbose: bool) {
    append_unpriced_notes(lines, &report.totals, &report.unpriced_models);

    if verbose {
        if let Some(diagnostics) = &report.diagnostics {
            lines.push(String::new());
            lines.push("Diagnostics:".to_string());
            lines.push(format!(
                "  Observed windows: {}",
                format_integer(diagnostics.observed_windows)
            ));
            lines.push(format!(
                "  Unobserved usage events: {}",
                format_integer(diagnostics.unobserved_usage_events)
            ));
            lines.push(format!(
                "  Usage events included: {}",
                format_integer(diagnostics.usage.included_usage_events)
            ));
            lines.push(format!(
                "  Rate-limit samples included: {}",
                format_integer(diagnostics.rate_limits.included_samples)
            ));
            lines.push(format!(
                "  Full files read: {}",
                format_integer(diagnostics.usage.read_files)
            ));
            lines.push(format!(
                "  Lines read: {}",
                format_integer(diagnostics.usage.read_lines)
            ));
        }
    }

    let warnings = super::reports::limit_usage_warnings(report);
    if !warnings.is_empty() {
        lines.push(String::new());
        lines.extend(warnings);
    }
}

fn append_unpriced_notes(
    lines: &mut Vec<String>,
    totals: &UsageStatRow,
    unpriced_models: &[UsageUnpricedModelRow],
) {
    if totals.unpriced_calls == 0 {
        return;
    }

    lines.push(String::new());
    lines.push(format!(
        "Note: {} usage events had no credit price and are excluded from Credits.",
        format_integer(totals.unpriced_calls)
    ));

    if !unpriced_models.is_empty() {
        lines.push("Unpriced models:".to_string());
        for row in unpriced_models {
            lines.push(format!(
                "  {}: {} calls, {} tokens",
                row.model,
                format_integer(row.calls),
                format_integer(row.total_tokens)
            ));
        }
        lines.push("Pricing stubs for data/codex-rate-card.json:".to_string());
        for row in unpriced_models {
            lines.push(indent_block(&row.pricing_stub, "  "));
        }
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

fn limit_usage_headers() -> Vec<String> {
    [
        "Window ID",
        "Window",
        "Window start",
        "Reset at",
        "Observed",
        "Group",
        "Group key",
        "Sessions",
        "Calls",
        "Input",
        "Cached",
        "Output",
        "Reasoning",
        "Total",
        "Credits",
        "USD",
        "Priced",
        "Unpriced",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn limit_usage_row(row: &LimitUsageRow) -> Vec<String> {
    vec![
        row.window_id.clone(),
        row.window.clone(),
        row.window_start.map(format_date_time).unwrap_or_default(),
        row.reset_at.map(format_date_time).unwrap_or_default(),
        row.observed.to_string(),
        row.group_by.to_string(),
        row.group_key.clone(),
        format_integer(row.sessions as i64),
        format_integer(row.calls),
        format_integer(row.usage.input_tokens),
        format_integer(row.usage.cached_input_tokens),
        format_integer(row.usage.output_tokens),
        format_integer(row.usage.reasoning_output_tokens),
        format_integer(row.usage.total_tokens),
        format_credits(row.credits),
        format_usd(row.usd),
        format_integer(row.priced_calls),
        format_integer(row.unpriced_calls),
    ]
}

fn limit_usage_total_row(row: &UsageStatRow) -> Vec<String> {
    vec![
        "Total".to_string(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        format_integer(row.sessions as i64),
        format_integer(row.calls),
        format_integer(row.usage.input_tokens),
        format_integer(row.usage.cached_input_tokens),
        format_integer(row.usage.output_tokens),
        format_integer(row.usage.reasoning_output_tokens),
        format_integer(row.usage.total_tokens),
        format_credits(row.credits),
        format_usd(row.usd),
        format_integer(row.priced_calls),
        format_integer(row.unpriced_calls),
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

pub(super) fn build_usage_session_compact_rows(
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
            .map(|row| aggregate_session_compact_rows(std::slice::from_ref(row)))
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
    use crate::time::StatGroupBy;
    use chrono::TimeZone;

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
                usage: usage(10, 2, 12),
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
    fn formats_usage_stats_csv_and_markdown() {
        let report = sample_usage_stats_report();

        let csv = format_usage_stats(&report, StatFormat::Csv, false).expect("csv");
        assert!(csv
            .starts_with("Group,Sessions,Calls,Input,Cached,Output,Reasoning,Total,Credits,USD\n"));
        assert!(csv.contains("Total,1,1,10,1,2,1,12,0.00,$0.00\n"));

        let markdown = format_usage_stats(&report, StatFormat::Markdown, false).expect("markdown");
        assert!(markdown.contains("| Group | Sessions | Calls |"));
        assert!(markdown.contains("| Total | 1 | 1 |"));
    }

    #[test]
    fn verbose_diagnostics_include_tail_and_mtime_counts() {
        let mut report = sample_usage_stats_report();
        let mut diagnostics = UsageDiagnostics::new(8, false);
        diagnostics.read_files = 3;
        diagnostics.tail_read_files = 5;
        diagnostics.tail_read_hits = 2;
        diagnostics.mtime_read_files = 4;
        diagnostics.mtime_tail_hits = 3;
        diagnostics.mtime_read_hits = 1;
        report.diagnostics = Some(diagnostics);

        let text = format_usage_stats(&report, StatFormat::Table, true).expect("table");

        assert!(text.contains("Full files read: 3"));
        assert!(text.contains("Tail token reads: 5"));
        assert!(text.contains("Tail token read hits: 2"));
        assert!(text.contains("File mtimes read: 4"));
        assert!(text.contains("File mtime hits requiring tail read: 3"));
        assert!(text.contains("File mtime hits requiring full read: 1"));
    }

    fn sample_usage_stats_report() -> UsageStatsReport {
        UsageStatsReport {
            start: utc_time(2026, 5, 10, 0),
            end: utc_time(2026, 5, 10, 2),
            group_by: StatGroupBy::Model,
            include_reasoning_effort: false,
            sort_by: None,
            limit: None,
            sessions_dir: "/sessions".to_string(),
            rows: vec![UsageStatRow {
                key: "gpt-5.5".to_string(),
                sessions: 1,
                calls: 1,
                usage: usage(10, 2, 12),
                credits: 0.0,
                usd: 0.0,
                priced_calls: 1,
                unpriced_calls: 0,
            }],
            totals: UsageStatRow {
                key: "Total".to_string(),
                sessions: 1,
                calls: 1,
                usage: usage(10, 2, 12),
                credits: 0.0,
                usd: 0.0,
                priced_calls: 1,
                unpriced_calls: 0,
            },
            unpriced_models: Vec::new(),
            diagnostics: None,
        }
    }

    fn usage(input_tokens: i64, output_tokens: i64, total_tokens: i64) -> TokenUsage {
        TokenUsage {
            input_tokens,
            cached_input_tokens: 1,
            output_tokens,
            reasoning_output_tokens: 1,
            total_tokens,
        }
    }

    fn utc_time(year: i32, month: u32, day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, 0, 0)
            .single()
            .expect("utc time")
    }
}
