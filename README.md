# codex-ops

`codex-ops` is a Rust CLI for local Codex auth profiles, session usage, and
weekly cycle workflows.

The public command and Rust crate name are both `codex-ops`. The npm package is
only a thin distribution shim: it detects the current platform, finds the
prebuilt Rust binary from an optional platform package, and forwards argv,
stdio, signals, and exit codes. It does not expose a JavaScript import API and
does not contain JavaScript business logic.

## Usage

After the npm package is published, run it directly with:

```bash
npx codex-ops
```

After the crate is published, Rust users can install the same CLI with:

```bash
cargo install codex-ops
```

Local development in this repository uses Cargo for the product binary:

```bash
rtk cargo test
rtk cargo build --release
rtk target/release/codex-ops --help
```

The npm shim can be tested against the local release binary:

```bash
rtk env CODEX_OPS_RUST_BINARY=target/release/codex-ops npm run smoke:npm-shim
```

The Rust CLI fixture smoke runs as an integration test:

```bash
rtk npm run smoke:rust-cli
```

Published npm installs support Node.js `>=20.12.0` for the shim. Local
development requires a Rust stable toolchain; Node.js `>=20.12.0` is only
needed for npm shim, packaging, and release helper scripts.

## Commands

```bash
codex-ops --help
codex-ops auth status
codex-ops auth status --auth-file ~/.codex/auth.json
codex-ops auth status --json
codex-ops auth save
codex-ops auth list
codex-ops auth select
codex-ops auth select --account-id <account-id>
codex-ops auth remove
codex-ops auth remove --account-id <account-id> --yes
codex-ops doctor
codex-ops stat
codex-ops stat --start 2026-05-01 --end 2026-05-12 --group-by day
codex-ops stat --group-by hour
codex-ops stat --group-by week
codex-ops stat --group-by month
codex-ops stat --group-by model
codex-ops stat --group-by model --reasoning-effort
codex-ops stat --group-by cwd
codex-ops stat --group-by account
codex-ops stat --account-id <account-id>
codex-ops stat --all --group-by model --format csv
codex-ops stat --today
codex-ops stat --month --format markdown
codex-ops stat --last 30d --format json
codex-ops stat --last 2w --format csv
codex-ops stat --group-by model --sort credits --limit 5
codex-ops stat --verbose
codex-ops stat sessions --top 10
codex-ops stat sessions --sort time --limit 10
codex-ops stat sessions session-a --last 30d
codex-ops stat sessions session-a --format json --limit 20
codex-ops stat sessions --last 30d --format json
codex-ops cycle add "2026-05-01 08:00" --note "initial weekly cycle"
codex-ops cycle add "2026-05-01 08:00" "2026-05-09 10:30"
codex-ops cycle list
codex-ops cycle remove <anchor-id>
codex-ops cycle current
codex-ops cycle history
codex-ops cycle history <cycle-id>
codex-ops cycle history --select
codex-ops cycle history --start 2026-05-01 --end 2026-05-31 --format json
codex-ops cycle history --estimate-before-anchor
```

### Auth

Syntax:

```bash
codex-ops auth status
codex-ops auth save
codex-ops auth list
codex-ops auth select
codex-ops auth remove
```

Auth commands read `auth.json` from `$CODEX_HOME/auth.json` by default, or
`~/.codex/auth.json` when `CODEX_HOME` is not set. It expects the fixed Codex
auth structure and decodes `tokens.id_token` without verifying the signature.
`auth status` prints only the key account fields: account ID, key ID, name,
email, user ID, plan, and organizations. It never prints the raw ID token.

`auth save` persists the entire current `auth.json` under the profile store
using the account ID as the unique key. By default the store is
`$CODEX_HOME/codex-ops/auth-profiles`; `--auth-file` only changes which
auth file is read. Use `--store-dir` to choose a different profile store.
`auth list` only shows the current profile and readable persisted profiles. If a
persisted profile cannot be decoded, it is listed under skipped profiles instead
of failing the whole command. `auth select` switches to a persisted profile; in
an interactive terminal it uses an Up/Down/Enter selection list, saves the
current `auth.json` first, then replaces `auth.json` with the selected persisted
content. The first switch also initializes
`$CODEX_HOME/codex-ops/auth-account-history.json` from the current
`auth.json`, then records each successful `auth select` timestamp so usage can be
attributed back to the active account. `--store-dir` only moves saved auth
profiles; use `--account-history-file` if the account history itself should live
somewhere else. `auth remove` shows an interactive multi-select list where Space
toggles entries and Enter confirms the selection, then asks for a second
confirmation before deleting persisted copies. The interactive remove list does
not offer the currently active profile, and cancelling an interactive prompt
leaves auth files, saved profiles, and account history unchanged.

