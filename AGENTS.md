# AGENTS.md

## Project

`codex-ops` is a Rust CLI distributed through a thin npm shim. It is intended to
run as:

```bash
npx codex-ops
```

The Rust crate, npm package, and user-facing binary are all named `codex-ops`.

## Development

- Use Rust for CLI business logic.
- Keep Rust source in standard Cargo paths: `src/**/*.rs` and
  `src/bin/**/*.rs`.
- Keep the npm entrypoint in `bin/codex-ops.js` as a shim only: platform
  detection, binary lookup, process forwarding, and clear install errors.
- Do not add auth, doctor, stat, cycle, pricing, parsing, or storage business
  logic to JavaScript.
- Build with `rtk cargo build --release`.
- Run tests with `rtk cargo test`.
- Run release metadata checks with `rtk npm run release:check`.
- Run shim smoke with
  `rtk env CODEX_OPS_RUST_BINARY=target/release/codex-ops npm run smoke:npm-shim`.
- Run Rust CLI fixture smoke with
  `rtk env CODEX_OPS_RUST_BINARY=target/release/codex-ops npm run smoke:rust-cli`.
- Run the default Rust benchmark smoke with `rtk npm run bench:rust`.
- Do not commit real Codex auth files, session JSONL, account IDs, tokens, cwd
  values, or user content. Use only synthetic fixtures.

## Local Shell

This workspace follows the local RTK instruction:

```bash
rtk <command>
```

Prefix shell commands with `rtk` when working in this repository.
