use super::reports::{
    TokenUsage, UsageDiagnostics, UsageRecordView, UsageSessionDetailReport, UsageSessionEventRow,
    UsageSessionRow, UsageSessionsReport, UsageStatRow, UsageStatsReport, UsageUnpricedModelRow,
};
use super::scan::UsageRecordAccumulator;
use super::StatSort;
use crate::format::{credits_to_usd, round_credits};
use crate::pricing::{calculate_credit_cost, normalize_model_name};
use crate::time::StatGroupBy;
use chrono::{DateTime, Datelike, Local, Timelike, Utc};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

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

pub(super) struct UsageStatsAccumulator {
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
    pub(super) fn new(
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

    pub(super) fn add(&mut self, record: UsageRecordView<'_>) {
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

    pub(super) fn finish(self, diagnostics: Option<UsageDiagnostics>) -> UsageStatsReport {
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

pub(super) struct UsageSessionsAccumulator {
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
    pub(super) fn new(
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

    pub(super) fn add(&mut self, record: UsageRecordView<'_>) {
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

    pub(super) fn finish(self, diagnostics: Option<UsageDiagnostics>) -> UsageSessionsReport {
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

pub(super) struct UsageSessionDetailAccumulator {
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
    pub(super) fn new(
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

    pub(super) fn add(&mut self, record: UsageRecordView<'_>) {
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

    pub(super) fn finish(
        mut self,
        diagnostics: Option<UsageDiagnostics>,
    ) -> UsageSessionDetailReport {
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

    match effort {
        Some(effort) if record.model != "unknown" => format!("{}-{}", record.model, effort),
        _ => record.model.to_string(),
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
        "{{\n  \"key\": \"{key}\",\n  \"label\": \"{}\",\n  \"input_credits_per_million\": 0,\n  \"cached_input_credits_per_million\": 0,\n  \"output_credits_per_million\": 0\n}}",
        escape_double_quoted(model)
    )
}

fn escape_double_quoted(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

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

    #[test]
    fn records_unpriced_model_notes_and_pricing_stub() {
        let start = utc_time(2026, 5, 10, 0);
        let end = utc_time(2026, 5, 10, 2);
        let mut accumulator = UsageStatsAccumulator::new(
            start,
            end,
            StatGroupBy::Model,
            "/sessions".to_string(),
            false,
            None,
            None,
        );
        let usage = usage(10, 2, 12);

        accumulator.add(test_record(
            utc_time(2026, 5, 10, 0),
            "session-a",
            "brand-new-model",
            "/repo-a",
            "/tmp/a.jsonl",
            &usage,
        ));

        let report = accumulator.finish(None);
        let row = report.unpriced_models.first().expect("unpriced model row");

        assert_eq!(report.totals.unpriced_calls, 1);
        assert_eq!(row.model, "brand-new-model");
        assert_eq!(row.calls, 1);
        assert_eq!(row.total_tokens, 12);
        assert!(row.pricing_stub.contains("\"brand-new-model\""));
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
