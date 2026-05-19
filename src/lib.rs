pub mod auth;
pub mod cycles;
pub mod doctor;
pub mod error;
pub mod format;
pub mod pricing;
pub mod stats;
pub mod storage;
pub mod time;

use crate::auth::{
    format_auth_profile_entry, format_auth_profile_list, format_auth_status,
    list_codex_auth_profiles, read_codex_auth_status, remove_codex_auth_profile,
    save_current_codex_auth_profile, switch_codex_auth_profile, AuthCommandOptions,
};
use crate::cycles::{
    run_cycle_add_from_args, run_cycle_current_from_args, run_cycle_history_from_args,
    run_cycle_list_from_args, run_cycle_remove_from_args,
};
use crate::doctor::{format_doctor_report, read_doctor_report, DoctorOptions};
use crate::error::AppError;
use crate::stats::run_stat_command_from_args;
use chrono::{DateTime, Utc};
use std::env;
use std::path::PathBuf;

const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

const ROOT_HELP: &str = "\
Usage: codex-ops <command> [options]

Commands:
  auth      Show and manage Codex authentication information
  doctor    Check local Codex Ops configuration and data
  stat      Show Codex session token usage statistics
  cycle     Manage Codex weekly limit cycle anchors and usage reports

Options:
  -h, --help     Print help
  -V, --version  Print version
";

const AUTH_HELP: &str = "\
Usage: codex-ops auth <command> [options]

Commands:
  status    Decode auth.json and show key claims
  save      Persist the current auth.json by account id
  list      List current and persisted auth profiles
  select    Activate a persisted auth profile
  remove    Remove persisted auth profiles

Options:
  -h, --help  Print help
";

const AUTH_STATUS_HELP: &str = "\
Usage: codex-ops auth status [options]

Options:
  --auth-file <path>          Path to auth.json
  --codex-home <path>         Codex home directory
  -j, --json                  Print JSON
  --include-token-claims      Include decoded JWT header and claims in JSON output
  -h, --help                  Print help
";

const AUTH_SAVE_HELP: &str = "\
Usage: codex-ops auth save [options]

Options:
  --auth-file <path>     Path to auth.json
  --codex-home <path>    Codex home directory
  --store-dir <path>     Auth profile store directory
  -h, --help             Print help
";

const AUTH_LIST_HELP: &str = "\
Usage: codex-ops auth list [options]

Options:
  --auth-file <path>     Path to auth.json
  --codex-home <path>    Codex home directory
  --store-dir <path>     Auth profile store directory
  -h, --help             Print help
";

const AUTH_SELECT_HELP: &str = "\
Usage: codex-ops auth select [options]

Options:
  --auth-file <path>               Path to auth.json
  --codex-home <path>              Codex home directory
  --store-dir <path>               Auth profile store directory
  --account-history-file <path>    Auth account history file
  -A, --account-id <id>            Activate a specific persisted account id
  -h, --help                       Print help
";

const AUTH_REMOVE_HELP: &str = "\
Usage: codex-ops auth remove [options]

Options:
  --auth-file <path>      Path to auth.json
  --codex-home <path>     Codex home directory
  --store-dir <path>      Auth profile store directory
  -A, --account-id <id>   Remove a specific persisted account id
  -y, --yes               Skip confirmation when --account-id is supplied
  -h, --help              Print help
";

const DOCTOR_HELP: &str = "\
Usage: codex-ops doctor [options]

Options:
  --auth-file <path>      Path to auth.json
  --codex-home <path>     Codex home directory
  --sessions-dir <path>   Codex sessions directory
  --cycle-file <path>     Weekly cycle anchor store file
  -j, --json              Print JSON
  -h, --help              Print help
";

const STAT_HELP: &str = "\
Usage: codex-ops stat [view] [session] [options]

Views:
  sessions                 Show top sessions or one session detail

