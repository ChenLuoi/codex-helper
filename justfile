# Local development orchestration. Keep assertions in Rust tests or scripts;
# recipes here should only compose existing commands.

default:
    @just --list

# Check Rust formatting.
fmt:
    cargo fmt --check

# Run the Rust test suite.
test:
    cargo test

# Build the release binary.
build:
    cargo build --release

# Run the Rust CLI fixture smoke integration test.
smoke-rust-cli:
    cargo test --test cli_smoke

# Run the npm shim smoke.
smoke-npm-shim: build
    CODEX_OPS_RUST_BINARY=target/release/codex-ops npm run smoke:npm-shim

# Run all smoke checks.
smoke: smoke-rust-cli smoke-npm-shim

# Run the default fixture benchmark smoke.
bench: build
    target/release/codex-ops-bench --fixture test/fixtures/rust-run --runs 1 --rust-binary target/release/codex-ops

# Validate release metadata and local package layout.
release-check: build
    npm run release:check
    npm run release:dry-run

# Run the local CI-style verification set without publishing.
ci-local: fmt test build release-check smoke bench