Options:

| Option | Behavior |
| --- | --- |
| `--auth-file <path>` | Use a specific `auth.json` file. |
| `--codex-home <path>` | Read `<path>/auth.json`. Ignored when `--auth-file` is supplied. |
| `--store-dir <path>` | Use a specific auth profile store directory for `save`, `list`, `select`, and `remove`. |
| `--account-history-file <path>` | Use a specific auth account history file for `select`. |
| `-j, --json` | Print JSON output with the summarized auth fields. |
| `--include-token-claims` | Include the decoded JWT header and claims in JSON output. |
| `-A, --account-id <id>` | Select or remove a specific persisted profile. |
| `-y, --yes` | Skip confirmation when removing with `--account-id`. |

### Stat

Syntax:

```bash
codex-ops stat [view] [session]
```

`stat` reads Codex session JSONL files from `~/.codex/sessions` by default.
Use `--codex-home` or `--sessions-dir` to point it at another Codex data
directory. The default scanner reads rollout files in the requested range and
checks older rollout files in a bounded lookback window by their last
`token_count` timestamp before deciding whether to read them. The lookback is
`min(max((end - start) / 2, 7 days), 30 days)`.
Use `-F, --full-scan` when you need exact local `token_count` results across
long sessions that may have started before the requested range. Full scan checks
all rollout files before the requested range by last `token_count` timestamp.
Date-ranged non-full-scan table and Markdown output includes a reminder, and
JSON output includes the same message in `warnings`.
Use `--group-by account` or `--account-id <id>` to initialize/read
`auth-account-history.json` and attribute `token_count` events by the account
active at each event timestamp.

Views:

| Command | Output |
| --- | --- |
| `codex-ops stat` | Aggregate token usage by the resolved `group-by` value. |
| `codex-ops stat sessions` | Top sessions by credits by default. |
| `codex-ops stat sessions <session-id>` | Event-level token usage timeline for one session. |

### Weekly Limit Cycles

Syntax:

```bash
codex-ops cycle add/list/remove
codex-ops cycle current
codex-ops cycle history
codex-ops cycle history <cycle-id>
codex-ops cycle history --select
```

`cycle` estimates Codex weekly-limit usage from local `token_count` events
and user-provided anchors. It does not call Codex or OpenAI services and it does
not implement 5-hour limit windows.

A weekly anchor is the first real use that starts a weekly limit cycle. The
cycle resets 168 hours later. If no local usage occurs after that reset, no new
cycle is opened yet; the next local usage event after reset becomes the next
cycle start.

Anchors are stored by account in
`$CODEX_HOME/codex-ops/stat-cycles.json`. The account is resolved from
`--account-id`, then the current `auth.json` account, then the fallback
`default` account bucket. Cycle usage reads `auth-account-history.json` when
available so usage from other accounts is not mixed into the selected account.
If account history is missing and the selected account matches the current
`auth.json`, cycle reports initialize the history default before filtering. Use
`--cycle-file <path>` for an isolated store.

Examples:

```bash
codex-ops cycle add "2026-05-01 08:00" --note "known reset use"
codex-ops cycle add "2026-05-01 08:00" "2026-05-09 10:30"
codex-ops cycle list
codex-ops cycle current
codex-ops cycle history --last 30d
codex-ops cycle history cyc_20260509T080000000Z --last 30d
codex-ops cycle history --select --last 30d
codex-ops cycle history --estimate-before-anchor --format json
```

`cycle add` accepts one or more times. Quote values that contain spaces, or
pass common `YYYY-MM-DD HH:mm` values as unquoted date/time pairs. Time input
with an explicit offset, such as `2026-05-01T08:00:00+08:00`, is parsed using
that offset. Time input without an offset, such as `2026-05-01 08:00`, is
interpreted in the current system time zone. Stored anchors keep the original
input and save the instant as UTC ISO. Use `-n, --note <text>` to attach a note
to added anchors.

History reports include a stable cycle ID in each row. Manual cycles use the
anchor ID, derived cycles use `cyc_<UTC-start>`, and estimated cycles use
`est_<UTC-start>`. Pass one of those IDs to `cycle history <cycle-id>` to show
current-style details for that cycle, including by-day and by-model breakdowns.
Use `cycle history --select` in an interactive terminal to choose from matching
history rows.

