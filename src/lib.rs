pub mod account_history;
pub mod auth;
pub mod cli;
pub mod doctor;
pub mod error;
pub mod format;
pub mod limits;
pub mod pricing;
pub mod prompt;
pub(crate) mod session_scan;
pub mod stats;
pub mod storage;
pub mod time;
pub mod usage_mode_history;

use crate::auth::{
    format_auth_profile_entry, format_auth_profile_list, format_auth_status,
    list_codex_auth_profiles, read_codex_auth_status, remove_codex_auth_profile,
    save_current_codex_auth_profile, switch_codex_auth_profile, AuthCommandOptions,
    AuthProfileEntry, AuthProfileListReport,
};
use crate::cli::{
    AuthCliCommand, AuthCliPaths, AuthProfileCliOptions, AuthRemoveCliOptions,
    AuthSelectCliOptions, AuthStatusCliOptions, CliCommand, DoctorCliCommand, DoctorCliPaths,
    FastCliCommand, FastCliOptions, LimitCliCommand, ParsedCli, StatCliCommand,
};
use crate::doctor::{format_doctor_report, read_doctor_report, DoctorOptions};
use crate::error::AppError;
use crate::format::to_pretty_json;
use crate::limits::run_limit_command;
use crate::prompt::{DialoguerPrompt, Prompt};
use crate::stats::{run_fast_candidates_command, run_stat_command};
use crate::storage::{path_to_string, resolve_storage_paths, StorageOptions};
use crate::time::DateBound;
use crate::usage_mode_history::UsageModeSwitchEvent;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::env;
use std::path::{Path, PathBuf};

const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Eq, PartialEq)]
pub struct CliResult {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CliResult {
    fn success(stdout: impl Into<String>) -> Self {
        Self {
            code: 0,
            stdout: ensure_trailing_newline(stdout.into()),
            stderr: String::new(),
        }
    }

    fn app_error(error: AppError) -> Self {
        Self {
            code: error.exit_code(),
            stdout: String::new(),
            stderr: ensure_trailing_newline(error.message().to_string()),
        }
    }

    fn parse_error(code: i32, stderr: impl Into<String>) -> Self {
        Self {
            code,
            stdout: String::new(),
            stderr: ensure_trailing_newline(stderr.into()),
        }
    }
}

pub fn run_cli<I, S>(args: I) -> CliResult
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let args: Vec<String> = args.into_iter().map(Into::into).collect();

    match cli::parse_cli(&args) {
        Ok(ParsedCli::Help(help)) => CliResult::success(help),
        Ok(ParsedCli::Version) => CliResult::success(PACKAGE_VERSION),
        Ok(ParsedCli::Command(command)) => match *command {
            CliCommand::Auth(command) => run_auth(command),
            CliCommand::Doctor(command) => run_doctor(command),
            CliCommand::Stat(command) => run_stat(command),
            CliCommand::Limit(command) => run_limit(command),
            CliCommand::Fast(command) => run_fast(command),
        },
        Err(error) => CliResult::parse_error(error.code, error.message),
    }
}

fn run_auth(command: AuthCliCommand) -> CliResult {
    match command {
        AuthCliCommand::Status(options) => run_auth_status(options),
        AuthCliCommand::Save(options) => run_auth_save(options),
        AuthCliCommand::List(options) => run_auth_list(options),
        AuthCliCommand::Select(options) => run_auth_select(options),
        AuthCliCommand::Remove(options) => run_auth_remove(options),
    }
}

fn run_auth_status(options: AuthStatusCliOptions) -> CliResult {
    let result = (|| {
        let auth_options = auth_command_options(&options.paths);
        let report = read_codex_auth_status(&auth_options, cli_now()?)?;
        format_auth_status(&report, options.json, options.include_token_claims)
    })();

    cli_result_from_string(result)
}

fn run_auth_save(options: AuthProfileCliOptions) -> CliResult {
    let result = (|| {
        let auth_options = auth_command_options(&options.paths);
        let report = save_current_codex_auth_profile(&auth_options, cli_now()?)?;
        Ok(format!(
            "Saved auth profile: {}\nStore: {}\n",
            format_auth_profile_entry(&report.profile),
            report.store_dir
        ))
    })();

    cli_result_from_string(result)
}