Options:
  -g, --group-by <group>   hour, day, week, month, model, cwd, account
  -S, --sort <sort>        time, tokens, credits, calls, sessions
  -n, --limit <n>          Maximum rows to show
  -T, --top <n>            Number of sessions to show
  -d, --detail             Show full event-level rows
  -F, --full-scan          Scan all session files
  -a, --all                Include all session usage
  -r, --reasoning-effort   Include reasoning effort in model grouping
  -A, --account-id <id>    Only include one account id
  --auth-file <path>       Path to auth.json
  --account-history-file <path>
                             Auth account history file
  --codex-home <path>      Codex home directory
  --sessions-dir <path>    Codex sessions directory
  -s, --start <time>       Start time
  -e, --end <time>         End time
  -t, --today              Use today as the range
  --yesterday              Use yesterday as the range
  -m, --month              Use the current calendar month
  -L, --last <duration>    Recent duration such as 12h, 7d, 2w, 1mo
  -f, --format <format>    table, json, csv, markdown
  -j, --json               Print JSON
  -v, --verbose            Show diagnostics
  -h, --help               Print help
";

const CYCLE_HELP: &str = "\
Usage: codex-ops cycle <command> [options]

Commands:
  add       Add a weekly cycle anchor
  list      List weekly cycle anchors
  remove    Remove a weekly cycle anchor
  current   Show the current weekly cycle
  history   Show weekly cycle history

Options:
  -h, --help  Print help
";

const CYCLE_ADD_HELP: &str = "\
Usage: codex-ops cycle add <time...> [options]

Options:
  -n, --note <text>               Anchor note
  -A, --account-id <id>           Weekly cycle account id
  --auth-file <path>              Path to auth.json
  --codex-home <path>             Codex home directory
  --cycle-file <path>             Weekly cycle anchor store file
  --account-history-file <path>   Auth account history file
  -h, --help                      Print help
";

const CYCLE_LIST_HELP: &str = "\
Usage: codex-ops cycle list [options]

Options:
  -A, --account-id <id>           Weekly cycle account id
  --auth-file <path>              Path to auth.json
  --codex-home <path>             Codex home directory
  --cycle-file <path>             Weekly cycle anchor store file
  --account-history-file <path>   Auth account history file
  -f, --format <format>           table, json, csv, markdown
  -j, --json                      Print JSON
  -h, --help                      Print help
";

const CYCLE_REMOVE_HELP: &str = "\
Usage: codex-ops cycle remove <anchor-id> [options]

Options:
  -A, --account-id <id>           Weekly cycle account id
  --auth-file <path>              Path to auth.json
  --codex-home <path>             Codex home directory
  --cycle-file <path>             Weekly cycle anchor store file
  --account-history-file <path>   Auth account history file
  -h, --help                      Print help
";

const CYCLE_CURRENT_HELP: &str = "\
Usage: codex-ops cycle current [options]

Options:
  -A, --account-id <id>           Weekly cycle account id
  --auth-file <path>              Path to auth.json
  --codex-home <path>             Codex home directory
  --sessions-dir <path>           Codex sessions directory
  --cycle-file <path>             Weekly cycle anchor store file
  --account-history-file <path>   Auth account history file
  -f, --format <format>           table, json, csv, markdown
  -j, --json                      Print JSON
  -h, --help                      Print help
";

const CYCLE_HISTORY_HELP: &str = "\
Usage: codex-ops cycle history [cycle-id] [options]

Options:
  -i, --select                    Interactively select a cycle detail
  --estimate-before-anchor        Include estimated cycles before earliest anchor
  -A, --account-id <id>           Weekly cycle account id
  --auth-file <path>              Path to auth.json
  --codex-home <path>             Codex home directory
  --sessions-dir <path>           Codex sessions directory
  --cycle-file <path>             Weekly cycle anchor store file
  --account-history-file <path>   Auth account history file
  -s, --start <time>              Start time
  -e, --end <time>                End time
  -t, --today                     Use today as the range
  --yesterday                     Use yesterday as the range
  -m, --month                     Use the current calendar month
  -L, --last <duration>           Recent duration such as 12h, 7d, 2w, 1mo
  -a, --all                       Include all session usage
  -f, --format <format>           table, json, csv, markdown
  -j, --json                      Print JSON
  -v, --verbose                   Show diagnostics
  -h, --help                      Print help
