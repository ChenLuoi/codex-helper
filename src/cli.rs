use crate::cycles::CycleCommandOptions;
use crate::stats::StatCommandOptions;
use clap::error::ErrorKind;
use clap::{Args, CommandFactory, Parser, Subcommand};
use std::env;
use std::path::PathBuf;

const ROOT_USAGE: &str = "codex-ops <command> [options]";
const AUTH_USAGE: &str = "codex-ops auth <command> [options]";
const AUTH_STATUS_USAGE: &str = "codex-ops auth status [options]";
const AUTH_SAVE_USAGE: &str = "codex-ops auth save [options]";
const AUTH_LIST_USAGE: &str = "codex-ops auth list [options]";
const AUTH_SELECT_USAGE: &str = "codex-ops auth select [options]";
const AUTH_REMOVE_USAGE: &str = "codex-ops auth remove [options]";
const DOCTOR_USAGE: &str = "codex-ops doctor [options]";

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ParsedCli {
    Command(Box<CliCommand>),
    Help(String),
    Version,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum CliCommand {
    Auth(AuthCliCommand),
    Doctor(DoctorCliCommand),
    Stat(StatCliCommand),
    Cycle(CycleCliCommand),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum AuthCliCommand {
    Status(AuthStatusCliOptions),
    Save(AuthProfileCliOptions),
    List(AuthProfileCliOptions),
    Select(AuthSelectCliOptions),
    Remove(AuthRemoveCliOptions),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum DoctorCliCommand {
    Run(DoctorCliOptions),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct StatCliCommand {
    pub view: Option<String>,
    pub session: Option<String>,
    pub options: StatCommandOptions,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum CycleCliCommand {
    Add {
        time_parts: Vec<String>,
        options: CycleCommandOptions,
    },
    List {
        options: CycleCommandOptions,
    },
    Remove {
        anchor_id: String,
        options: CycleCommandOptions,
    },
    Current {
        options: CycleCommandOptions,
    },
    History {
        cycle_id: Option<String>,
        options: CycleCommandOptions,
    },
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct AuthCliPaths {
    pub auth_file: Option<PathBuf>,
    pub codex_home: Option<PathBuf>,
    pub store_dir: Option<PathBuf>,
    pub account_history_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AuthStatusCliOptions {
    pub paths: AuthCliPaths,
    pub json: bool,
    pub include_token_claims: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AuthProfileCliOptions {
    pub paths: AuthCliPaths,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AuthSelectCliOptions {
    pub paths: AuthCliPaths,
    pub account_id: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AuthRemoveCliOptions {
    pub paths: AuthCliPaths,
    pub account_id: Option<String>,
    pub yes: bool,
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct DoctorCliPaths {
    pub auth_file: Option<PathBuf>,
    pub codex_home: Option<PathBuf>,
    pub sessions_dir: Option<PathBuf>,
    pub cycle_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DoctorCliOptions {
    pub paths: DoctorCliPaths,
    pub json: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CliParseError {
    pub code: i32,
    pub message: String,
}

impl CliParseError {
    fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "codex-ops",
    disable_help_subcommand = true,
    disable_version_flag = true,
    override_usage = ROOT_USAGE,
    color = clap::ColorChoice::Never
)]
struct CliArgs {
    #[arg(short = 'V', long, help = "Print version")]
    version: bool,

    #[command(subcommand)]
    command: Option<RootCommand>,
}

#[derive(Debug, Subcommand)]
enum RootCommand {
    #[command(
        about = "Show and manage Codex authentication information",
        override_usage = AUTH_USAGE
    )]
    Auth(AuthArgs),
    #[command(
        about = "Check local Codex Ops configuration and data",
        override_usage = DOCTOR_USAGE
    )]
    Doctor(DoctorArgs),
    #[command(
        about = "Show Codex session token usage statistics",
        override_usage = "codex-ops stat [view] [session] [options]"
    )]
    Stat(StatArgs),
    #[command(
        about = "Manage Codex weekly limit cycle anchors and usage reports",
        override_usage = "codex-ops cycle <command> [options]"
    )]
    Cycle(CycleArgs),
}

#[derive(Debug, Args)]
struct AuthArgs {
    #[command(subcommand)]
    command: Option<AuthCommand>,
}

#[derive(Debug, Subcommand)]
enum AuthCommand {
    #[command(
        about = "Decode auth.json and show key claims",
        override_usage = AUTH_STATUS_USAGE
    )]
    Status(AuthStatusArgs),
    #[command(
        about = "Persist the current auth.json by account id",
        override_usage = AUTH_SAVE_USAGE
    )]
    Save(AuthProfileArgs),
    #[command(
        about = "List current and persisted auth profiles",
        override_usage = AUTH_LIST_USAGE
    )]
    List(AuthProfileArgs),
    #[command(
        about = "Activate a persisted auth profile",
        override_usage = AUTH_SELECT_USAGE
    )]
    Select(AuthSelectArgs),
    #[command(
        about = "Remove persisted auth profiles",
        override_usage = AUTH_REMOVE_USAGE
    )]
    Remove(AuthRemoveArgs),
}