fn run_auth_list(options: AuthProfileCliOptions) -> CliResult {
    let result = (|| {
        let auth_options = auth_command_options(&options.paths);
        let report = list_codex_auth_profiles(&auth_options, cli_now()?)?;
        Ok(format_auth_profile_list(&report))
    })();

    cli_result_from_string(result)
}

fn run_auth_select(options: AuthSelectCliOptions) -> CliResult {
    let result = (|| {
        let now = cli_now()?;
        let auth_options = auth_command_options(&options.paths);
        let report = list_codex_auth_profiles(&auth_options, now)?;
        if let Some(account_id) = options.account_id.as_deref() {
            return select_auth_profile_by_account_id(account_id, &report, &auth_options, now);
        }

        if !prompt::stdin_and_stderr_are_terminals() {
            return Err(AppError::new(
                "auth select requires an interactive terminal unless --account-id is supplied.",
            ));
        }

        let mut prompt = DialoguerPrompt::default();
        select_auth_profile_interactively(&report, &auth_options, now, &mut prompt)
    })();

    cli_result_from_string(result)
}

fn run_auth_remove(options: AuthRemoveCliOptions) -> CliResult {
    let result = (|| {
        let now = cli_now()?;
        let auth_options = auth_command_options(&options.paths);
        let report = list_codex_auth_profiles(&auth_options, now)?;
        if report.stored.is_empty() {
            return Ok("No persisted auth profiles.\n".to_string());
        }

        if let Some(account_id) = options.account_id.as_deref() {
            if !options.yes {
                return Err(AppError::new(
                    "auth remove --account-id requires --yes when not running interactively.",
                ));
            }
            return remove_auth_profile_by_account_id(account_id, &report, &auth_options, now);
        }

        if !prompt::stdin_and_stderr_are_terminals() {
            return Err(AppError::new(
                "auth remove requires an interactive terminal unless --account-id is supplied.",
            ));
        }

        let mut prompt = DialoguerPrompt::default();
        remove_auth_profiles_interactively(&report, &auth_options, now, &mut prompt)
    })();

    cli_result_from_string(result)
}

fn select_auth_profile_interactively(
    report: &AuthProfileListReport,
    options: &AuthCommandOptions,
    now: DateTime<Utc>,
    prompt: &mut impl Prompt,
) -> Result<String, AppError> {
    if report.stored.is_empty() {
        return Err(AppError::new("No persisted auth profiles."));
    }

    let items = report
        .stored
        .iter()
        .map(format_auth_profile_entry)
        .collect::<Vec<_>>();
    let selected_index = prompt
        .select("Select auth profile", &items)?
        .ok_or_else(|| AppError::new("auth select cancelled."))?;
    let selected = report
        .stored
        .get(selected_index)
        .ok_or_else(|| AppError::new("Prompt returned an invalid auth profile selection."))?;

    select_auth_profile_entry(selected, report, options, now)
}

fn select_auth_profile_by_account_id(
    account_id: &str,
    report: &AuthProfileListReport,
    options: &AuthCommandOptions,
    now: DateTime<Utc>,
) -> Result<String, AppError> {
    let selected = report
        .stored
        .iter()
        .find(|entry| entry.account_id == account_id)
        .ok_or_else(|| {
            AppError::new(format!(
                "No persisted auth profile found for account id: {account_id}"
            ))
        })?;

    select_auth_profile_entry(selected, report, options, now)
}

fn select_auth_profile_entry(
    selected: &AuthProfileEntry,
    report: &AuthProfileListReport,
    options: &AuthCommandOptions,
    now: DateTime<Utc>,
) -> Result<String, AppError> {
    if Some(&selected.account_id) == report.current.as_ref().map(|entry| &entry.account_id) {
        return Ok(format!(
            "Auth profile already active: {}\n",
            format_auth_profile_entry(selected)
        ));
    }

    let switched = switch_codex_auth_profile(&selected.account_id, options, now)?;
    Ok(format!(
        "Saved current auth profile: {}\nActivated auth profile: {}\n",
        format_auth_profile_entry(&switched.saved_current),
        format_auth_profile_entry(&switched.activated)
    ))
}

