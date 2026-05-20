use super::accounts::{
    cycle_report_context, format_cycle_account_line, resolve_cycle_account_label,
};
use super::formatters::{
    format_cycle_history_prompt_item, format_weekly_cycle_anchor_list, format_weekly_cycle_current,
    format_weekly_cycle_detail, format_weekly_cycle_history,
};
use super::reports::{
    build_weekly_cycle_current_report, build_weekly_cycle_detail_report,
    build_weekly_cycle_history_report, WeeklyCycleHistoryReport, WeeklyCycleReportContext,
};
use super::store::{
    add_weekly_cycle_anchors_to_file, list_weekly_cycle_anchors_from_file,
    remove_weekly_cycle_anchor_from_file,
};
use super::time::parse_cycle_add_times;
use super::usage::{read_weekly_cycle_usage_for_current, read_weekly_cycle_usage_for_history};
use crate::auth::AuthCommandOptions;
use crate::error::AppError;
use crate::prompt::{self, DialoguerPrompt, Prompt};
use crate::stats::{
    resolve_stat_range_options_from_raw, StatCommandOptions, StatFormat, UsageDiagnostics,
    UsageRecord,
};
use crate::storage::{resolve_storage_paths, StorageOptions};
use chrono::{DateTime, Utc};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct CycleCommandHelps<'a> {
    pub add: &'a str,
    pub list: &'a str,
    pub remove: &'a str,
    pub current: &'a str,
    pub history: &'a str,
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct CycleCommandOptions {
    pub auth_file: Option<PathBuf>,
    pub codex_home: Option<PathBuf>,
    pub cycle_file: Option<PathBuf>,
    pub account_history_file: Option<PathBuf>,
    pub sessions_dir: Option<PathBuf>,
    pub account_id: Option<String>,
    pub note: Option<String>,
    pub format: Option<String>,
    pub json: bool,
    pub select: bool,
    pub estimate_before_anchor: bool,
    pub stat: StatCommandOptions,
}

pub fn run_cycle_add(
    time_parts: &[String],
    options: CycleCommandOptions,
    now: DateTime<Utc>,
) -> Result<String, AppError> {
    let times = parse_cycle_add_times(time_parts)?;
    let report = add_weekly_cycle_anchors_to_file(&options, &times, now)?;
    let account_label = resolve_cycle_account_label(&report.account_id, &options, now);
    let mut lines = Vec::new();

    if report.anchors.len() == 1 {
        let anchor = report
            .anchors
            .first()
            .ok_or_else(|| AppError::new("No weekly cycle anchor was added."))?;
        lines.push(format!("Added weekly cycle anchor: {}", anchor.id));
        lines.push(format!("At: {}", anchor.at));
    } else {
        lines.push(format!(
            "Added {} weekly cycle anchors:",
            report.anchors.len()
        ));
        for anchor in &report.anchors {
            lines.push(format!("- {} at {}", anchor.id, anchor.at));
        }
    }

    lines.push(format_cycle_account_line(
        &report.account_id,
        account_label.as_deref(),
    ));
    lines.push(format!("Cycle file: {}", report.cycle_file));
    Ok(lines.join("\n"))
}

pub fn run_cycle_list(
    options: CycleCommandOptions,
    now: DateTime<Utc>,
) -> Result<String, AppError> {
    let report = list_weekly_cycle_anchors_from_file(&options, now)?;
    let context = cycle_report_context(
        &report.account_id,
        report.account_source,
        &report.cycle_file,
        &options,
        now,
    );
    format_weekly_cycle_anchor_list(&report, resolve_cycle_format(&options)?, &context)
}

pub fn run_cycle_remove(
    anchor_id: &str,
    options: CycleCommandOptions,
    now: DateTime<Utc>,
) -> Result<String, AppError> {
    let report = remove_weekly_cycle_anchor_from_file(anchor_id, &options, now)?;
    let account_label = resolve_cycle_account_label(&report.account_id, &options, now);

    Ok(format!(
        "Removed weekly cycle anchor: {}\n{}\nCycle file: {}",
        report.anchor.id,
        format_cycle_account_line(&report.account_id, account_label.as_deref()),
        report.cycle_file
    ))
}

pub fn run_cycle_current(
    options: CycleCommandOptions,
    now: DateTime<Utc>,
) -> Result<String, AppError> {
    let format = resolve_cycle_format(&options)?;
    let anchor_report = list_weekly_cycle_anchors_from_file(&options, now)?;
    let context = cycle_report_context(
        &anchor_report.account_id,
        anchor_report.account_source,
        &anchor_report.cycle_file,
        &options,
        now,
    );
    let usage = read_weekly_cycle_usage_for_current(
        &anchor_report.anchors,
        &anchor_report.account_id,
        &options,
        now,
    )?;
    let report = build_weekly_cycle_current_report(
        &anchor_report.anchors,
        usage.records,
        now,
        usage.diagnostics,
    );

    format_weekly_cycle_current(&report, format, &context)
}