#[derive(Debug, Args)]
struct AuthStatusArgs {
    #[arg(long, value_name = "path", help = "Path to auth.json")]
    auth_file: Option<PathBuf>,
    #[arg(long, value_name = "path", help = "Codex home directory")]
    codex_home: Option<PathBuf>,
    #[arg(short = 'j', long, help = "Print JSON")]
    json: bool,
    #[arg(long, help = "Include decoded JWT header and claims in JSON output")]
    include_token_claims: bool,
}

#[derive(Debug, Args)]
struct AuthProfileArgs {
    #[arg(long, value_name = "path", help = "Path to auth.json")]
    auth_file: Option<PathBuf>,
    #[arg(long, value_name = "path", help = "Codex home directory")]
    codex_home: Option<PathBuf>,
    #[arg(long, value_name = "path", help = "Auth profile store directory")]
    store_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct AuthSelectArgs {
    #[arg(long, value_name = "path", help = "Path to auth.json")]
    auth_file: Option<PathBuf>,
    #[arg(long, value_name = "path", help = "Codex home directory")]
    codex_home: Option<PathBuf>,
    #[arg(long, value_name = "path", help = "Auth profile store directory")]
    store_dir: Option<PathBuf>,
    #[arg(long, value_name = "path", help = "Auth account history file")]
    account_history_file: Option<PathBuf>,
    #[arg(
        short = 'A',
        long,
        value_name = "id",
        help = "Activate a specific persisted account id"
    )]
    account_id: Option<String>,
}

#[derive(Debug, Args)]
struct AuthRemoveArgs {
    #[arg(long, value_name = "path", help = "Path to auth.json")]
    auth_file: Option<PathBuf>,
    #[arg(long, value_name = "path", help = "Codex home directory")]
    codex_home: Option<PathBuf>,
    #[arg(long, value_name = "path", help = "Auth profile store directory")]
    store_dir: Option<PathBuf>,
    #[arg(
        short = 'A',
        long,
        value_name = "id",
        help = "Remove a specific persisted account id"
    )]
    account_id: Option<String>,
    #[arg(
        short = 'y',
        long,
        help = "Skip confirmation when --account-id is supplied"
    )]
    yes: bool,
}

#[derive(Debug, Args)]
struct DoctorArgs {
    #[arg(long, value_name = "path", help = "Path to auth.json")]
    auth_file: Option<PathBuf>,
    #[arg(long, value_name = "path", help = "Codex home directory")]
    codex_home: Option<PathBuf>,
    #[arg(long, value_name = "path", help = "Codex sessions directory")]
    sessions_dir: Option<PathBuf>,
    #[arg(long, value_name = "path", help = "Weekly cycle anchor store file")]
    cycle_file: Option<PathBuf>,
    #[arg(short = 'j', long, help = "Print JSON")]
    json: bool,
}

#[derive(Debug, Args)]
struct StatArgs {
    #[arg(value_name = "view")]
    view: Option<String>,
    #[arg(value_name = "session")]
    session: Option<String>,
    #[command(flatten)]
    options: StatOptionArgs,
}

