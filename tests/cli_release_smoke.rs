mod common;

use common::repo_root;
use std::process::Command;

#[test]
fn release_metadata_guard_passes_and_checks_cargo_package_contents() {
    let output = Command::new("node")
        .arg("scripts/check-release.mjs")
        .current_dir(repo_root())
        .output()
        .expect("run release metadata guard");

    assert!(
        output.status.success(),
        "release metadata guard failed\n--- stdout ---\n{}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("release metadata check passed for codex-ops"),
        "release metadata guard stdout missing success line\n--- stdout ---\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
}
