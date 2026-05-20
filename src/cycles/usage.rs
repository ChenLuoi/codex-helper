use super::cli::{resolve_account_history_file, CycleCommandOptions};
use super::reports::{
    WeeklyCycleBreakdownRow, WeeklyCycleReportRow, WeeklyCycleUnpricedModelRow,
    WeeklyCycleUsageTotals,
};
use super::windows::earliest_anchor_date;
use crate::error::AppError;
use crate::pricing::{
    calculate_credit_cost, normalize_model_name, TokenUsage as PricingTokenUsage,
};
use crate::stats::{
    read_usage_records_report, ResolvedStatRangeOptions, TokenUsage, UsageDiagnostics, UsageRecord,
    UsageRecordsReadOptions,
};
use crate::storage::{resolve_storage_paths, StorageOptions};
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, HashMap, HashSet};

use super::store::WeeklyCycleAnchor;

pub(super) struct CycleUsageReadResult {
    pub(super) records: Vec<UsageRecord>,
    pub(super) diagnostics: Option<UsageDiagnostics>,
}

pub(super) fn read_weekly_cycle_usage_for_current(
    anchors: &[WeeklyCycleAnchor],
    account_id: &str,
    options: &CycleCommandOptions,
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

pub(super) fn read_weekly_cycle_usage_for_history(
    anchors: &[WeeklyCycleAnchor],
    account_id: &str,
    options: &CycleCommandOptions,
    range: &ResolvedStatRangeOptions,
) -> Result<CycleUsageReadResult, AppError> {
    let Some(earliest_anchor) = earliest_anchor_date(anchors) else {
        return Ok(CycleUsageReadResult {
            records: Vec::new(),
            diagnostics: None,
        });
    };
    let scan_start = if options.estimate_before_anchor && range.start < earliest_anchor {
        range.start
    } else {
        earliest_anchor
    };
    if scan_start > range.end {
        return Ok(CycleUsageReadResult {
            records: Vec::new(),
            diagnostics: None,
        });
    }

    let report = read_usage_records_report(&UsageRecordsReadOptions {
        start: scan_start,
        end: range.end,
        sessions_dir: range.sessions_dir.clone(),
        scan_all_files: true,
        account_history_file: Some(resolve_account_history_file(options)),
        account_id: Some(account_id.to_string()),
    })?;

    Ok(CycleUsageReadResult {
        records: report.records,
        diagnostics: Some(report.diagnostics),
    })
}

pub(super) fn aggregate_weekly_cycle_records(records: &[UsageRecord]) -> WeeklyCycleUsageTotals {
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

pub(super) fn build_weekly_cycle_breakdown(
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

pub(super) fn usage_totals_from_row(row: &WeeklyCycleReportRow) -> WeeklyCycleUsageTotals {
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

pub(super) fn empty_weekly_cycle_totals() -> WeeklyCycleUsageTotals {
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

pub(super) fn sort_usage_records(records: &mut [UsageRecord]) {
    records.sort_by(|left, right| {
        left.timestamp
            .cmp(&right.timestamp)
            .then_with(|| left.session_id.cmp(&right.session_id))
            .then_with(|| left.file_path.cmp(&right.file_path))
    });
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

fn round_credits(value: f64) -> f64 {
    ((value + f64::EPSILON) * 1_000_000.0).round() / 1_000_000.0
}

fn credits_to_usd(credits: f64) -> f64 {
    (((credits / 25.0) + f64::EPSILON) * 1_000_000.0).round() / 1_000_000.0
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
        "{{\n  \"key\": \"{key}\",\n  \"label\": \"{}\",\n  \"input_credits_per_million\": 0,\n  \"cached_input_credits_per_million\": 0,\n  \"output_credits_per_million\": 0\n}}",
        model.replace('\\', "\\\\").replace('"', "\\\"")
    )
}