fn remove_auth_profiles_interactively(
    report: &AuthProfileListReport,
    options: &AuthCommandOptions,
    now: DateTime<Utc>,
    prompt: &mut impl Prompt,
) -> Result<String, AppError> {
    let current_account_id = report
        .current
        .as_ref()
        .map(|entry| entry.account_id.as_str());
    let candidates = report
        .stored
        .iter()
        .filter(|entry| Some(entry.account_id.as_str()) != current_account_id)
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Ok("No removable persisted auth profiles.\n".to_string());
    }

    let items = candidates
        .iter()
        .map(|entry| format_auth_profile_entry(entry))
        .collect::<Vec<_>>();
    let selected_indices = prompt
        .multi_select("Remove auth profiles", &items)?
        .ok_or_else(|| AppError::new("auth remove cancelled."))?;
    if selected_indices.is_empty() {
        return Err(AppError::new("auth remove cancelled."));
    }

    let selected = selected_indices
        .into_iter()
        .map(|index| {
            candidates
                .get(index)
                .copied()
                .ok_or_else(|| AppError::new("Prompt returned an invalid auth profile selection."))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let confirmed = prompt
        .confirm("Remove selected auth profiles?", false)?
        .unwrap_or(false);
    if !confirmed {
        return Err(AppError::new("auth remove cancelled."));
    }

    selected
        .into_iter()
        .map(|entry| remove_auth_profile_entry(entry, options, now))
        .collect::<Result<Vec<_>, _>>()
        .map(|lines| lines.join("\n"))
}

fn remove_auth_profile_by_account_id(
    account_id: &str,
    report: &AuthProfileListReport,
    options: &AuthCommandOptions,
    now: DateTime<Utc>,
) -> Result<String, AppError> {
    let selected = report
        .stored
        .iter()
        .find(|entry| entry.account_id == account_id)
        .ok_or_else(|| {
            AppError::new(format!(
                "No persisted auth profile found for account id: {account_id}"
            ))
        })?;

    remove_auth_profile_entry(selected, options, now)
}

fn remove_auth_profile_entry(
    selected: &AuthProfileEntry,
    options: &AuthCommandOptions,
    now: DateTime<Utc>,
) -> Result<String, AppError> {
    let removed = remove_codex_auth_profile(&selected.account_id, options, now)?;
    Ok(format!(
        "Removed auth profile: {}",
        format_auth_profile_entry(&removed.removed)
    ))
}

fn run_doctor(command: DoctorCliCommand) -> CliResult {
    let DoctorCliCommand::Run(options) = command;
    let result = (|| {
        let doctor_options = doctor_command_options(&options.paths);
        let report = read_doctor_report(&doctor_options, cli_now()?);
        format_doctor_report(&report, options.json)
    })();

    cli_result_from_string(result)
}

fn run_stat(command: StatCliCommand) -> CliResult {
    let result = (|| {
        run_stat_command(
            command.view.as_deref(),
            command.session.as_deref(),
            command.options,
            cli_now()?,
        )
    })();
    cli_result_from_string(result)
}

fn run_limit(command: LimitCliCommand) -> CliResult {
    let result = (|| run_limit_command(command.command, command.options, cli_now()?))();
    cli_result_from_string(result)
}

fn run_fast(command: FastCliCommand) -> CliResult {
    let result = (|| run_fast_command(command, cli_now()?))();
    cli_result_from_string(result)
}

fn run_fast_command(command: FastCliCommand, now: DateTime<Utc>) -> Result<String, AppError> {
    match command {
        FastCliCommand::On(options) => record_fast(options, true, now),
        FastCliCommand::Off(options) => record_fast(options, false, now),
        FastCliCommand::Status(options) => format_fast_status(options, now),
        FastCliCommand::History(options) => format_fast_history(options),
        FastCliCommand::Candidates(options) => run_fast_candidates_command(*options, now),
    }
}

fn record_fast(
    options: FastCliOptions,
    fast: bool,
    now: DateTime<Utc>,
) -> Result<String, AppError> {
    let path = usage_mode_history_file_path(&options);
    let timestamp = fast_timestamp(options.at.as_deref(), now)?;
    let current = usage_mode_history::read_usage_mode_history_store(&path)?;
    let next = usage_mode_history::record_usage_mode_switch(current, fast, timestamp)?;
    usage_mode_history::write_usage_mode_history_store(&path, &next)?;

    if options.json {
        return fast_record_json(&path, fast, timestamp);
    }

    Ok(format!(
        "Recorded local fast attribution only. This does not change Codex settings.\nFast: {}\nAt: {}\nHistory file: {}\n",
        fast_label(Some(fast)),
        usage_mode_history::format_usage_mode_history_iso(timestamp),
        path_to_string(&path)
    ))
}

fn format_fast_status(options: FastCliOptions, now: DateTime<Utc>) -> Result<String, AppError> {
    let path = usage_mode_history_file_path(&options);
    let store = usage_mode_history::read_usage_mode_history_store(&path)?;
    let fast = usage_mode_history::usage_mode_history_from_store(store.clone())?
        .and_then(|history| history.fast_at(now));
    let latest_switch = store.switches.last();

    if options.json {
        let json = FastStatusJson {
            history_file: path_to_string(&path),
            as_of: usage_mode_history::format_usage_mode_history_iso(now),
            state: fast_label(fast).to_string(),
            fast,
            latest_switch,
            switch_count: store.switches.len(),
            recorded_local_attribution_only: true,
            changes_codex_settings: false,
        };
        return to_pretty_json(&json).map_err(|error| AppError::new(error.to_string()));
    }

    let latest = latest_switch
        .map(format_usage_mode_switch)
        .unwrap_or_else(|| "none".to_string());
    Ok(format!(
        "Local fast attribution: {}\nAs of: {}\nLatest switch: {}\nSwitches: {}\nHistory file: {}\nRecorded local fast attribution only. This does not change Codex settings.\n",
        fast_label(fast),
        usage_mode_history::format_usage_mode_history_iso(now),
        latest,
        store.switches.len(),
        path_to_string(&path)
    ))
}

fn format_fast_history(options: FastCliOptions) -> Result<String, AppError> {
    let path = usage_mode_history_file_path(&options);
    let store = usage_mode_history::read_usage_mode_history_store(&path)?;

    if options.json {
        let json = FastHistoryJson {
            history_file: path_to_string(&path),
            default_mode: store.default_mode.as_ref(),
            switches: &store.switches,
            switch_count: store.switches.len(),
            recorded_local_attribution_only: true,
            changes_codex_settings: false,
        };
        return to_pretty_json(&json).map_err(|error| AppError::new(error.to_string()));
    }

    let mut output =
        "Recorded local fast attribution only. This does not change Codex settings.\n".to_string();
    output.push_str(&format!("History file: {}\n", path_to_string(&path)));

    if let Some(default_mode) = &store.default_mode {
        output.push_str(&format!(
            "Default: {} at {} ({})\n",
            fast_label(Some(default_mode.fast)),
            default_mode.observed_at,
            default_mode.source
        ));
    }

    if store.switches.is_empty() {
        output.push_str("No local fast attribution switches recorded.\n");
        return Ok(output);
    }

    output.push_str("\nTime                     Fast    Source\n");
    output.push_str("-----------------------  ------  ----------------\n");
    for entry in &store.switches {
        output.push_str(&format!(
            "{:<23}  {:<6}  {}\n",
            entry.timestamp,
            fast_label(Some(entry.fast)),
            entry.source
        ));
    }

    Ok(output)
}

fn fast_timestamp(at: Option<&str>, now: DateTime<Utc>) -> Result<DateTime<Utc>, AppError> {
    match at {
        Some(value) => time::parse_date_bound(value, DateBound::Start),
        None => Ok(now),
    }
}

fn usage_mode_history_file_path(options: &FastCliOptions) -> PathBuf {
    resolve_storage_paths(&StorageOptions {
        usage_mode_history_file: options.usage_mode_history_file.clone(),
        ..StorageOptions::default()
    })
    .usage_mode_history_file
}

fn fast_label(fast: Option<bool>) -> &'static str {
    match fast {
        Some(true) => "on",
        Some(false) => "off",
        None => "unknown",
    }
}