";

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

    fn error(stderr: impl Into<String>) -> Self {
        Self {
            code: 2,
            stdout: String::new(),
            stderr: ensure_trailing_newline(stderr.into()),
        }
    }

    fn app_error(error: AppError) -> Self {
        Self {
            code: error.exit_code(),
            stdout: String::new(),
            stderr: ensure_trailing_newline(error.message().to_string()),
        }
    }
}

pub fn run_cli<I, S>(args: I) -> CliResult
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let args: Vec<String> = args.into_iter().map(Into::into).collect();

    match args.first().map(String::as_str) {
        None | Some("-h") | Some("--help") => CliResult::success(ROOT_HELP),
        Some("-V") | Some("--version") => CliResult::success(PACKAGE_VERSION),
        Some("auth") => run_auth(&args[1..]),
        Some("doctor") => run_doctor(&args[1..]),
        Some("stat") => run_stat(&args[1..]),
        Some("cycle") => run_cycle(&args[1..]),
        Some(command) => {
            CliResult::error(format!("error: Unknown command: {command}\n\n{ROOT_HELP}"))
        }
    }
}

fn run_auth(args: &[String]) -> CliResult {
    match args.first().map(String::as_str) {
        None | Some("-h") | Some("--help") => CliResult::success(AUTH_HELP),
        Some("status") => run_auth_status(&args[1..]),
        Some("save") => run_auth_save(&args[1..]),
        Some("list") => run_auth_list(&args[1..]),
        Some("select") => run_auth_select(&args[1..]),
        Some("remove") => run_auth_remove(&args[1..]),
        Some(command) => CliResult::error(format!(
            "error: Unknown auth command: {command}\n\n{AUTH_HELP}"
        )),
    }
}

fn run_auth_status(args: &[String]) -> CliResult {
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        return CliResult::success(AUTH_STATUS_HELP);
    }

    let result = (|| {
        let options = parse_auth_cli_options(args, AUTH_STATUS_HELP)?;
        let report = read_codex_auth_status(&options.auth, cli_now()?)?;
        format_auth_status(&report, options.json, options.include_token_claims)
    })();

    cli_result_from_string(result)
}

fn run_auth_save(args: &[String]) -> CliResult {
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        return CliResult::success(AUTH_SAVE_HELP);
    }

    let result = (|| {
        let options = parse_auth_cli_options(args, AUTH_SAVE_HELP)?;
        let report = save_current_codex_auth_profile(&options.auth, cli_now()?)?;
        Ok(format!(
            "Saved auth profile: {}\nStore: {}\n",
            format_auth_profile_entry(&report.profile),
            report.store_dir
        ))
    })();

    cli_result_from_string(result)
}

fn run_auth_list(args: &[String]) -> CliResult {
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        return CliResult::success(AUTH_LIST_HELP);
    }

    let result = (|| {
        let options = parse_auth_cli_options(args, AUTH_LIST_HELP)?;
        let report = list_codex_auth_profiles(&options.auth, cli_now()?)?;
        Ok(format_auth_profile_list(&report))
    })();

    cli_result_from_string(result)
}

