use super::reports::{
    WeeklyCycleBreakdownRow, WeeklyCycleCurrentReport, WeeklyCycleDetailReport,
    WeeklyCycleDiagnostics, WeeklyCycleHistoryReport, WeeklyCycleReportContext,
    WeeklyCycleReportRow, WeeklyCycleUsageTotals,
};
use super::store::{AnchorListReport, WeeklyCycleAnchor};
use super::time::{format_date_time, iso_string, parse_iso_timestamp};
use super::WEEKLY_CYCLE_PERIOD_HOURS;
use crate::error::AppError;
use crate::format::{format_csv, format_integer, format_markdown_table, to_pretty_json};
use crate::stats::StatFormat;
use serde::Serialize;

pub(super) fn format_weekly_cycle_anchor_list(
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

pub(super) fn format_weekly_cycle_current(
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

pub(super) fn format_weekly_cycle_history(
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

pub(super) fn format_weekly_cycle_detail(
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

pub(super) fn format_cycle_history_prompt_item(row: &WeeklyCycleReportRow) -> String {
    format!(
        "{} | {} to {} | {} | {} calls | {} tokens | {} credits",
        row.id,
        format_date_time(parse_iso_timestamp(&row.start).expect("row start")),
        format_date_time(parse_iso_timestamp(&row.reset_at).expect("row reset")),
        row.source,
        format_integer(row.calls),
        format_integer(row.usage.total_tokens),
        format_cycle_credits(row.credits)
    )
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

fn format_cycle_credits(value: f64) -> String {
    format!("{value:.6}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

fn format_cycle_usd(value: f64) -> String {
    format!("${}", format_cycle_credits(value))
}
