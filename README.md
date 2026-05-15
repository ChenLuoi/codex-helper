# codex-helper

`codex-helper` is a Node.js command line project scaffold for Codex-oriented development workflows.

## Usage

After the package is published, run it directly with:

```bash
npx codex-helper
```

Local development:

```bash
npm install
npm run dev -- --help
npm run build
npm test
```

Published CLI installs support Node.js `>=20.12.0`. Local development currently
requires Node.js `^22.18.0 || >=24.0.0` for the build tooling.

## Commands

```bash
codex-helper --help
codex-helper auth status
codex-helper auth status --auth-file ~/.codex/auth.json
codex-helper auth status --json
codex-helper auth save
codex-helper auth list
codex-helper auth select
codex-helper auth select --account-id <account-id>
codex-helper auth remove
codex-helper auth remove --account-id <account-id> --yes
codex-helper doctor
codex-helper stat
codex-helper stat --start 2026-05-01 --end 2026-05-12 --group-by day
codex-helper stat --group-by hour
codex-helper stat --group-by week
codex-helper stat --group-by month
codex-helper stat --group-by model
codex-helper stat --group-by model --reasoning-effort
codex-helper stat --group-by cwd
codex-helper stat --group-by account
codex-helper stat --account-id <account-id>
codex-helper stat --all --group-by model --format csv
codex-helper stat --today
codex-helper stat --month --format markdown
codex-helper stat --last 30d --format json
codex-helper stat --last 2w --format csv
codex-helper stat --group-by model --sort credits --limit 5
codex-helper stat --verbose
codex-helper stat sessions --top 10
codex-helper stat sessions --sort time --limit 10
codex-helper stat sessions session-a --last 30d
codex-helper stat sessions session-a --format json --limit 20
codex-helper stat sessions --last 30d --format json
codex-helper cycle add "2026-05-01 08:00" --note "initial weekly cycle"
codex-helper cycle add "2026-05-01 08:00" "2026-05-09 10:30"
codex-helper cycle list
codex-helper cycle remove <anchor-id>
codex-helper cycle current
codex-helper cycle history
codex-helper cycle history <cycle-id>
codex-helper cycle history --select
codex-helper cycle history --start 2026-05-01 --end 2026-05-31 --format json
codex-helper cycle history --estimate-before-anchor
```

### Auth

Syntax:

```bash
codex-helper auth status
codex-helper auth save
codex-helper auth list
codex-helper auth select
codex-helper auth remove
```

Auth commands read `auth.json` from `$CODEX_HOME/auth.json` by default, or
`~/.codex/auth.json` when `CODEX_HOME` is not set. It expects the fixed Codex
auth structure and decodes `tokens.id_token` without verifying the signature.
`auth status` prints only the key account fields: account ID, key ID, name,
email, user ID, plan, and organizations. It never prints the raw ID token.

`auth save` persists the entire current `auth.json` under the profile store
using the account ID as the unique key. By default the store is
`$CODEX_HOME/codex-helper/auth-profiles`; `--auth-file` only changes which
auth file is read. Use `--store-dir` to choose a different profile store.
`auth list` only shows the current profile and readable persisted profiles. If a
persisted profile cannot be decoded, it is listed under skipped profiles instead
of failing the whole command. `auth select` switches to a persisted profile; in
an interactive terminal it uses an Up/Down/Enter selection list, saves the
current `auth.json` first, then replaces `auth.json` with the selected persisted
content. The first switch also initializes
`$CODEX_HOME/codex-helper/auth-account-history.json` from the current
`auth.json`, then records each successful `auth select` timestamp so usage can be
attributed back to the active account. `--store-dir` only moves saved auth
profiles; use `--account-history-file` if the account history itself should live
somewhere else. `auth remove` shows an interactive multi-select list where Space
toggles entries and Enter confirms the selection, then asks for a second
confirmation before deleting persisted copies.

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
codex-helper stat [view] [session]
```

`stat` reads Codex session JSONL files from `~/.codex/sessions` by default.
Use `--codex-home` or `--sessions-dir` to point it at another Codex data
directory. The default scanner reads rollout files in the requested range and
checks older rollout files in a bounded lookback window by their last
`token_count` timestamp before deciding whether to read them. The lookback is
`min(max((end - start) / 2, 2 days), 7 days)`.
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
| `codex-helper stat` | Aggregate token usage by the resolved `group-by` value. |
| `codex-helper stat sessions` | Top sessions by credits by default. |
| `codex-helper stat sessions <session-id>` | Event-level token usage timeline for one session. |

### Weekly Limit Cycles

Syntax:

```bash
codex-helper cycle add/list/remove
codex-helper cycle current
codex-helper cycle history
codex-helper cycle history <cycle-id>
codex-helper cycle history --select
```

`cycle` estimates Codex weekly-limit usage from local `token_count` events
and user-provided anchors. It does not call Codex or OpenAI services and it does
not implement 5-hour limit windows.

A weekly anchor is the first real use that starts a weekly limit cycle. The
cycle resets 168 hours later. If no local usage occurs after that reset, no new
cycle is opened yet; the next local usage event after reset becomes the next
cycle start.

Anchors are stored by account in
`$CODEX_HOME/codex-helper/stat-cycles.json`. The account is resolved from
`--account-id`, then the current `auth.json` account, then the fallback
`default` account bucket. Cycle usage reads `auth-account-history.json` when
available so usage from other accounts is not mixed into the selected account.
Use `--cycle-file <path>` for an isolated store.

Examples:

```bash
codex-helper cycle add "2026-05-01 08:00" --note "known reset use"
codex-helper cycle add "2026-05-01 08:00" "2026-05-09 10:30"
codex-helper cycle list
codex-helper cycle current
codex-helper cycle history --last 30d
codex-helper cycle history cyc_20260509T080000000Z --last 30d
codex-helper cycle history --select --last 30d
codex-helper cycle history --estimate-before-anchor --format json
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
an unpriced-model breakdown with a stub you can fill into `src/pricing.ts`.
JSON output includes the same information under `unpricedModels`.

| Model | Input / 1M | Cached input / 1M | Output / 1M |
| --- | ---: | ---: | ---: |
| GPT-5.5 | 125 credits | 12.50 credits | 750 credits |
| GPT-5.4 | 62.50 credits | 6.250 credits | 375 credits |
| GPT-5.4-mini | 18.75 credits | 1.875 credits | 113 credits |
| GPT-5.3-Codex | 43.75 credits | 4.375 credits | 350 credits |
| GPT-5.2 | 43.75 credits | 4.375 credits | 350 credits |
| GPT-5.3-Codex-Spark | research preview | research preview | research preview |
| GPT-Image-2 (image) | 200 credits | 50 credits | 750 credits |
| GPT-Image-2 (text) | 125 credits | 31.25 credits | 250 credits |

## Tech Stack

- TypeScript for typed source code.
- tsdown for ESM builds and declaration output.
- Vitest for unit tests.
- Commander for CLI parsing.
- Inquirer for interactive CLI prompts.
- picocolors and ora for richer terminal output.

## Package Layout

```text
src/cli.ts        CLI entrypoint
src/index.ts      Public package entry
test/*.test.ts    Vitest tests
dist/             Build output
```