fn run_auth_select(args: &[String]) -> CliResult {
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        return CliResult::success(AUTH_SELECT_HELP);
    }

    let result = (|| {
        let options = parse_auth_cli_options(args, AUTH_SELECT_HELP)?;
        let now = cli_now()?;
        let account_id = options.account_id.clone().ok_or_else(|| {
            AppError::new(
                "auth select requires an interactive terminal unless --account-id is supplied.",
            )
        })?;
        let report = list_codex_auth_profiles(&options.auth, now)?;
        let selected = report
            .stored
            .iter()
            .find(|entry| entry.account_id == account_id)
            .cloned()
            .ok_or_else(|| {
                AppError::new(format!(
                    "No persisted auth profile found for account id: {account_id}"
                ))
            })?;

        if Some(&selected.account_id) == report.current.as_ref().map(|entry| &entry.account_id) {
            return Ok(format!(
                "Auth profile already active: {}\n",
                format_auth_profile_entry(&selected)
            ));
        }

        let switched = switch_codex_auth_profile(&selected.account_id, &options.auth, now)?;
        Ok(format!(
            "Saved current auth profile: {}\nActivated auth profile: {}\n",
            format_auth_profile_entry(&switched.saved_current),
            format_auth_profile_entry(&switched.activated)
        ))
    })();

    cli_result_from_string(result)
}

fn run_auth_remove(args: &[String]) -> CliResult {
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        return CliResult::success(AUTH_REMOVE_HELP);
    }

    let result = (|| {
        let options = parse_auth_cli_options(args, AUTH_REMOVE_HELP)?;
        let now = cli_now()?;
        let report = list_codex_auth_profiles(&options.auth, now)?;
        if report.stored.is_empty() {
            return Ok("No persisted auth profiles.\n".to_string());
        }

        let account_id = options.account_id.clone().ok_or_else(|| {
            AppError::new(
                "auth remove requires an interactive terminal unless --account-id is supplied.",
            )
        })?;
        if !options.yes {
            return Err(AppError::new(
                "auth remove --account-id requires --yes when not running interactively.",
            ));
        }
        let selected = report
            .stored
            .iter()
            .find(|entry| entry.account_id == account_id)
            .ok_or_else(|| {
                AppError::new(format!(
                    "No persisted auth profile found for account id: {account_id}"
                ))
            })?;
        let removed = remove_codex_auth_profile(&selected.account_id, &options.auth, now)?;
        Ok(format!(
            "Removed auth profile: {}\n",
            format_auth_profile_entry(&removed.removed)
        ))
    })();

    cli_result_from_string(result)
}

fn run_doctor(args: &[String]) -> CliResult {
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        return CliResult::success(DOCTOR_HELP);
    }

    let result = (|| {
        let options = parse_doctor_cli_options(args, DOCTOR_HELP)?;
        let report = read_doctor_report(&options.doctor, cli_now()?);
        format_doctor_report(&report, options.json)
    })();

    cli_result_from_string(result)
}

fn run_stat(args: &[String]) -> CliResult {
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        return CliResult::success(STAT_HELP);
    }

    let result = (|| run_stat_command_from_args(args, STAT_HELP, cli_now()?))();
    cli_result_from_string(result)
}

fn run_cycle(args: &[String]) -> CliResult {
    match args.first().map(String::as_str) {
        None | Some("-h") | Some("--help") => CliResult::success(CYCLE_HELP),
        Some("add") => run_cycle_leaf(&args[1..], CYCLE_ADD_HELP, run_cycle_add_from_args),
        Some("list") => run_cycle_leaf(&args[1..], CYCLE_LIST_HELP, run_cycle_list_from_args),
        Some("remove") => run_cycle_leaf(&args[1..], CYCLE_REMOVE_HELP, run_cycle_remove_from_args),
        Some("current") => {
            run_cycle_leaf(&args[1..], CYCLE_CURRENT_HELP, run_cycle_current_from_args)
        }
        Some("history") => {
            run_cycle_leaf(&args[1..], CYCLE_HISTORY_HELP, run_cycle_history_from_args)
        }
        Some(command) => CliResult::error(format!(
            "error: Unknown cycle command: {command}\n\n{CYCLE_HELP}"
        )),
    }
}

fn run_cycle_leaf(
    args: &[String],
    help: &str,
    handler: fn(&[String], &str, DateTime<Utc>) -> Result<String, AppError>,
) -> CliResult {
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        return CliResult::success(help);
    }

    let result = (|| {
        let now = cli_now()?;
        handler(args, help, now)
    })();
    cli_result_from_string(result)
}