#[derive(Debug, Args)]
struct StatOptionArgs {
    #[arg(
        short = 'g',
        long,
        value_name = "group",
        help = "hour, day, week, month, model, cwd, account"
    )]
    group_by: Option<String>,
    #[arg(
        short = 'S',
        long,
        value_name = "sort",
        help = "time, tokens, credits, calls, sessions"
    )]
    sort: Option<String>,
    #[arg(short = 'n', long, value_name = "n", help = "Maximum rows to show")]
    limit: Option<String>,
    #[arg(
        short = 'T',
        long,
        value_name = "n",
        help = "Number of sessions to show"
    )]
    top: Option<String>,
    #[arg(short = 'd', long, help = "Show full event-level rows")]
    detail: bool,
    #[arg(short = 'F', long, help = "Scan all session files")]
    full_scan: bool,
    #[arg(short = 'a', long, help = "Include all session usage")]
    all: bool,
    #[arg(short = 'r', long, help = "Include reasoning effort in model grouping")]
    reasoning_effort: bool,
    #[arg(
        short = 'A',
        long,
        value_name = "id",
        help = "Only include one account id"
    )]
    account_id: Option<String>,
    #[arg(long, value_name = "path", help = "Path to auth.json")]
    auth_file: Option<PathBuf>,
    #[arg(long, value_name = "path", help = "Auth account history file")]
    account_history_file: Option<PathBuf>,
    #[arg(long, value_name = "path", help = "Codex home directory")]
    codex_home: Option<PathBuf>,
    #[arg(long, value_name = "path", help = "Codex sessions directory")]
    sessions_dir: Option<PathBuf>,
    #[arg(short = 's', long, value_name = "time", help = "Start time")]
    start: Option<String>,
    #[arg(short = 'e', long, value_name = "time", help = "End time")]
    end: Option<String>,
    #[arg(short = 't', long, help = "Use today as the range")]
    today: bool,
    #[arg(long, help = "Use yesterday as the range")]
    yesterday: bool,
    #[arg(short = 'm', long, help = "Use the current calendar month")]
    month: bool,
    #[arg(
        short = 'L',
        long,
        value_name = "duration",
        help = "Recent duration such as 12h, 7d, 2w, 1mo"
    )]
    last: Option<String>,
    #[arg(
        short = 'f',
        long,
        value_name = "format",
        help = "table, json, csv, markdown"
    )]
    format: Option<String>,
    #[arg(short = 'j', long, help = "Print JSON")]
    json: bool,
    #[arg(short = 'v', long, help = "Show diagnostics")]
    verbose: bool,
}

#[derive(Debug, Args)]
struct CycleArgs {
    #[command(subcommand)]
    command: Option<CycleSubcommand>,
}

#[derive(Debug, Subcommand)]
enum CycleSubcommand {
    #[command(
        about = "Add a weekly cycle anchor",
        override_usage = "codex-ops cycle add <time...> [options]"
    )]
    Add(CycleAddArgs),
    #[command(
        about = "List weekly cycle anchors",
        override_usage = "codex-ops cycle list [options]"
    )]
    List(CycleListArgs),
    #[command(
        about = "Remove a weekly cycle anchor",
        override_usage = "codex-ops cycle remove <anchor-id> [options]"
    )]
    Remove(CycleRemoveArgs),
    #[command(
        about = "Show the current weekly cycle",
        override_usage = "codex-ops cycle current [options]"
    )]
    Current(CycleCurrentArgs),
    #[command(
        about = "Show weekly cycle history",
        override_usage = "codex-ops cycle history [cycle-id] [options]"
    )]
    History(CycleHistoryArgs),
}

#[derive(Debug, Args)]
struct CycleAddArgs {
    #[arg(value_name = "time")]
    time_parts: Vec<String>,
    #[arg(short = 'n', long, value_name = "text", help = "Anchor note")]
    note: Option<String>,
    #[command(flatten)]
    account: CycleAccountArgs,
}

#[derive(Debug, Args)]
struct CycleListArgs {
    #[command(flatten)]
    account: CycleAccountArgs,
    #[command(flatten)]
    format: CycleFormatArgs,
}

#[derive(Debug, Args)]
struct CycleRemoveArgs {
    #[arg(value_name = "anchor-id")]
    anchor_id: String,
    #[command(flatten)]
    account: CycleAccountArgs,
}

#[derive(Debug, Args)]
struct CycleCurrentArgs {
    #[command(flatten)]
    account: CycleAccountArgs,
    #[arg(long, value_name = "path", help = "Codex sessions directory")]
    sessions_dir: Option<PathBuf>,
    #[command(flatten)]
    format: CycleFormatArgs,
}

