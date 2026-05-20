mod common;

use common::{assert_contains, assert_json_eq, assert_success, parse_json, run_codex_ops, Sandbox};

#[test]
fn thin_end_to_end_smoke() {
    let sandbox = Sandbox::new();

    let help = run_codex_ops(["--help"], &sandbox);
    assert_success(&help, "root help");
    assert_contains(
        &help.stdout,
        "Usage: codex-ops <command> [options]",
        "root help",
    );

    let auth_status = run_codex_ops(
        [
            "auth",
            "status",
            "--auth-file",
            sandbox.auth_file.to_str().unwrap(),
            "--json",
        ],
        &sandbox,
    );
    assert_success(&auth_status, "auth status json");
    let auth = parse_json(&auth_status.stdout, "auth status json");
    assert_json_eq(
        &auth["summary"]["chatgptAccountId"],
        "account-fixture",
        "auth account id",
    );

    let stat_json = run_codex_ops(
        [
            "stat",
            "--all",
            "--format",
            "json",
            "--sessions-dir",
            sandbox.sessions_dir.to_str().unwrap(),
        ],
        &sandbox,
    );
    assert_success(&stat_json, "stat json");
    let stat = parse_json(&stat_json.stdout, "stat json");
    assert_json_eq(&stat["totals"]["calls"], 3, "stat total calls");

    let cycle_current = run_codex_ops(
        [
            "cycle",
            "current",
            "--cycle-file",
            sandbox.cycle_file.to_str().unwrap(),
            "--account-id",
            "account-fixture",
            "--sessions-dir",
            sandbox.sessions_dir.to_str().unwrap(),
            "--format",
            "json",
        ],
        &sandbox,
    );
    assert_success(&cycle_current, "cycle current json");
    let current = parse_json(&cycle_current.stdout, "cycle current json");
    assert_json_eq(&current["status"], "active", "cycle current status");
}