#[derive(Default)]
struct AuthCliOptions {
    auth: AuthCommandOptions,
    json: bool,
    include_token_claims: bool,
    account_id: Option<String>,
    yes: bool,
}

#[derive(Default)]
struct DoctorCliOptions {
    doctor: DoctorOptions,
    json: bool,
}

fn parse_auth_cli_options(args: &[String], help: &str) -> Result<AuthCliOptions, AppError> {
    let mut options = AuthCliOptions::default();
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--auth-file" => {
                options.auth.auth_file = Some(read_path_value(args, &mut index, "--auth-file")?);
            }
            "--codex-home" => {
                options.auth.codex_home =
                    Some(read_raw_path_value(args, &mut index, "--codex-home")?);
            }
            "--store-dir" => {
                options.auth.store_dir = Some(read_path_value(args, &mut index, "--store-dir")?);
            }
            "--account-history-file" => {
                options.auth.account_history_file =
                    Some(read_path_value(args, &mut index, "--account-history-file")?);
            }
            "-A" | "--account-id" => {
                options.account_id = Some(read_string_value(args, &mut index, "--account-id")?);
            }
            "-j" | "--json" => options.json = true,
            "--include-token-claims" => options.include_token_claims = true,
            "-y" | "--yes" => options.yes = true,
            unknown if unknown.starts_with('-') => {
                return Err(AppError::invalid_input(format!(
                    "error: Unknown option: {unknown}\n\n{help}"
                )));
            }
            unexpected => {
                return Err(AppError::invalid_input(format!(
                    "error: Unexpected argument: {unexpected}\n\n{help}"
                )));
            }
        }

        index += 1;
    }

    Ok(options)
}

fn parse_doctor_cli_options(args: &[String], help: &str) -> Result<DoctorCliOptions, AppError> {
    let mut options = DoctorCliOptions::default();
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--auth-file" => {
                options.doctor.auth_file = Some(read_path_value(args, &mut index, "--auth-file")?);
            }
            "--codex-home" => {
                options.doctor.codex_home =
                    Some(read_raw_path_value(args, &mut index, "--codex-home")?);
            }
            "--sessions-dir" => {
                options.doctor.sessions_dir =
                    Some(read_raw_path_value(args, &mut index, "--sessions-dir")?);
            }
            "--cycle-file" => {
                options.doctor.cycle_file =
                    Some(read_path_value(args, &mut index, "--cycle-file")?);
            }
            "-j" | "--json" => options.json = true,
            unknown if unknown.starts_with('-') => {
                return Err(AppError::invalid_input(format!(
                    "error: Unknown option: {unknown}\n\n{help}"
                )));
            }
            unexpected => {
                return Err(AppError::invalid_input(format!(
                    "error: Unexpected argument: {unexpected}\n\n{help}"
                )));
            }
        }

        index += 1;
    }

    Ok(options)
}

fn read_string_value(args: &[String], index: &mut usize, name: &str) -> Result<String, AppError> {
    *index += 1;
    args.get(*index)
        .cloned()
        .filter(|value| !value.starts_with("--"))
        .ok_or_else(|| AppError::invalid_input(format!("error: Missing value for {name}")))
}

fn read_path_value(args: &[String], index: &mut usize, name: &str) -> Result<PathBuf, AppError> {
    let value = read_string_value(args, index, name)?;
    let path = PathBuf::from(value);
    if path.is_absolute() {
        Ok(path)
    } else {
        env::current_dir()
            .map(|cwd| cwd.join(path))
            .map_err(|error| AppError::new(error.to_string()))
    }
}

fn read_raw_path_value(
    args: &[String],
    index: &mut usize,
    name: &str,
) -> Result<PathBuf, AppError> {
    Ok(PathBuf::from(read_string_value(args, index, name)?))
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
        assert!(result.stderr.contains("Unknown command: missing"));
    }
}