#[derive(Debug, Args)]
struct CycleHistoryArgs {
    #[arg(value_name = "cycle-id")]
    cycle_id: Option<String>,
    #[arg(short = 'i', long, help = "Interactively select a cycle detail")]
    select: bool,
    #[arg(long, help = "Include estimated cycles before earliest anchor")]
    estimate_before_anchor: bool,
    #[command(flatten)]
    account: CycleAccountArgs,
    #[arg(long, value_name = "path", help = "Codex sessions directory")]
    sessions_dir: Option<PathBuf>,
    #[arg(short = 's', long, value_name = "time", help = "Start time")]
    start: Option<String>,
    #[arg(short = 'e', long, value_name = "time", help = "End time")]
    end: Option<String>,
    #[arg(short = 't', long, help = "Use today as the range")]
    today: bool,
    #[arg(long, help = "Use yesterday as the range")]
    yesterday: bool,
    #[arg(short = 'm', long, help = "Use the current calendar month")]
    month: bool,
    #[arg(
        short = 'L',
        long,
        value_name = "duration",
        help = "Recent duration such as 12h, 7d, 2w, 1mo"
    )]
    last: Option<String>,
    #[arg(short = 'a', long, help = "Include all session usage")]
    all: bool,
    #[command(flatten)]
    format: CycleFormatArgs,
    #[arg(short = 'v', long, help = "Show diagnostics")]
    verbose: bool,
}

#[derive(Debug, Args)]
struct CycleAccountArgs {
    #[arg(short = 'A', long, value_name = "id", help = "Weekly cycle account id")]
    account_id: Option<String>,
    #[arg(long, value_name = "path", help = "Path to auth.json")]
    auth_file: Option<PathBuf>,
    #[arg(long, value_name = "path", help = "Codex home directory")]
    codex_home: Option<PathBuf>,
    #[arg(long, value_name = "path", help = "Weekly cycle anchor store file")]
    cycle_file: Option<PathBuf>,
    #[arg(long, value_name = "path", help = "Auth account history file")]
    account_history_file: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct CycleFormatArgs {
    #[arg(
        short = 'f',
        long,
        value_name = "format",
        help = "table, json, csv, markdown"
    )]
    format: Option<String>,
    #[arg(short = 'j', long, help = "Print JSON")]
    json: bool,
}

pub fn parse_cli(args: &[String]) -> Result<ParsedCli, CliParseError> {
    let argv = std::iter::once("codex-ops".to_string()).chain(args.iter().cloned());
    match CliArgs::try_parse_from(argv) {
        Ok(parsed) => parsed.into_parsed_cli(),
        Err(error) => match error.kind() {
            ErrorKind::DisplayHelp => Ok(ParsedCli::Help(error.to_string())),
            ErrorKind::DisplayVersion => Ok(ParsedCli::Version),
            _ => Err(CliParseError::new(error.exit_code(), error.to_string())),
        },
    }
}

impl CliArgs {
    fn into_parsed_cli(self) -> Result<ParsedCli, CliParseError> {
        if self.version {
            return Ok(ParsedCli::Version);
        }

        match self.command {
            None => Ok(ParsedCli::Help(root_help())),
            Some(RootCommand::Auth(args)) => auth_command(args),
            Some(RootCommand::Doctor(args)) => Ok(parsed_command(CliCommand::Doctor(
                DoctorCliCommand::Run(doctor_options(args)?),
            ))),
            Some(RootCommand::Stat(args)) => {
                Ok(parsed_command(CliCommand::Stat(stat_command(args)?)))
            }
            Some(RootCommand::Cycle(args)) => cycle_command(args),
        }
    }
}

fn auth_command(args: AuthArgs) -> Result<ParsedCli, CliParseError> {
    let command = match args.command {
        None => return Ok(ParsedCli::Help(auth_help())),
        Some(AuthCommand::Status(args)) => AuthCliCommand::Status(AuthStatusCliOptions {
            paths: AuthCliPaths {
                auth_file: resolve_cli_path(args.auth_file)?,
                codex_home: args.codex_home,
                ..AuthCliPaths::default()
            },
            json: args.json,
            include_token_claims: args.include_token_claims,
        }),
        Some(AuthCommand::Save(args)) => AuthCliCommand::Save(AuthProfileCliOptions {
            paths: auth_profile_paths(args)?,
        }),
        Some(AuthCommand::List(args)) => AuthCliCommand::List(AuthProfileCliOptions {
            paths: auth_profile_paths(args)?,
        }),
        Some(AuthCommand::Select(args)) => AuthCliCommand::Select(AuthSelectCliOptions {
            paths: AuthCliPaths {
                auth_file: resolve_cli_path(args.auth_file)?,
                codex_home: args.codex_home,
                store_dir: resolve_cli_path(args.store_dir)?,
                account_history_file: resolve_cli_path(args.account_history_file)?,
            },
            account_id: args.account_id,
        }),
        Some(AuthCommand::Remove(args)) => AuthCliCommand::Remove(AuthRemoveCliOptions {
            paths: AuthCliPaths {
                auth_file: resolve_cli_path(args.auth_file)?,
                codex_home: args.codex_home,
                store_dir: resolve_cli_path(args.store_dir)?,
                account_history_file: None,
            },
            account_id: args.account_id,
            yes: args.yes,
        }),
    };

    Ok(parsed_command(CliCommand::Auth(command)))
}