Cycle reports mark each row with a source:

| Source | Meaning |
| --- | --- |
| `manual` | Cycle start came from a user anchor. |
| `derived` | Cycle start came from the first local usage event after reset. |
| `estimated` | Pre-anchor history was included only because `--estimate-before-anchor` was supplied. |
| `unanchored` | No usable anchor exists for the selected account. |

By default, history before the earliest anchor is not reported as exact. Use
`--estimate-before-anchor` only when you want fixed 168-hour pre-anchor buckets
clearly labeled as `estimated`. `current` table output shows the current cycle
summary, a by-day breakdown, and a by-model breakdown; JSON output includes the
same data as `current`, `byDay`, and `byModel`. `current` and `history` read
session JSONL files from the normal stat sessions directory and enable full
JSONL file scanning so long sessions are filtered by event time instead of
rollout filename time.

Cycle options:

| Option | Behavior |
| --- | --- |
| `-A, --account-id <id>` | Use a specific cycle account bucket. |
| `--cycle-file <path>` | Use a specific anchor store file. |
| `--auth-file <path>` | Use a specific `auth.json` when resolving the account. |
| `--codex-home <path>` | Resolve `auth.json`, sessions, and the default cycle file under this Codex home. |
| `--sessions-dir <path>` | Use a specific sessions directory for `current` and `history`. |
| `-i, --select` | Interactively select a history cycle to show in detail. |
| `--estimate-before-anchor` | Include pre-anchor estimated history rows. |

Time range options:

| Option | Behavior |
| --- | --- |
| `-s, --start <time>` | Start time. Date-only values start at local `00:00:00.000`. |
| `-e, --end <time>` | End time. Date-only values end at local `23:59:59.999`. |
| `-t, --today` | Current local day through now. |
| `--yesterday` | Previous local day. |
| `-m, --month` | Current local calendar month through now. |
| `-L, --last <duration>` | Recent duration such as `12h`, `7d`, `2w`, or `1mo`. |
| `-a, --all` | Scan and include all session usage records without date pruning. |

When `--group-by` is not supplied, `stat` chooses a default from the resolved
time range: ranges up to 48 hours use `hour`, ranges up to 31 days use `day`,
ranges up to six calendar months use `week`, and longer ranges use `month`.
`--month` remains grouped by `day` by default, while `--all` defaults to
`month`.

Aggregation and shaping options:

| Option | Behavior |
| --- | --- |
| `-g, --group-by <group>` | Aggregate by `hour`, `day`, `week`, `month`, `model`, or `cwd`. Ignored by `sessions` views. |
| `-S, --sort <sort>` | Sort rows by `time`, `tokens`, `credits`, `calls`, or `sessions`. |
| `-n, --limit <n>` | Cap output rows. For `sessions <session-id>`, this caps displayed events while totals still cover the whole matched session. |
| `-T, --top <n>` | Session-list row count. When both `--top` and `--limit` are supplied to `stat sessions`, `--top` wins. |
| `-d, --detail` | Show full event-level rows for `stat sessions <session-id>`. |
| `-F, --full-scan` | Scan all session files instead of pruning by date. |
| `-r, --reasoning-effort` | When grouping by `model`, append Codex reasoning effort to the model key. |
| `-A, --account-id <id>` | Only include usage attributed to an account id. |

When `--reasoning-effort` is combined with `--group-by model`, Codex reasoning
effort is appended when present, for example `gpt-5.5-high` or
`gpt-5.5-xhigh`. Pricing still uses the base model name.

Output options:

| Option | Behavior |
| --- | --- |
| `-f, --format <format>` | Output `table`, `json`, `csv`, or `markdown`. |
| `-j, --json` | Alias for `--format json`. |
| `-v, --verbose` | Include scan and parsing diagnostics in table output. JSON output always includes diagnostics. |

Diagnostics include scanned/skipped directories, read/skipped files, read lines,
invalid JSON lines, token-count events, included usage events, skipped-event
reasons, and file read concurrency.

Credits are estimated from the token counters in each session. Cached input
tokens are billed at the cached-input rate; regular input credits use
`max(inputTokens - cachedInputTokens, 0)`. USD estimates use `25 credits = $1`.
When a model has no configured price, it is excluded from Credits and listed in
an unpriced-model breakdown with a stub you can fill into
`data/codex-rate-card.json`.
JSON output includes the same information under `unpricedModels`.

