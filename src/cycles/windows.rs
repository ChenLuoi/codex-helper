use super::store::WeeklyCycleAnchor;
use super::time::{compact_iso_timestamp, parse_iso_timestamp};
use super::{WEEKLY_CYCLE_PERIOD_HOURS, WEEKLY_CYCLE_PERIOD_MS};
use crate::stats::UsageRecord;
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use std::cmp::Ordering;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub(super) struct WeeklyCycleAnchorWithDate {
    pub(super) anchor: WeeklyCycleAnchor,
    pub(super) at_date: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum WeeklyCycleWindowSource {
    Manual,
    Derived,
    Estimated,
}

impl WeeklyCycleWindowSource {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Derived => "derived",
            Self::Estimated => "estimated",
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct InternalWeeklyCycleWindow {
    pub(super) start: DateTime<Utc>,
    pub(super) reset_at: DateTime<Utc>,
    pub(super) exclusive_end: DateTime<Utc>,
    pub(super) source: WeeklyCycleWindowSource,
    pub(super) anchor_id: Option<String>,
    pub(super) calibration_anchor_id: Option<String>,
}

pub(super) fn derive_anchored_weekly_cycle_windows(
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

pub(super) fn derive_estimated_weekly_cycle_windows(
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

pub(super) fn record_belongs_to_window(
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

pub(super) fn sort_anchors_with_dates(
    anchors: &[WeeklyCycleAnchor],
) -> Vec<WeeklyCycleAnchorWithDate> {
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

pub(super) fn earliest_anchor_date(anchors: &[WeeklyCycleAnchor]) -> Option<DateTime<Utc>> {
    sort_anchors_with_dates(anchors)
        .first()
        .map(|anchor| anchor.at_date)
}

pub(super) fn compare_windows(
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

pub(super) fn window_overlaps_range(
    window: &InternalWeeklyCycleWindow,
    range_start: Option<DateTime<Utc>>,
    range_end: DateTime<Utc>,
) -> bool {
    window.start <= range_end && range_start.is_none_or(|start| window.exclusive_end > start)
}

pub(super) fn weekly_cycle_window_id(window: &InternalWeeklyCycleWindow) -> String {
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

fn source_sort_key(source: WeeklyCycleWindowSource) -> u8 {
    match source {
        WeeklyCycleWindowSource::Estimated => 0,
        WeeklyCycleWindowSource::Manual => 1,
        WeeklyCycleWindowSource::Derived => 2,
    }
}
