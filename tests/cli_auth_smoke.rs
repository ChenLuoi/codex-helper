mod common;

use common::{
    assert_contains, assert_failure_contains, assert_json_eq, assert_no_secrets, assert_success,
    parse_json, read_file_bytes, run_codex_ops, Sandbox,
};

#[test]
fn auth_status_save_and_list_redact_secrets() {
    let sandbox = Sandbox::new();

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
    assert_no_secrets(&auth_status, "auth status json");
    let auth = parse_json(&auth_status.stdout, "auth status json");
    assert_json_eq(
        &auth["summary"]["chatgptAccountId"],
        "account-fixture",
        "auth account id",
    );
    assert_json_eq(
        &auth["summary"]["email"],
        "fixture@example.test",
        "auth email",
    );
    assert_json_eq(&auth["tokenClaimsIncluded"], false, "auth claims default");

    let auth_save = run_codex_ops(
        [
            "auth",
            "save",
            "--auth-file",
            sandbox.auth_file.to_str().unwrap(),
            "--store-dir",
            sandbox.store_dir.to_str().unwrap(),
        ],
        &sandbox,
    );
    assert_success(&auth_save, "auth save");
    assert_contains(
        &auth_save.stdout,
        "Saved auth profile: fixture@example.test(account-fixture) - pro",
        "auth save",
    );
    assert!(
        sandbox.store_dir.join("account-fixture.json").exists(),
        "auth save did not write current profile"
    );

    let auth_list = run_codex_ops(
        [
            "auth",
            "list",
            "--auth-file",
            sandbox.auth_file.to_str().unwrap(),
            "--store-dir",
            sandbox.store_dir.to_str().unwrap(),
        ],
        &sandbox,
    );
    assert_success(&auth_list, "auth list");
    assert_no_secrets(&auth_list, "auth list");
    assert_contains(
        &auth_list.stdout,
        "Current: fixture@example.test(account-fixture) - pro",
        "auth list current",
    );
    assert_contains(
        &auth_list.stdout,
        "other@example.test(account-other) - plus",
        "auth list persisted",
    );
}

#[test]
fn auth_non_tty_negative_paths_do_not_mutate_files() {
    let sandbox = Sandbox::new();
    let auth_file_before = read_file_bytes(&sandbox.auth_file);
    let account_history_before = read_file_bytes(&sandbox.account_history_file);
    let stored_profile = sandbox.store_dir.join("account-other.json");
    let stored_profile_before = read_file_bytes(&stored_profile);

    let auth_select = run_codex_ops(
        [
            "auth",
            "select",
            "--auth-file",
            sandbox.auth_file.to_str().unwrap(),
            "--store-dir",
            sandbox.store_dir.to_str().unwrap(),
            "--account-history-file",
            sandbox.account_history_file.to_str().unwrap(),
        ],
        &sandbox,
    );
    assert_failure_contains(
        &auth_select,
        1,
        "auth select requires an interactive terminal unless --account-id is supplied.",
        "auth select non-tty",
    );

    let auth_remove = run_codex_ops(
        [
            "auth",
            "remove",
            "--auth-file",
            sandbox.auth_file.to_str().unwrap(),
            "--store-dir",
            sandbox.store_dir.to_str().unwrap(),
        ],
        &sandbox,
    );
    assert_failure_contains(
        &auth_remove,
        1,
        "auth remove requires an interactive terminal unless --account-id is supplied.",
        "auth remove non-tty",
    );

    let auth_remove_no_yes = run_codex_ops(
        [
            "auth",
            "remove",
            "--auth-file",
            sandbox.auth_file.to_str().unwrap(),
            "--store-dir",
            sandbox.store_dir.to_str().unwrap(),
            "--account-id",
            "account-other",
        ],
        &sandbox,
    );
    assert_failure_contains(
        &auth_remove_no_yes,
        1,
        "auth remove --account-id requires --yes when not running interactively.",
        "auth remove missing yes",
    );

    assert_eq!(
        read_file_bytes(&sandbox.auth_file),
        auth_file_before,
        "auth non-tty errors changed auth.json"
    );
    assert_eq!(
        read_file_bytes(&sandbox.account_history_file),
        account_history_before,
        "auth non-tty errors changed account history"
    );
    assert_eq!(
        read_file_bytes(&stored_profile),
        stored_profile_before,
        "auth non-tty errors changed stored profile"
    );
}
