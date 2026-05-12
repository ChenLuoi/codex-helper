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

## Commands

```bash
codex-helper --help
codex-helper doctor
codex-helper stat
codex-helper stat --start 2026-05-01 --end 2026-05-12 --group-by day
codex-helper stat --group-by hour
codex-helper stat --group-by week
codex-helper stat --group-by month
codex-helper stat --group-by model
codex-helper stat --group-by cwd
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
```

### Stat

Syntax:

```bash
codex-helper stat [view] [session]
```

`stat` reads Codex session JSONL files from `~/.codex/sessions` by default.
Use `--codex-home` or `--sessions-dir` to point it at another Codex data
directory. The scanner prunes date-shaped `YYYY/MM/DD` directories and rollout
filenames by the requested range, then reads matching files with bounded
concurrency.

Views:

| Command | Output |
| --- | --- |
| `codex-helper stat` | Aggregate token usage by the resolved `group-by` value. |
| `codex-helper stat sessions` | Top sessions by credits by default. |
| `codex-helper stat sessions <session-id>` | Event-level token usage timeline for one session. |

Time range options:

| Option | Behavior |
| --- | --- |
| `--start <time>` | Start time. Date-only values start at local `00:00:00.000`. |
| `--end <time>` | End time. Date-only values end at local `23:59:59.999`. |
| `--today` | Current local day through now. |
| `--yesterday` | Previous local day. |
| `--month` | Current local calendar month through now. |
| `--last <duration>` | Recent duration such as `12h`, `7d`, `2w`, or `1mo`. |

When `--group-by` is not supplied, `stat` chooses a default from the resolved
time range: ranges up to 48 hours use `hour`, ranges up to 31 days use `day`,
ranges up to six calendar months use `week`, and longer ranges use `month`.
`--month` remains grouped by `day` by default.

Aggregation and shaping options:

| Option | Behavior |
| --- | --- |
| `--group-by <group>` | Aggregate by `hour`, `day`, `week`, `month`, `model`, or `cwd`. Ignored by `sessions` views. |
| `--sort <sort>` | Sort rows by `time`, `tokens`, `credits`, `calls`, or `sessions`. |
| `--limit <n>` | Cap output rows. For `sessions <session-id>`, this caps displayed events while totals still cover the whole matched session. |
| `--top <n>` | Session-list row count. When both `--top` and `--limit` are supplied to `stat sessions`, `--top` wins. |

Output options:

| Option | Behavior |
| --- | --- |
| `--format <format>` | Output `table`, `json`, `csv`, or `markdown`. |
| `--json` | Alias for `--format json`. |
| `--verbose` | Include scan and parsing diagnostics in table output. JSON output always includes diagnostics. |

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
- picocolors and ora for richer terminal output.

## Package Layout

```text
src/cli.ts        CLI entrypoint
src/index.ts      Public package entry
test/*.test.ts    Vitest tests
dist/             Build output
```
