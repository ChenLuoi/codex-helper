mod common;

use common::repo_root;
use std::process::Command;

#[test]
fn npm_shim_reports_unsupported_musl_without_binary_lookup() {
    let output = Command::new("node")
        .arg("bin/codex-ops.js")
        .arg("--help")
        .current_dir(repo_root())
        .env_remove("CODEX_OPS_RUST_BINARY")
        .env("CODEX_OPS_SHIM_TEST_PLATFORM", "linux")
        .env("CODEX_OPS_SHIM_TEST_ARCH", "x64")
        .env("CODEX_OPS_SHIM_TEST_LIBC", "musl")
        .output()
        .expect("run npm shim unsupported musl path");

    assert_eq!(
        output.status.code(),
        Some(1),
        "unsupported musl should exit 1\n--- stdout ---\n{}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    for expected in [
        "codex-ops: unsupported platform for the bundled Rust binary.",
        "target: linux-x64-musl",
        "Alpine/musl Linux is not supported",
        "linux-x64-gnu",
        "linux-arm64-gnu",
    ] {
        assert!(
            stderr.contains(expected),
            "unsupported musl stderr missing {expected:?}\n--- stderr ---\n{stderr}"
        );
    }
}