Pricing data is statically embedded from `data/codex-rate-card.json`. The
current snapshot source is OpenAI Help Center Codex rate card, checked
2026-05-13.

| Model | Input / 1M | Cached input / 1M | Output / 1M | Note |
| --- | ---: | ---: | ---: | --- |
| GPT-5.5 | 125 credits | 12.50 credits | 750 credits |  |
| GPT-5.4 | 62.50 credits | 6.250 credits | 375 credits |  |
| GPT-5.4-mini | 18.75 credits | 1.875 credits | 113 credits |  |
| GPT-5.3-Codex | 43.75 credits | 4.375 credits | 350 credits |  |
| GPT-5.2 | 43.75 credits | 4.375 credits | 350 credits |  |
| GPT-5.3-Codex-Spark | 0 credits | 0 credits | 0 credits | research preview; charged at 0 credits |
| GPT-Image-2 (image) | 200 credits | 50 credits | 750 credits |  |
| GPT-Image-2 (text) | 125 credits | 31.25 credits | 250 credits |  |

## Development

The CLI implementation lives in standard Cargo source paths. Keep business
logic in Rust; JavaScript is reserved for the npm shim and release helper
scripts.

```bash
rtk cargo fmt --check
rtk cargo test
rtk cargo build --release
rtk npm run release:check
rtk npm run smoke:rust-cli
rtk env CODEX_OPS_RUST_BINARY=target/release/codex-ops npm run smoke:npm-shim
rtk npm run bench:rust
```

The repository also includes a `justfile` for local orchestration. Recipes only
compose existing Cargo and npm commands; assertions and fixture behavior belong
in Rust tests or dedicated helper scripts. In this workspace, run recipes
through RTK:

```bash
rtk just --list
rtk just test
rtk just build
rtk just smoke
rtk just bench
rtk just release-check
```

The default benchmark command is Rust-only and uses the synthetic fixture in
`test/fixtures/rust-run`. Larger 100x benchmark data is local-only and must not
be committed.

Node scripts under `scripts/` are reserved for npm shim smoke and npm release
packaging. Default CLI smoke and benchmark coverage should stay in Rust tests
or Rust helper binaries.

## Package Layout

```text
src/main.rs                      Rust CLI process entry
src/lib.rs                       Rust command parsing and dispatch
src/*.rs                         Rust business modules
src/bin/codex-ops-bench.rs       Local Rust benchmark smoke helper
bin/codex-ops.js                 npm shim entrypoint
npm/<target>/package.json        npm platform package manifests
justfile                         local command orchestration
scripts/*.mjs                    npm shim/release helpers
test/fixtures/rust-run/          synthetic fixture data only
```

The published Cargo crate exposes only the `codex-ops` binary. Local helper
binaries, npm packaging assets, CI configuration, task documents, and synthetic
fixtures are excluded from the crate package; they remain repository-only
development assets.

## Release

GitHub Actions builds release artifacts for:

```text
linux-x64-gnu
linux-arm64-gnu
linux-x64-musl
linux-arm64-musl
darwin-x64
darwin-arm64
win32-x64-msvc
```

Linux npm packages are split by libc. GNU/glibc Linux installs the `*-gnu`
package for native performance, while musl Linux installs the static `*-musl`
package for Alpine-style environments.

The main `codex-ops` npm package depends on scoped `@codexops/*` platform
packages through `optionalDependencies`. The release workflow is tag-driven:
push a `vX.Y.Z` tag matching `package.json` / `Cargo.toml`, and Actions builds
the platform binaries, npm tarballs, crate archive, `release-manifest.json`, and
top-level `SHA256SUMS` once. Those assets are uploaded to a draft GitHub
Release before registry publishing starts.

Publishing to crates.io and npm is gated by the GitHub `release` environment.
After approval, the workflow publishes the crate, then the scoped platform npm
tarballs, then the main npm tarball. npm publish steps reuse the previously
packed tarballs and skip package versions that already exist, so a failed
registry publish can be retried from the same tag. When all registry publishes
finish, the draft GitHub Release is published.

Before publishing, recheck npm platform package name availability, crates.io
token access, GitHub `release` environment approval, and whether any legacy
migration package or alias is needed.

## Data Safety

`codex-ops` reads local Codex files such as `$CODEX_HOME/auth.json`,
`$CODEX_HOME/sessions`, and `$CODEX_HOME/codex-ops/*`. Do not commit real
auth files, raw session JSONL, account IDs, tokens, cwd values, or user content.
Use only synthetic fixtures under `test/fixtures/**`.
