mod common;

use common::{
    assert_contains, assert_failure_contains, assert_help, assert_not_contains, assert_success,
    run_codex_ops, Sandbox,
};

#[test]
fn help_is_generated_for_root_and_subcommands() {
    let sandbox = Sandbox::new();

    assert_help(
        &["--help"],
        &[
            "Usage: codex-ops <command> [options]",
            "auth    Show and manage Codex authentication information",
            "limit   Show Codex server rate-limit telemetry",
        ],
        &sandbox,
        "root help",
    );
    let root_help = run_codex_ops(["--help"], &sandbox);
    assert_success(&root_help, "root help no cycle");
    assert_not_contains(&root_help.stdout, "cycle", "root help no cycle");
    assert_help(
        &["auth", "--help"],
        &[
            "Usage: codex-ops auth <command> [options]",
            "select  Activate a persisted auth profile",
            "remove  Remove persisted auth profiles",
        ],
        &sandbox,
        "auth help",
    );
    assert_help(
        &["auth", "status", "--help"],
        &[
            "Usage: codex-ops auth status [options]",
            "--auth-file <path>",
            "--include-token-claims",
        ],
        &sandbox,
        "auth status help",
    );
    assert_help(
        &["auth", "save", "--help"],
        &[
            "Usage: codex-ops auth save [options]",
            "--auth-file <path>",
            "--store-dir <path>",
        ],
        &sandbox,
        "auth save help",
    );
    assert_help(
        &["auth", "list", "--help"],
        &[
            "Usage: codex-ops auth list [options]",
            "--auth-file <path>",
            "--store-dir <path>",
        ],
        &sandbox,
        "auth list help",
    );
    assert_help(
        &["auth", "select", "--help"],
        &[
            "Usage: codex-ops auth select [options]",
            "--account-history-file <path>",
            "-A, --account-id <id>",
        ],
        &sandbox,
        "auth select help",
    );
    assert_help(
        &["auth", "remove", "--help"],
        &[
            "Usage: codex-ops auth remove [options]",
            "-A, --account-id <id>",
            "-y, --yes",
        ],
        &sandbox,
        "auth remove help",
    );
    assert_help(
        &["doctor", "--help"],
        &[
            "Usage: codex-ops doctor [options]",
            "--sessions-dir <path>",
            "-j, --json",
        ],
        &sandbox,
        "doctor help",
    );
    let doctor_help = run_codex_ops(["doctor", "--help"], &sandbox);
    assert_success(&doctor_help, "doctor help no cycle file");
    assert_not_contains(
        &doctor_help.stdout,
        "cycle-file",
        "doctor help no cycle file",
    );
    assert_help(
        &["stat", "--help"],
        &[
            "Usage: codex-ops stat [view] [session] [options]",
            "Arguments:",
            "-g, --group-by <group>",
            "--limit-window <window>",
            "server rate-limit windows",
            "-L, --last <duration>",
        ],
        &sandbox,
        "stat help",
    );
    assert_help(
        &["stat", "sessions", "--help"],
        &[
            "Usage: codex-ops stat [view] [session] [options]",
            "-T, --top <n>",
            "-d, --detail",
        ],
        &sandbox,
        "stat sessions help",
    );
    assert_help(
        &["limit", "--help"],
        &[
            "Usage: codex-ops limit <command> [options]",
            "current  Show latest observed rate-limit state",
            "samples  Export raw rate-limit samples",
        ],
        &sandbox,
        "limit help",
    );
    assert_help(
        &["limit", "trend", "--help"],
        &[
            "Usage: codex-ops limit trend [options]",
            "--window <window>",
            "-f, --format <format>",
        ],
        &sandbox,
        "limit trend help",
    );
    assert_help(
        &["limit", "current", "--help"],
        &[
            "Usage: codex-ops limit current [options]",
            "--sessions-dir <path>",
            "--window <window>",
        ],
        &sandbox,
        "limit current help",
    );
    let current_help = run_codex_ops(["limit", "current", "--help"], &sandbox);
    assert_success(&current_help, "limit current help without ranges");
    assert_not_contains(
        &current_help.stdout,
        "--start",
        "limit current should not expose start option",
    );
    assert_not_contains(
        &current_help.stdout,
        "--last",
        "limit current should not expose last option",
    );
    assert_help(
        &["limit", "windows", "--help"],
        &[
            "Usage: codex-ops limit windows [options]",
            "--window <window>",
            "-f, --format <format>",
        ],
        &sandbox,
        "limit windows help",
    );
    assert_help(
        &["limit", "resets", "--help"],
        &[
            "Usage: codex-ops limit resets [options]",
            "--early-only",
            "--window <window>",
        ],
        &sandbox,
        "limit resets help",
    );
    assert_help(
        &["limit", "samples", "--help"],
        &[
            "Usage: codex-ops limit samples [options]",
            "--window <window>",
            "-j, --json",
        ],
        &sandbox,
        "limit samples help",
    );
}