fn format_usage_mode_switch(entry: &UsageModeSwitchEvent) -> String {
    format!(
        "{} -> {} ({})",
        entry.timestamp,
        fast_label(Some(entry.fast)),
        entry.source
    )
}

fn fast_record_json(path: &Path, fast: bool, timestamp: DateTime<Utc>) -> Result<String, AppError> {
    let json = FastRecordJson {
        history_file: path_to_string(path),
        timestamp: usage_mode_history::format_usage_mode_history_iso(timestamp),
        state: fast_label(Some(fast)),
        fast,
        recorded_local_attribution_only: true,
        changes_codex_settings: false,
    };
    to_pretty_json(&json).map_err(|error| AppError::new(error.to_string()))
}

fn auth_command_options(paths: &AuthCliPaths) -> AuthCommandOptions {
    AuthCommandOptions {
        auth_file: paths.auth_file.clone(),
        codex_home: paths.codex_home.clone(),
        store_dir: paths.store_dir.clone(),
        account_history_file: paths.account_history_file.clone(),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FastRecordJson {
    history_file: String,
    timestamp: String,
    state: &'static str,
    fast: bool,
    recorded_local_attribution_only: bool,
    changes_codex_settings: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FastStatusJson<'a> {
    history_file: String,
    as_of: String,
    state: String,
    fast: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    latest_switch: Option<&'a UsageModeSwitchEvent>,
    switch_count: usize,
    recorded_local_attribution_only: bool,
    changes_codex_settings: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FastHistoryJson<'a> {
    history_file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_mode: Option<&'a usage_mode_history::UsageModeDefault>,
    switches: &'a [UsageModeSwitchEvent],
    switch_count: usize,
    recorded_local_attribution_only: bool,
    changes_codex_settings: bool,
}

fn doctor_command_options(paths: &DoctorCliPaths) -> DoctorOptions {
    DoctorOptions {
        auth_file: paths.auth_file.clone(),
        codex_home: paths.codex_home.clone(),
        sessions_dir: paths.sessions_dir.clone(),
    }
}

fn cli_now() -> Result<DateTime<Utc>, AppError> {
    match env::var("CODEX_OPS_FIXED_NOW") {
        Ok(value) if !value.trim().is_empty() => DateTime::parse_from_rfc3339(value.trim())
            .map(|date| date.with_timezone(&Utc))
            .map_err(|_| {
                AppError::new("Invalid CODEX_OPS_FIXED_NOW. Expected an ISO date string.")
            }),
        _ => Ok(Utc::now()),
    }
}

fn cli_result_from_string(result: Result<String, AppError>) -> CliResult {
    match result {
        Ok(stdout) => CliResult::success(stdout),
        Err(error) => CliResult::app_error(error),
    }
}

fn ensure_trailing_newline(mut value: String) -> String {
    if !value.ends_with('\n') {
        value.push('\n');
    }
    value
}

#[cfg(test)]
pub(crate) mod test_utils;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;

    #[test]
    fn root_help_lists_top_level_commands() {
        let result = run_cli(["--help"]);

        assert_eq!(result.code, 0);
        assert!(result.stderr.is_empty());
        assert!(result.stdout.contains("Usage: codex-ops"));
        assert!(result.stdout.contains("auth"));
        assert!(result.stdout.contains("doctor"));
        assert!(result.stdout.contains("stat"));
        assert!(result.stdout.contains("limit"));
        assert!(!result.stdout.contains("cycle"));
    }

    #[test]
    fn auth_help_lists_profile_commands() {
        let result = run_cli(["auth", "--help"]);

        assert_eq!(result.code, 0);
        assert!(result.stdout.contains("status"));
        assert!(result.stdout.contains("save"));
        assert!(result.stdout.contains("list"));
        assert!(result.stdout.contains("select"));
        assert!(result.stdout.contains("remove"));
    }

    #[test]
    fn child_help_works() {
        let result = run_cli(["stat", "--help"]);

        assert_eq!(result.code, 0);
        assert!(result.stdout.contains("--group-by"));
        assert!(result.stdout.contains("sessions"));
    }

    #[test]
    fn unknown_command_returns_error() {
        let result = run_cli(["missing"]);

        assert_eq!(result.code, 2);
        assert!(result.stderr.contains("unrecognized subcommand 'missing'"));
    }

    #[test]
    fn removed_cycle_command_returns_unknown_command() {
        let result = run_cli(["cycle", "current"]);

        assert_eq!(result.code, 2);
        assert!(result.stdout.is_empty());
        assert!(result.stderr.contains("unrecognized subcommand 'cycle'"));
    }

    #[test]
    fn interactive_auth_select_switches_profile_and_writes_history() {
        let fixture = AuthPromptFixture::new("select-switch");
        let now = fixed_now();
        let current_content = auth_content("account-a", "a@example.test", "plus");
        let selected_content = auth_content("account-b", "b@example.test", "pro");

        fixture.save_profile(&selected_content, now);
        std::fs::write(&fixture.auth_file, &current_content).expect("write current auth");
        let options = fixture.options();
        let report = list_codex_auth_profiles(&options, now).expect("list profiles");
        let mut prompt = FakePrompt {
            select: Some(Some(0)),
            ..FakePrompt::default()
        };

        let output =
            select_auth_profile_interactively(&report, &options, now, &mut prompt).unwrap();

        assert!(output.contains("Activated auth profile: b@example.test(account-b) - pro"));
        assert_eq!(
            std::fs::read_to_string(&fixture.auth_file).expect("read auth"),
            selected_content
        );
        let history: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&fixture.history_file).expect("read history"),
        )
        .expect("parse history");
        assert_eq!(history["switches"][0]["fromAccountId"], "account-a");
        assert_eq!(history["switches"][0]["toAccountId"], "account-b");
        assert_eq!(
            prompt.select_items[0],
            vec!["b@example.test(account-b) - pro".to_string()]
        );
    }

    #[test]
    fn interactive_auth_select_cancel_has_no_side_effects() {
        let fixture = AuthPromptFixture::new("select-cancel");
        let now = fixed_now();
        let current_content = auth_content("account-a", "a@example.test", "plus");
        let selected_content = auth_content("account-b", "b@example.test", "pro");

        fixture.save_profile(&selected_content, now);
        std::fs::write(&fixture.auth_file, &current_content).expect("write current auth");
        let options = fixture.options();
        let report = list_codex_auth_profiles(&options, now).expect("list profiles");
        let mut prompt = FakePrompt {
            select: Some(None),
            ..FakePrompt::default()
        };

        let error =
            select_auth_profile_interactively(&report, &options, now, &mut prompt).unwrap_err();

        assert_eq!(error.message(), "auth select cancelled.");
        assert_eq!(
            std::fs::read_to_string(&fixture.auth_file).expect("read auth"),
            current_content
        );
        assert!(!fixture.history_file.exists());
    }

    #[test]
    fn interactive_auth_remove_deletes_selected_profiles_but_not_current() {
        let fixture = AuthPromptFixture::new("remove-selected");
        let now = fixed_now();
        let current_content = auth_content("account-a", "a@example.test", "plus");
        let other_content = auth_content("account-b", "b@example.test", "pro");
        let third_content = auth_content("account-c", "c@example.test", "team");

        fixture.save_profile(&current_content, now);
        fixture.save_profile(&other_content, now);
        fixture.save_profile(&third_content, now);
        std::fs::write(&fixture.auth_file, &current_content).expect("write current auth");
        let options = fixture.options();
        let report = list_codex_auth_profiles(&options, now).expect("list profiles");
        let mut prompt = FakePrompt {
            multi_select: Some(Some(vec![0])),
            confirm: Some(Some(true)),
            ..FakePrompt::default()
        };

        let output =
            remove_auth_profiles_interactively(&report, &options, now, &mut prompt).unwrap();

        assert!(output.contains("Removed auth profile: b@example.test(account-b) - pro"));
        assert_eq!(
            prompt.multi_select_items[0],
            vec![
                "b@example.test(account-b) - pro".to_string(),
                "c@example.test(account-c) - team".to_string()
            ]
        );
        assert!(fixture.profile_file("account-a").exists());
        assert!(!fixture.profile_file("account-b").exists());
        assert!(fixture.profile_file("account-c").exists());
    }

    #[test]
    fn interactive_auth_remove_confirmation_reject_has_no_side_effects() {
        let fixture = AuthPromptFixture::new("remove-cancel");
        let now = fixed_now();
        let current_content = auth_content("account-a", "a@example.test", "plus");
        let other_content = auth_content("account-b", "b@example.test", "pro");

        fixture.save_profile(&current_content, now);
        fixture.save_profile(&other_content, now);
        std::fs::write(&fixture.auth_file, &current_content).expect("write current auth");
        let options = fixture.options();
        let report = list_codex_auth_profiles(&options, now).expect("list profiles");
        let mut prompt = FakePrompt {
            multi_select: Some(Some(vec![0])),
            confirm: Some(Some(false)),
            ..FakePrompt::default()
        };

        let error =
            remove_auth_profiles_interactively(&report, &options, now, &mut prompt).unwrap_err();

        assert_eq!(error.message(), "auth remove cancelled.");
        assert!(fixture.profile_file("account-a").exists());
        assert!(fixture.profile_file("account-b").exists());
    }

    #[derive(Default)]
    struct FakePrompt {
        select: Option<Option<usize>>,
        multi_select: Option<Option<Vec<usize>>>,
        confirm: Option<Option<bool>>,
        select_items: Vec<Vec<String>>,
        multi_select_items: Vec<Vec<String>>,
    }

    impl Prompt for FakePrompt {
        fn select(&mut self, _prompt: &str, items: &[String]) -> Result<Option<usize>, AppError> {
            self.select_items.push(items.to_vec());
            self.select
                .take()
                .ok_or_else(|| AppError::new("missing fake select response"))
        }

        fn multi_select(
            &mut self,
            _prompt: &str,
            items: &[String],
        ) -> Result<Option<Vec<usize>>, AppError> {
            self.multi_select_items.push(items.to_vec());
            self.multi_select
                .take()
                .ok_or_else(|| AppError::new("missing fake multi-select response"))
        }

        fn confirm(&mut self, _prompt: &str, _default: bool) -> Result<Option<bool>, AppError> {
            self.confirm
                .take()
                .ok_or_else(|| AppError::new("missing fake confirm response"))
        }
    }

    struct AuthPromptFixture {
        root: std::path::PathBuf,
        auth_file: std::path::PathBuf,
        store_dir: std::path::PathBuf,
        history_file: std::path::PathBuf,
    }

    impl AuthPromptFixture {
        fn new(label: &str) -> Self {
            let root = temp_dir(&format!("codex-ops-auth-{label}"));
            std::fs::create_dir_all(&root).expect("create auth fixture root");
            Self {
                auth_file: root.join("auth.json"),
                store_dir: root.join("auth-profiles"),
                history_file: root.join("auth-account-history.json"),
                root,
            }
        }

        fn options(&self) -> AuthCommandOptions {
            AuthCommandOptions {
                auth_file: Some(self.auth_file.clone()),
                store_dir: Some(self.store_dir.clone()),
                account_history_file: Some(self.history_file.clone()),
                ..AuthCommandOptions::default()
            }
        }

        fn save_profile(&self, content: &str, now: DateTime<Utc>) {
            std::fs::write(&self.auth_file, content).expect("write auth profile content");
            save_current_codex_auth_profile(
                &AuthCommandOptions {
                    auth_file: Some(self.auth_file.clone()),
                    store_dir: Some(self.store_dir.clone()),
                    ..AuthCommandOptions::default()
                },
                now,
            )
            .expect("save auth profile");
        }

        fn profile_file(&self, account_id: &str) -> std::path::PathBuf {
            self.store_dir.join(format!("{account_id}.json"))
        }
    }

    impl Drop for AuthPromptFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn fixed_now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-05-13T00:00:00.000Z")
            .expect("fixed now")
            .with_timezone(&Utc)
    }
}