pub fn run_cycle_history(
    cycle_id: Option<String>,
    mut options: CycleCommandOptions,
    now: DateTime<Utc>,
) -> Result<String, AppError> {
    if cycle_id.is_some() && options.select {
        return Err(AppError::new(
            "cycle history accepts either a cycle id or --select, not both.",
        ));
    }

    if !has_explicit_cycle_history_range(&options) {
        options.stat.all = true;
    }

    let format = resolve_cycle_format(&options)?;
    let range = resolve_stat_range_options_from_raw(&options.stat, now)?;
    let anchor_report = list_weekly_cycle_anchors_from_file(&options, now)?;
    let context = cycle_report_context(
        &anchor_report.account_id,
        anchor_report.account_source,
        &anchor_report.cycle_file,
        &options,
        now,
    );
    let usage = read_weekly_cycle_usage_for_history(
        &anchor_report.anchors,
        &anchor_report.account_id,
        &options,
        &range,
    )?;
    let history = build_weekly_cycle_history_report(
        &anchor_report.anchors,
        usage.records.clone(),
        Some(range.start),
        range.end,
        options.estimate_before_anchor,
        usage.diagnostics.clone(),
    );

    if let Some(cycle_id) = cycle_id {
        let detail = build_weekly_cycle_detail_report(
            &history,
            &cycle_id,
            usage.records,
            usage.diagnostics,
        )?;
        return format_weekly_cycle_detail(&detail, format, &context);
    }

    if options.select {
        if history.rows.is_empty() {
            return Ok("No weekly cycles to select.\n".to_string());
        }
        if !prompt::stdin_and_stderr_are_terminals() {
            return Err(AppError::new(
                "cycle history --select requires an interactive terminal unless a cycle id is supplied.",
            ));
        }

        let mut prompt = DialoguerPrompt::default();
        return select_weekly_cycle_history_detail(
            &history,
            usage.records,
            usage.diagnostics,
            format,
            &context,
            &mut prompt,
        );
    }

    format_weekly_cycle_history(&history, format, &context)
}

pub(super) fn select_weekly_cycle_history_detail(
    history: &WeeklyCycleHistoryReport,
    records: Vec<UsageRecord>,
    usage_diagnostics: Option<UsageDiagnostics>,
    format: StatFormat,
    context: &WeeklyCycleReportContext,
    prompt: &mut impl Prompt,
) -> Result<String, AppError> {
    let items = history
        .rows
        .iter()
        .map(format_cycle_history_prompt_item)
        .collect::<Vec<_>>();
    let selected_index = prompt
        .select("Select weekly cycle", &items)?
        .ok_or_else(|| AppError::new("cycle history select cancelled."))?;
    let selected = history
        .rows
        .get(selected_index)
        .ok_or_else(|| AppError::new("Prompt returned an invalid weekly cycle selection."))?;
    let detail =
        build_weekly_cycle_detail_report(history, &selected.id, records, usage_diagnostics)?;
    format_weekly_cycle_detail(&detail, format, context)
}

pub(super) fn resolve_cycle_format(options: &CycleCommandOptions) -> Result<StatFormat, AppError> {
    if options.json {
        return Ok(StatFormat::Json);
    }
    match options.format.as_deref().unwrap_or("table") {
        "table" => Ok(StatFormat::Table),
        "json" => Ok(StatFormat::Json),
        "csv" => Ok(StatFormat::Csv),
        "markdown" => Ok(StatFormat::Markdown),
        _ => Err(AppError::invalid_input(
            "Invalid format value. Expected one of: table, json, csv, markdown.",
        )),
    }
}

pub(super) fn has_explicit_cycle_history_range(options: &CycleCommandOptions) -> bool {
    options.stat.all
        || options.stat.today
        || options.stat.yesterday
        || options.stat.month
        || options.stat.last.is_some()
        || options.stat.start.is_some()
        || options.stat.end.is_some()
}

pub(super) fn resolve_cycle_file(options: &CycleCommandOptions) -> PathBuf {
    resolve_storage_paths(&StorageOptions {
        codex_home: options.codex_home.clone(),
        cycle_file: options.cycle_file.clone(),
        ..StorageOptions::default()
    })
    .cycle_file
}

pub(super) fn resolve_account_history_file(options: &CycleCommandOptions) -> PathBuf {
    resolve_storage_paths(&StorageOptions {
        codex_home: options.codex_home.clone(),
        account_history_file: options.account_history_file.clone(),
        ..StorageOptions::default()
    })
    .account_history_file
}

pub(super) fn auth_options(options: &CycleCommandOptions) -> AuthCommandOptions {
    AuthCommandOptions {
        auth_file: options.auth_file.clone(),
        codex_home: options.codex_home.clone(),
        store_dir: None,
        account_history_file: options.account_history_file.clone(),
    }
}