#[test]
fn stat_limit_window_parser_contract_is_fixed() {
    let sandbox = Sandbox::new();

    let bad_window = run_codex_ops(["stat", "--limit-window", "bogus"], &sandbox);
    assert_failure_contains(&bad_window, 2, "5h", "stat invalid limit window");
    assert_contains(&bad_window.stderr, "7d", "stat invalid limit window");

    let sessions = run_codex_ops(["stat", "sessions", "--limit-window", "7d"], &sandbox);
    assert_failure_contains(
        &sessions,
        2,
        "stat sessions does not support --limit-window",
        "stat sessions rejects limit window",
    );

    let time_group = run_codex_ops(
        ["stat", "--limit-window", "7d", "--group-by", "day"],
        &sandbox,
    );
    assert_failure_contains(
        &time_group,
        2,
        "--group-by model, cwd, or account",
        "stat limit window rejects time group",
    );

    let model_group = run_codex_ops(
        [
            "stat",
            "--limit-window",
            "7d",
            "--group-by",
            "model",
            "--all",
            "--json",
            "--sessions-dir",
            sandbox.sessions_dir.to_str().unwrap(),
        ],
        &sandbox,
    );
    common::assert_success(&model_group, "stat limit window accepts model group");
}

#[test]
fn limit_trend_rejects_removed_group_by_option() {
    let sandbox = Sandbox::new();

    let old_group_by = run_codex_ops(["limit", "trend", "--group-by", "hour"], &sandbox);
    assert_failure_contains(
        &old_group_by,
        2,
        "unexpected argument '--group-by'",
        "limit trend rejects old group-by",
    );
}

#[test]
fn limit_current_rejects_date_range_options() {
    let sandbox = Sandbox::new();

    let start = run_codex_ops(["limit", "current", "--start", "2026-05-01"], &sandbox);
    assert_failure_contains(
        &start,
        2,
        "unexpected argument '--start'",
        "limit current rejects start",
    );

    let last = run_codex_ops(["limit", "current", "--last", "30d"], &sandbox);
    assert_failure_contains(
        &last,
        2,
        "unexpected argument '--last'",
        "limit current rejects last",
    );
}

#[test]
fn parser_errors_keep_stderr_only_contract() {
    let sandbox = Sandbox::new();

    let version = run_codex_ops(["--version"], &sandbox);
    common::assert_success(&version, "root version");
    assert_eq!(
        version.stdout.trim(),
        env!("CARGO_PKG_VERSION"),
        "root version should use Cargo package version"
    );

    let unknown_root = run_codex_ops(["missing"], &sandbox);
    assert_failure_contains(
        &unknown_root,
        2,
        "unrecognized subcommand 'missing'",
        "unknown root command",
    );

    let unknown_auth = run_codex_ops(["auth", "missing"], &sandbox);
    assert_failure_contains(
        &unknown_auth,
        2,
        "unrecognized subcommand 'missing'",
        "unknown auth command",
    );

    let removed_cycle = run_codex_ops(["cycle", "current"], &sandbox);
    assert_failure_contains(
        &removed_cycle,
        2,
        "unrecognized subcommand 'cycle'",
        "removed cycle command",
    );

    let unknown_auth_option = run_codex_ops(["auth", "status", "--bogus"], &sandbox);
    assert_failure_contains(
        &unknown_auth_option,
        2,
        "unexpected argument '--bogus' found",
        "unknown auth option",
    );

    let unknown_doctor_option = run_codex_ops(["doctor", "--bogus"], &sandbox);
    assert_failure_contains(
        &unknown_doctor_option,
        2,
        "unexpected argument '--bogus' found",
        "unknown doctor option",
    );
}
