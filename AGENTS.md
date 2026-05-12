# AGENTS.md

## Project

`codex-helper` is a Node.js CLI package. It is intended to run as:

```bash
npx codex-helper
```

## Development

- Use TypeScript for source files.
- Keep CLI parsing in `src/cli.ts`.
- Keep reusable logic in `src/index.ts` and cover it with Vitest tests.
- Build with `npm run build`.
- Run tests with `npm test`.
- Run type checks with `npm run typecheck`.

## Local Shell

This workspace follows the local RTK instruction:

```bash
rtk <command>
```

Prefix shell commands with `rtk` when working in this repository.
