pub mod account_history;
pub mod auth;
pub mod cli;
pub mod cycles;
pub mod doctor;
pub mod error;
pub mod format;
pub mod pricing;
pub mod prompt;
pub mod stats;
pub mod storage;
pub mod time;

use crate::auth::{
    format_auth_profile_entry, format_auth_profile_list, format_auth_status,
    list_codex_auth_profiles, read_codex_auth_status, remove_codex_auth_profile,
    save_current_codex_auth_profile, switch_codex_auth_profile, AuthCommandOptions,
    AuthProfileEntry, AuthProfileListReport,
};
use crate::cli::{
    AuthCliCommand, AuthCliPaths, AuthProfileCliOptions, AuthRemoveCliOptions,
    AuthSelectCliOptions, AuthStatusCliOptions, CliCommand, CycleCliCommand, DoctorCliCommand,
    DoctorCliPaths, ParsedCli, StatCliCommand,
};
use crate::cycles::{
    run_cycle_add, run_cycle_current, run_cycle_history, run_cycle_list, run_cycle_remove,
};
use crate::doctor::{format_doctor_report, read_doctor_report, DoctorOptions};
use crate::error::AppError;
use crate::prompt::{DialoguerPrompt, Prompt};
use crate::stats::run_stat_command;
use chrono::{DateTime, Utc};
use std::env;

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
            CliCommand::Cycle(command) => run_cycle(command),
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

fn run_cycle(command: CycleCliCommand) -> CliResult {
    let result = (|| {
        let now = cli_now()?;
        match command {
            CycleCliCommand::Add {
                time_parts,
                options,
            } => run_cycle_add(&time_parts, options, now),
            CycleCliCommand::List { options } => run_cycle_list(options, now),
            CycleCliCommand::Remove { anchor_id, options } => {
                run_cycle_remove(&anchor_id, options, now)
            }
            CycleCliCommand::Current { options } => run_cycle_current(options, now),
            CycleCliCommand::History { cycle_id, options } => {
                run_cycle_history(cycle_id, options, now)
            }
        }
    })();
    cli_result_from_string(result)
}

fn auth_command_options(paths: &AuthCliPaths) -> AuthCommandOptions {
    AuthCommandOptions {
        auth_file: paths.auth_file.clone(),
        codex_home: paths.codex_home.clone(),
        store_dir: paths.store_dir.clone(),
        account_history_file: paths.account_history_file.clone(),
    }
}

fn doctor_command_options(paths: &DoctorCliPaths) -> DoctorOptions {
    DoctorOptions {
        auth_file: paths.auth_file.clone(),
        codex_home: paths.codex_home.clone(),
        sessions_dir: paths.sessions_dir.clone(),
        cycle_file: paths.cycle_file.clone(),
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
mod tests {
    use super::*;

    #[test]
    fn root_help_lists_top_level_commands() {
        let result = run_cli(["--help"]);

        assert_eq!(result.code, 0);
        assert!(result.stderr.is_empty());
        assert!(result.stdout.contains("Usage: codex-ops"));
        assert!(result.stdout.contains("auth"));
        assert!(result.stdout.contains("doctor"));
        assert!(result.stdout.contains("stat"));
        assert!(result.stdout.contains("cycle"));
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
    fn cycle_leaf_validates_arguments() {
        let result = run_cli(["cycle", "add"]);

        assert_eq!(result.code, 1);
        assert!(result.stdout.is_empty());
        assert!(result
            .stderr
            .contains("cycle add requires at least one weekly cycle start time"));
    }

    #[test]
    fn unknown_command_returns_error() {
        let result = run_cli(["missing"]);

        assert_eq!(result.code, 2);
        assert!(result.stderr.contains("unrecognized subcommand 'missing'"));
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

    fn auth_content(account_id: &str, email: &str, plan: &str) -> String {
        let payload = serde_json::json!({
            "sub": format!("auth0|{account_id}"),
            "email": email,
            "https://api.openai.com/auth": {
                "chatgpt_account_id": account_id,
                "chatgpt_plan_type": plan,
                "chatgpt_user_id": format!("user-{account_id}"),
                "user_id": format!("user-{account_id}")
            }
        });
        let token = jwt(r#"{"alg":"RS256","kid":"key-1"}"#, &payload.to_string());
        serde_json::to_string_pretty(&serde_json::json!({
            "auth_mode": "chatgpt",
            "tokens": {
                "id_token": token,
                "refresh_token": "synthetic-refresh-token",
                "account_id": account_id
            },
            "last_refresh": "2026-05-12T05:32:41.917677755Z"
        }))
        .expect("serialize auth content")
    }

    fn jwt(header: &str, payload: &str) -> String {
        format!(
            "{}.{}.signature",
            encode_base64url(header),
            encode_base64url(payload)
        )
    }

    fn encode_base64url(value: &str) -> String {
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let bytes = value.as_bytes();
        let mut output = String::new();
        let mut index = 0;

        while index < bytes.len() {
            let b0 = bytes[index];
            let b1 = *bytes.get(index + 1).unwrap_or(&0);
            let b2 = *bytes.get(index + 2).unwrap_or(&0);
            output.push(TABLE[(b0 >> 2) as usize] as char);
            output.push(TABLE[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
            if index + 1 < bytes.len() {
                output.push(TABLE[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
            }
            if index + 2 < bytes.len() {
                output.push(TABLE[(b2 & 0x3f) as usize] as char);
            }
            index += 3;
        }

        output
    }

    fn temp_dir(prefix: &str) -> std::path::PathBuf {
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_millis();
        std::env::temp_dir().join(format!("{prefix}-{millis}-{}", std::process::id()))
    }
}
