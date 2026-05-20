use super::store::WeeklyCycleAnchor;
use super::time::{iso_string, local_date_key, parse_iso_timestamp};
use super::usage::{
    aggregate_weekly_cycle_records, build_weekly_cycle_breakdown, empty_weekly_cycle_totals,
    sort_usage_records, usage_totals_from_row,
};
use super::windows::{
    compare_windows, derive_anchored_weekly_cycle_windows, derive_estimated_weekly_cycle_windows,
    record_belongs_to_window, sort_anchors_with_dates, weekly_cycle_window_id,
    window_overlaps_range, InternalWeeklyCycleWindow, WeeklyCycleAnchorWithDate,
};
use super::{normalize_required_id, WEEKLY_CYCLE_PERIOD_HOURS};
use crate::error::AppError;
use crate::stats::{TokenUsage, UsageDiagnostics, UsageRecord};
use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(super) struct WeeklyCycleUnpricedModelRow {
    pub(super) model: String,
    pub(super) pricing_key: String,
    pub(super) calls: i64,
    pub(super) total_tokens: i64,
    pub(super) pricing_stub: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(super) struct WeeklyCycleUsageTotals {
    pub(super) sessions: usize,
    pub(super) calls: i64,
    pub(super) usage: TokenUsage,
    pub(super) credits: f64,
    pub(super) usd: f64,
    pub(super) priced_calls: i64,
    pub(super) unpriced_calls: i64,
    pub(super) unpriced_models: Vec<WeeklyCycleUnpricedModelRow>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(super) struct WeeklyCycleReportRow {
    pub(super) sessions: usize,
    pub(super) calls: i64,
    pub(super) usage: TokenUsage,
    pub(super) credits: f64,
    pub(super) usd: f64,
    pub(super) priced_calls: i64,
    pub(super) unpriced_calls: i64,
    pub(super) unpriced_models: Vec<WeeklyCycleUnpricedModelRow>,
    pub(super) id: String,
    pub(super) index: usize,
    pub(super) start: String,
    pub(super) reset_at: String,
    pub(super) exclusive_end: String,
    pub(super) source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) anchor_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) calibration_anchor_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(super) struct WeeklyCycleBreakdownRow {
    pub(super) key: String,
    pub(super) sessions: usize,
    pub(super) calls: i64,
    pub(super) usage: TokenUsage,
    pub(super) credits: f64,
    pub(super) usd: f64,
    pub(super) priced_calls: i64,
    pub(super) unpriced_calls: i64,
    pub(super) unpriced_models: Vec<WeeklyCycleUnpricedModelRow>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(super) struct WeeklyCycleDiagnostics {
    pub(super) anchors: usize,
    pub(super) usage_records: usize,
    pub(super) windows: usize,
    pub(super) derived_windows: usize,
    pub(super) estimated_windows: usize,
    pub(super) included_usage_events: i64,
    pub(super) ignored_before_anchor_events: usize,
    pub(super) estimate_before_anchor: bool,
    pub(super) unanchored: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) usage_diagnostics: Option<UsageDiagnostics>,
}

#[derive(Debug, Clone)]
pub(super) struct WeeklyCycleHistoryReport {
    pub(super) status: &'static str,
    pub(super) period_hours: i64,
    pub(super) start: Option<DateTime<Utc>>,
    pub(super) end: DateTime<Utc>,
    pub(super) rows: Vec<WeeklyCycleReportRow>,
    pub(super) totals: WeeklyCycleUsageTotals,
    pub(super) diagnostics: WeeklyCycleDiagnostics,
}

#[derive(Debug, Clone)]
pub(super) struct WeeklyCycleCurrentReport {
    pub(super) status: &'static str,
    pub(super) period_hours: i64,
    pub(super) now: DateTime<Utc>,
    pub(super) current: Option<WeeklyCycleReportRow>,
    pub(super) by_day: Vec<WeeklyCycleBreakdownRow>,
    pub(super) by_model: Vec<WeeklyCycleBreakdownRow>,
    pub(super) totals: WeeklyCycleUsageTotals,
    pub(super) diagnostics: WeeklyCycleDiagnostics,
}

#[derive(Debug, Clone)]
pub(super) struct WeeklyCycleDetailReport {
    pub(super) status: &'static str,
    pub(super) cycle_id: String,
    pub(super) period_hours: i64,
    pub(super) start: Option<DateTime<Utc>>,
    pub(super) end: DateTime<Utc>,
    pub(super) row: WeeklyCycleReportRow,
    pub(super) by_day: Vec<WeeklyCycleBreakdownRow>,
    pub(super) by_model: Vec<WeeklyCycleBreakdownRow>,
    pub(super) totals: WeeklyCycleUsageTotals,
    pub(super) diagnostics: WeeklyCycleDiagnostics,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WeeklyCycleReportContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) account_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) account_source: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) cycle_file: Option<String>,
}

pub(super) fn build_weekly_cycle_history_report(
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

pub(super) fn build_weekly_cycle_current_report(
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
    let (rows, totals) =
        build_cycle_rows(std::slice::from_ref(&current_window), &records, None, None);
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

pub(super) fn build_weekly_cycle_detail_report(
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
