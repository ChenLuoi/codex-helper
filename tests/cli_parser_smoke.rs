mod common;

use common::{assert_failure_contains, assert_help, run_codex_ops, Sandbox};

#[test]
fn help_is_generated_for_root_and_subcommands() {
    let sandbox = Sandbox::new();

    assert_help(
        &["--help"],
        &[
            "Usage: codex-ops <command> [options]",
            "auth    Show and manage Codex authentication information",
            "cycle   Manage Codex weekly limit cycle anchors and usage reports",
        ],
        &sandbox,
        "root help",
    );
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
    assert_help(
        &["stat", "--help"],
        &[
            "Usage: codex-ops stat [view] [session] [options]",
            "Arguments:",
            "-g, --group-by <group>",
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
        &["cycle", "--help"],
        &[
            "Usage: codex-ops cycle <command> [options]",
            "current  Show the current weekly cycle",
            "history  Show weekly cycle history",
        ],
        &sandbox,
        "cycle help",
    );
    assert_help(
        &["cycle", "add", "--help"],
        &[
            "Usage: codex-ops cycle add <time...> [options]",
            "-n, --note <text>",
            "--account-history-file <path>",
        ],
        &sandbox,
        "cycle add help",
    );
    assert_help(
        &["cycle", "list", "--help"],
        &[
            "Usage: codex-ops cycle list [options]",
            "-f, --format <format>",
            "-j, --json",
        ],
        &sandbox,
        "cycle list help",
    );
    assert_help(
        &["cycle", "remove", "--help"],
        &[
            "Usage: codex-ops cycle remove <anchor-id> [options]",
            "<anchor-id>",
            "--cycle-file <path>",
        ],
        &sandbox,
        "cycle remove help",
    );
    assert_help(
        &["cycle", "current", "--help"],
        &[
            "Usage: codex-ops cycle current [options]",
            "--sessions-dir <path>",
            "-f, --format <format>",
        ],
        &sandbox,
        "cycle current help",
    );
    assert_help(
        &["cycle", "history", "--help"],
        &[
            "Usage: codex-ops cycle history [cycle-id] [options]",
            "-i, --select",
            "--estimate-before-anchor",
        ],
        &sandbox,
        "cycle history help",
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