fn auth_profile_paths(args: AuthProfileArgs) -> Result<AuthCliPaths, CliParseError> {
    Ok(AuthCliPaths {
        auth_file: resolve_cli_path(args.auth_file)?,
        codex_home: args.codex_home,
        store_dir: resolve_cli_path(args.store_dir)?,
        account_history_file: None,
    })
}

fn doctor_options(args: DoctorArgs) -> Result<DoctorCliOptions, CliParseError> {
    Ok(DoctorCliOptions {
        paths: DoctorCliPaths {
            auth_file: resolve_cli_path(args.auth_file)?,
            codex_home: args.codex_home,
            sessions_dir: args.sessions_dir,
            cycle_file: resolve_cli_path(args.cycle_file)?,
        },
        json: args.json,
    })
}

fn stat_command(args: StatArgs) -> Result<StatCliCommand, CliParseError> {
    Ok(StatCliCommand {
        view: args.view,
        session: args.session,
        options: stat_options(args.options)?,
    })
}

fn stat_options(args: StatOptionArgs) -> Result<StatCommandOptions, CliParseError> {
    Ok(StatCommandOptions {
        start: args.start,
        end: args.end,
        group_by: args.group_by,
        format: args.format,
        codex_home: args.codex_home,
        sessions_dir: args.sessions_dir,
        auth_file: resolve_cli_path(args.auth_file)?,
        account_history_file: resolve_cli_path(args.account_history_file)?,
        today: args.today,
        yesterday: args.yesterday,
        month: args.month,
        all: args.all,
        reasoning_effort: args.reasoning_effort,
        account_id: args.account_id,
        last: args.last,
        sort: args.sort,
        limit: args.limit,
        top: args.top,
        detail: args.detail,
        full_scan: args.full_scan,
        verbose: args.verbose,
        json: args.json,
    })
}

fn cycle_command(args: CycleArgs) -> Result<ParsedCli, CliParseError> {
    let command = match args.command {
        None => return Ok(ParsedCli::Help(cycle_help())),
        Some(CycleSubcommand::Add(args)) => CycleCliCommand::Add {
            time_parts: args.time_parts,
            options: cycle_options(args.account, None, None, None, args.note, None, None)?,
        },
        Some(CycleSubcommand::List(args)) => CycleCliCommand::List {
            options: cycle_options(
                args.account,
                None,
                None,
                Some(args.format),
                None,
                None,
                None,
            )?,
        },
        Some(CycleSubcommand::Remove(args)) => CycleCliCommand::Remove {
            anchor_id: args.anchor_id,
            options: cycle_options(args.account, None, None, None, None, None, None)?,
        },
        Some(CycleSubcommand::Current(args)) => CycleCliCommand::Current {
            options: cycle_options(
                args.account,
                args.sessions_dir,
                None,
                Some(args.format),
                None,
                None,
                None,
            )?,
        },
        Some(CycleSubcommand::History(args)) => CycleCliCommand::History {
            cycle_id: args.cycle_id,
            options: cycle_options(
                args.account,
                args.sessions_dir,
                Some(CycleHistoryRangeArgs {
                    start: args.start,
                    end: args.end,
                    today: args.today,
                    yesterday: args.yesterday,
                    month: args.month,
                    last: args.last,
                    all: args.all,
                    verbose: args.verbose,
                }),
                Some(args.format),
                None,
                Some(args.select),
                Some(args.estimate_before_anchor),
            )?,
        },
    };

    Ok(parsed_command(CliCommand::Cycle(command)))
}

