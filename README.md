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
codex-helper init
codex-helper init --yes
```

## Tech Stack

- TypeScript for typed source code.
- tsdown for ESM builds and declaration output.
- Vitest for unit tests.
- Commander for CLI parsing.
- `@inquirer/prompts`, picocolors, and ora for richer terminal interactions.

## Package Layout

```text
src/cli.ts        CLI entrypoint
src/index.ts      Shared logic
test/*.test.ts    Vitest tests
dist/             Build output
```
