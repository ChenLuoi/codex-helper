mod common;

use common::repo_root;
use std::process::Command;

#[test]
fn npm_shim_resolves_linux_glibc_to_gnu_package_target() {
    let output = Command::new("node")
        .arg("bin/codex-ops.js")
        .arg("--help")
        .current_dir(repo_root())
        .env_remove("CODEX_OPS_RUST_BINARY")
        .env("CODEX_OPS_SHIM_TEST_PLATFORM", "linux")
        .env("CODEX_OPS_SHIM_TEST_ARCH", "x64")
        .env("CODEX_OPS_SHIM_TEST_LIBC", "glibc")
        .output()
        .expect("run npm shim glibc package lookup path");

    assert_eq!(
        output.status.code(),
        Some(127),
        "glibc package lookup should exit 127 when package is not installed\n--- stdout ---\n{}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    for expected in [
        "codex-ops: unable to find the Rust binary.",
        "target: linux-x64-gnu",
        "codex-ops-linux-x64-gnu",
    ] {
        assert!(
            stderr.contains(expected),
            "glibc package lookup stderr missing {expected:?}\n--- stderr ---\n{stderr}"
        );
    }
}

#[test]
fn npm_shim_resolves_linux_musl_to_musl_package_target() {
    let output = Command::new("node")
        .arg("bin/codex-ops.js")
        .arg("--help")
        .current_dir(repo_root())
        .env_remove("CODEX_OPS_RUST_BINARY")
        .env("CODEX_OPS_SHIM_TEST_PLATFORM", "linux")
        .env("CODEX_OPS_SHIM_TEST_ARCH", "x64")
        .env("CODEX_OPS_SHIM_TEST_LIBC", "musl")
        .output()
        .expect("run npm shim musl package lookup path");

    assert_eq!(
        output.status.code(),
        Some(127),
        "musl package lookup should exit 127 when package is not installed\n--- stdout ---\n{}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    for expected in [
        "codex-ops: unable to find the Rust binary.",
        "target: linux-x64-musl",
        "codex-ops-linux-x64-musl",
    ] {
        assert!(
            stderr.contains(expected),
            "musl package lookup stderr missing {expected:?}\n--- stderr ---\n{stderr}"
        );
    }
}