struct CycleHistoryRangeArgs {
    start: Option<String>,
    end: Option<String>,
    today: bool,
    yesterday: bool,
    month: bool,
    last: Option<String>,
    all: bool,
    verbose: bool,
}

fn cycle_options(
    account: CycleAccountArgs,
    sessions_dir: Option<PathBuf>,
    history: Option<CycleHistoryRangeArgs>,
    format: Option<CycleFormatArgs>,
    note: Option<String>,
    select: Option<bool>,
    estimate_before_anchor: Option<bool>,
) -> Result<CycleCommandOptions, CliParseError> {
    let auth_file = resolve_cli_path(account.auth_file)?;
    let cycle_file = resolve_cli_path(account.cycle_file)?;
    let account_history_file = resolve_cli_path(account.account_history_file)?;
    let mut stat = StatCommandOptions {
        auth_file: auth_file.clone(),
        codex_home: account.codex_home.clone(),
        sessions_dir: sessions_dir.clone(),
        account_history_file: account_history_file.clone(),
        account_id: account.account_id.clone(),
        ..StatCommandOptions::default()
    };

    if let Some(history) = history {
        stat.start = history.start;
        stat.end = history.end;
        stat.today = history.today;
        stat.yesterday = history.yesterday;
        stat.month = history.month;
        stat.last = history.last;
        stat.all = history.all;
        stat.verbose = history.verbose;
    }

    let (format, json) = format
        .map(|format| (format.format, format.json))
        .unwrap_or((None, false));
    stat.json = json;

    Ok(CycleCommandOptions {
        auth_file,
        codex_home: account.codex_home,
        cycle_file,
        account_history_file,
        sessions_dir,
        account_id: account.account_id,
        note,
        format,
        json,
        select: select.unwrap_or(false),
        estimate_before_anchor: estimate_before_anchor.unwrap_or(false),
        stat,
    })
}

fn resolve_cli_path(path: Option<PathBuf>) -> Result<Option<PathBuf>, CliParseError> {
    match path {
        None => Ok(None),
        Some(path) if path.is_absolute() => Ok(Some(path)),
        Some(path) => env::current_dir()
            .map(|cwd| Some(cwd.join(path)))
            .map_err(|error| CliParseError::new(1, error.to_string())),
    }
}

fn parsed_command(command: CliCommand) -> ParsedCli {
    ParsedCli::Command(Box::new(command))
}

fn root_help() -> String {
    let mut command = CliArgs::command();
    command.render_help().to_string()
}

fn auth_help() -> String {
    let mut command = CliArgs::command();
    let auth = command
        .find_subcommand_mut("auth")
        .expect("auth subcommand is defined");
    auth.render_help().to_string()
}

fn cycle_help() -> String {
    let mut command = CliArgs::command();
    let cycle = command
        .find_subcommand_mut("cycle")
        .expect("cycle subcommand is defined");
    cycle.render_help().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_path_options_from_space_separated_flags() {
        let args = vec![
            "auth".to_string(),
            "status".to_string(),
            "--auth-file".to_string(),
            "fixtures/auth.json".to_string(),
        ];

        let parsed = parse_cli(&args).expect("parse cli");
        let ParsedCli::Command(command) = parsed else {
            panic!("expected command");
        };
        let CliCommand::Auth(AuthCliCommand::Status(options)) = *command else {
            panic!("expected auth status");
        };

        assert_eq!(
            options.paths.auth_file,
            Some(env::current_dir().expect("cwd").join("fixtures/auth.json"))
        );
    }

    #[test]
    fn resolves_path_options_from_equals_flags() {
        let args = vec![
            "cycle".to_string(),
            "history".to_string(),
            "--cycle-file=fixtures/cycles.json".to_string(),
            "--account-history-file=fixtures/history.json".to_string(),
        ];

        let parsed = parse_cli(&args).expect("parse cli");
        let ParsedCli::Command(command) = parsed else {
            panic!("expected command");
        };
        let CliCommand::Cycle(CycleCliCommand::History { options, .. }) = *command else {
            panic!("expected cycle history");
        };
        let cwd = env::current_dir().expect("cwd");

        assert_eq!(options.cycle_file, Some(cwd.join("fixtures/cycles.json")));
        assert_eq!(
            options.account_history_file,
            Some(cwd.join("fixtures/history.json"))
        );
    }
}
