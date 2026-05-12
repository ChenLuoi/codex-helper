#!/usr/bin/env node

import { Command } from "commander";
import { readFileSync, realpathSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import ora from "ora";
import pc from "picocolors";
import {
  formatAuthProfileEntry,
  formatAuthProfileList,
  formatAuthStatus,
  listCodexAuthProfiles,
  readCodexAuthStatus,
  removeCodexAuthProfile,
  saveCurrentCodexAuthProfile,
  switchCodexAuthProfile,
  type AuthProfileEntry
} from "./index.js";
import {
  formatUsageSessionDetail,
  formatUsageSessions,
  formatUsageStats,
  readCodexUsageSessionDetail,
  readCodexUsageSessions,
  readCodexUsageStats,
  resolveStatRangeOptions,
  resolveStatOptions
} from "./stats.js";

const packageVersion = readPackageVersion();

export function createProgram(options: { output?: NodeJS.WritableStream } = {}) {
  const output = options.output ?? process.stdout;

  const program = new Command()
    .name("codex-helper")
    .description("Command line helpers for Codex-oriented development workflows.")
    .version(packageVersion)
    .option("-v, --verbose", "show more details");

  const authCommand = program.command("auth").description("Show Codex authentication information.");

  authCommand
    .command("status")
    .description("Decode the ID token in auth.json and show key claims.")
    .option("--auth-file <path>", "path to auth.json")
    .option("--codex-home <path>", "Codex home directory", process.env.CODEX_HOME)
    .option("--json", "print JSON")
    .action(async (options: AuthStatusCommandOptions) => {
      const report = await readCodexAuthStatus({
        authFile: options.authFile,
        codexHome: options.codexHome
      });
      output.write(
        withTrailingNewline(formatAuthStatus(report, options.json === true ? "json" : "table"))
      );
    });

  authCommand
    .command("save")
    .description("Persist the current auth.json by account id.")
    .option("--auth-file <path>", "path to auth.json")
    .option("--codex-home <path>", "Codex home directory", process.env.CODEX_HOME)
    .option("--store-dir <path>", "auth profile store directory")
    .action(async (options: AuthProfileCommandOptions) => {
      const report = await saveCurrentCodexAuthProfile(toAuthProfileOptions(options));
      output.write(`Saved auth profile: ${formatAuthProfileEntry(report.profile)}\n`);
      output.write(`Store: ${report.storeDir}\n`);
    });

  authCommand
    .command("list")
    .description("List current and persisted auth profiles.")
    .option("--auth-file <path>", "path to auth.json")
    .option("--codex-home <path>", "Codex home directory", process.env.CODEX_HOME)
    .option("--store-dir <path>", "auth profile store directory")
    .action(async (options: AuthProfileCommandOptions) => {
      const profileOptions = toAuthProfileOptions(options);
      const report = await listCodexAuthProfiles(profileOptions);
      output.write(withTrailingNewline(formatAuthProfileList(report)));
    });

  authCommand
    .command("select")
    .description("Select a persisted auth profile and activate it.")
    .option("--auth-file <path>", "path to auth.json")
    .option("--codex-home <path>", "Codex home directory", process.env.CODEX_HOME)
    .option("--store-dir <path>", "auth profile store directory")
    .option("--account-id <id>", "activate a specific persisted account id")
    .action(async (options: AuthProfileCommandOptions) => {
      const profileOptions = toAuthProfileOptions(options);
      const report = await listCodexAuthProfiles(profileOptions);
      const switchable = report.stored.filter(
        (entry) => entry.accountId !== report.current?.accountId
      );
      let selected =
        options.accountId === undefined
          ? undefined
          : report.stored.find((entry) => entry.accountId === options.accountId);

      if (options.accountId !== undefined && selected === undefined) {
        throw new Error(`No persisted auth profile found for account id: ${options.accountId}`);
      }

      if (selected !== undefined && selected.accountId === report.current?.accountId) {
        output.write(`Auth profile already active: ${formatAuthProfileEntry(selected)}\n`);
        return;
      }

      if (selected === undefined) {
        if (report.current === undefined) {
          throw new Error("auth select requires a current auth.json so it can be saved before switching.");
        }

        if (switchable.length === 0) {
          output.write("No persisted auth profiles to select.\n");
          return;
        }

        if (!canPromptList()) {
          throw new Error("auth select requires an interactive terminal unless --account-id is supplied.");
        }

        selected = await promptAuthProfileSelection("Select auth profile to activate", switchable, output);

        if (selected === undefined) {
          output.write("Cancelled.\n");
          return;
        }
      }

      const switched = await switchCodexAuthProfile(selected.accountId, profileOptions);
      output.write(`Saved current auth profile: ${formatAuthProfileEntry(switched.savedCurrent)}\n`);
      output.write(`Activated auth profile: ${formatAuthProfileEntry(switched.activated)}\n`);
    });

  authCommand
    .command("remove")
    .description("Remove a persisted auth profile after confirmation.")
    .option("--auth-file <path>", "path to auth.json")
    .option("--codex-home <path>", "Codex home directory", process.env.CODEX_HOME)
    .option("--store-dir <path>", "auth profile store directory")
    .option("--account-id <id>", "remove a specific persisted account id")
    .option("-y, --yes", "skip confirmation when --account-id is supplied")
    .action(async (options: AuthProfileCommandOptions) => {
      const profileOptions = toAuthProfileOptions(options);
      const report = await listCodexAuthProfiles(profileOptions);

      if (report.stored.length === 0) {
        output.write("No persisted auth profiles.\n");
        return;
      }

      let selected =
        options.accountId === undefined
          ? undefined
          : report.stored.find((entry) => entry.accountId === options.accountId);

      if (options.accountId !== undefined && selected === undefined) {
        throw new Error(`No persisted auth profile found for account id: ${options.accountId}`);
      }

      if (selected === undefined) {
        if (!canPromptList()) {
          throw new Error("auth remove requires an interactive terminal unless --account-id is supplied.");
        }

        const selectedProfiles = await promptAuthProfileMultiSelection(
          "Select auth profiles to remove",
          report.stored,
          output
        );

        if (selectedProfiles.length === 0) {
          output.write("Cancelled.\n");
          return;
        }

        const confirmed = await promptConfirmation(
          `Remove ${selectedProfiles.length} persisted auth profile(s)?`,
          output
        );

        if (!confirmed) {
          output.write("Cancelled.\n");
          return;
        }

        for (const profile of selectedProfiles) {
          const removed = await removeCodexAuthProfile(profile.accountId, profileOptions);
          output.write(`Removed auth profile: ${formatAuthProfileEntry(removed.removed)}\n`);
        }

        return;
      }

      if (options.yes !== true && !canPromptLine()) {
        throw new Error("auth remove --account-id requires --yes when not running interactively.");
      }

      const confirmed =
        options.yes === true
          ? true
          : await promptConfirmation(`Remove persisted auth profile ${selected.accountId}?`, output);

      if (!confirmed) {
        output.write("Cancelled.\n");
        return;
      }

      const removed = await removeCodexAuthProfile(selected.accountId, profileOptions);
      output.write(`Removed auth profile: ${formatAuthProfileEntry(removed.removed)}\n`);
    });

  program
    .command("doctor")
    .description("Run a quick local environment check.")
    .action(async () => {
      const spinner = ora("Checking local environment").start();
      await new Promise((resolve) => setTimeout(resolve, 150));
      spinner.succeed("Local environment looks ready.");
      output.write(`${pc.green("Node.js")} ${process.version}\n`);
    });

  const statCommand = program
    .command("stat [view] [session]")
    .description("Show Codex session token usage statistics.")
    .option("-g, --group-by <group>", "aggregation: hour, day, week, month, model, cwd")
    .option("--sort <sort>", "sort rows by: time, tokens, credits, calls, sessions")
    .option("--limit <n>", "maximum number of rows to show")
    .option("--top <n>", "number of sessions to show when view is sessions")
    .option("--verbose", "show scan and parsing diagnostics");
  addStatRangeOptions(statCommand);
  addStatFormatOptions(statCommand);
  statCommand.action(
    async (
      view: string | undefined,
      session: string | undefined,
      options: StatCommandOptions & { top?: string }
    ) => {
      const commandOptions = {
        ...options,
        verbose: program.opts<{ verbose?: boolean }>().verbose === true || options.verbose === true
      };
      let spinner: ReturnType<typeof ora> | undefined;

      try {
        if (view === undefined) {
          const statOptions = resolveStatOptions(commandOptions);
          spinner =
            statOptions.format === "table" ? ora("Reading Codex session usage").start() : undefined;
          const report = await readCodexUsageStats(statOptions);
          spinner?.succeed(`Read ${report.totals.calls} usage events.`);
          output.write(
            withTrailingNewline(
              formatUsageStats(report, statOptions.format, { verbose: statOptions.verbose })
            )
          );
          return;
        }

        if (view === "sessions") {
          const sessionOptions = resolveStatRangeOptions(commandOptions);

          if (session !== undefined) {
            spinner =
              sessionOptions.format === "table"
                ? ora("Reading Codex session usage").start()
                : undefined;
            const report = await readCodexUsageSessionDetail(sessionOptions, session);
            spinner?.succeed(`Read ${report.totals.calls} usage events.`);
            output.write(
              withTrailingNewline(
                formatUsageSessionDetail(report, sessionOptions.format, {
                  verbose: sessionOptions.verbose
                })
              )
            );
            return;
          }

          const top =
            commandOptions.top === undefined
              ? sessionOptions.limit ?? 10
              : parseTopLimit(commandOptions.top);
          spinner =
            sessionOptions.format === "table" ? ora("Reading Codex session usage").start() : undefined;
          const report = await readCodexUsageSessions(sessionOptions, top);
          spinner?.succeed(`Read ${report.totals.calls} usage events.`);
          output.write(
            withTrailingNewline(
              formatUsageSessions(report, sessionOptions.format, { verbose: sessionOptions.verbose })
            )
          );
          return;
        }

        throw new Error(`Unknown stat view: ${view}`);
      } catch (error) {
        spinner?.fail("Failed to read Codex session usage.");
        throw error;
      }
    }
  );

  return program;
}

type StatCommandOptions = {
  start?: string;
  end?: string;
  groupBy?: string;
  format?: string;
  codexHome?: string;
  sessionsDir?: string;
  today?: boolean;
  yesterday?: boolean;
  month?: boolean;
  last?: string;
  sort?: string;
  limit?: string;
  verbose?: boolean;
  json?: boolean;
};

type AuthStatusCommandOptions = {
  authFile?: string;
  codexHome?: string;
  json?: boolean;
};

type AuthProfileCommandOptions = {
  authFile?: string;
  codexHome?: string;
  storeDir?: string;
  accountId?: string;
  yes?: boolean;
};

function toAuthProfileOptions(options: AuthProfileCommandOptions) {
  return {
    authFile: options.authFile,
    codexHome: options.codexHome,
    storeDir: options.storeDir
  };
}

function canPromptLine() {
  return process.stdin.isTTY === true && process.stdout.isTTY === true;
}

function canPromptList() {
  return canPromptLine() && typeof process.stdin.setRawMode === "function";
}

async function promptAuthProfileSelection(
  label: string,
  entries: AuthProfileEntry[],
  output: NodeJS.WritableStream
) {
  let cursor = 0;

  return withRawMode<AuthProfileEntry | undefined>(output, undefined, (render, finish) => {
    renderSelectionList(label, entries, cursor, render);

    return (key) => {
      if (isCancelKey(key)) {
        finish(undefined);
        return;
      }

      if (key.name === "up") {
        cursor = wrapIndex(cursor - 1, entries.length);
        renderSelectionList(label, entries, cursor, render);
        return;
      }

      if (key.name === "down") {
        cursor = wrapIndex(cursor + 1, entries.length);
        renderSelectionList(label, entries, cursor, render);
        return;
      }

      if (key.name === "return") {
        finish(entries[cursor]);
      }
    };
  });
}

async function promptAuthProfileMultiSelection(
  label: string,
  entries: AuthProfileEntry[],
  output: NodeJS.WritableStream
) {
  let cursor = 0;
  const selectedIndexes = new Set<number>();

  return withRawMode<AuthProfileEntry[]>(output, [], (render, finish) => {
    renderMultiSelectionList(label, entries, cursor, selectedIndexes, render);

    return (key) => {
      if (isCancelKey(key)) {
        finish([]);
        return;
      }

      if (key.name === "up") {
        cursor = wrapIndex(cursor - 1, entries.length);
        renderMultiSelectionList(label, entries, cursor, selectedIndexes, render);
        return;
      }

      if (key.name === "down") {
        cursor = wrapIndex(cursor + 1, entries.length);
        renderMultiSelectionList(label, entries, cursor, selectedIndexes, render);
        return;
      }

      if (key.name === "space") {
        if (selectedIndexes.has(cursor)) {
          selectedIndexes.delete(cursor);
        } else {
          selectedIndexes.add(cursor);
        }
        renderMultiSelectionList(label, entries, cursor, selectedIndexes, render);
        return;
      }

      if (key.name === "return") {
        finish(
          [...selectedIndexes]
            .sort((left, right) => left - right)
            .map((index) => entries[index])
            .filter((entry): entry is AuthProfileEntry => entry !== undefined)
        );
      }
    };
  });
}

async function promptConfirmation(question: string, output: NodeJS.WritableStream) {
  const answer = (await promptLine(`${question} Type yes to confirm: `, output))
    .trim()
    .toLowerCase();

  return answer === "yes" || answer === "y";
}

async function promptLine(question: string, output: NodeJS.WritableStream) {
  const { createInterface } = await import("node:readline/promises");
  const readline = createInterface({
    input: process.stdin,
    output
  });

  try {
    return await readline.question(question);
  } finally {
    readline.close();
  }
}

type KeypressKey = {
  name?: string;
  ctrl?: boolean;
  sequence?: string;
};

type RawModeHandler<T> = (
  render: (lines: string[]) => void,
  finish: (value: T) => void
) => (key: KeypressKey) => void;

async function withRawMode<T>(
  output: NodeJS.WritableStream,
  cancelValue: T,
  createHandler: RawModeHandler<T>
) {
  const { emitKeypressEvents } = await import("node:readline");
  const input = process.stdin;
  const previousRawMode = input.isRaw === true;
  let renderedLines = 0;

  return await new Promise<T>((resolve) => {
    const render = (lines: string[]) => {
      if (renderedLines > 0) {
        output.write(`\x1b[${renderedLines}A`);
      } else {
        output.write("\x1b[?25l");
      }

      for (const line of lines) {
        output.write(`\x1b[2K${line}\n`);
      }

      renderedLines = lines.length;
    };
    const cleanup = () => {
      input.off("keypress", onKeypress);
      input.setRawMode(previousRawMode);
      input.pause();
      output.write("\x1b[?25h");
    };
    const finish = (value: T) => {
      cleanup();
      resolve(value);
    };
    const handler = createHandler(render, finish);
    const onKeypress = (_input: string, key: KeypressKey = {}) => {
      if (key.ctrl === true && key.name === "c") {
        cleanup();
        process.exitCode = 130;
        resolve(cancelValue);
        return;
      }

      handler(key);
    };

    emitKeypressEvents(input);
    input.setRawMode(true);
    input.resume();
    input.on("keypress", onKeypress);
  });
}

function renderSelectionList(
  label: string,
  entries: AuthProfileEntry[],
  cursor: number,
  render: (lines: string[]) => void
) {
  render([
    label,
    "Use Up/Down to move, Enter to select, q to cancel.",
    "",
    ...entries.map((entry, index) => `${index === cursor ? "> " : "  "}${formatAuthProfileEntry(entry)}`)
  ]);
}

function renderMultiSelectionList(
  label: string,
  entries: AuthProfileEntry[],
  cursor: number,
  selectedIndexes: Set<number>,
  render: (lines: string[]) => void
) {
  render([
    label,
    "Use Up/Down to move, Space to toggle, Enter to confirm, q to cancel.",
    "",
    ...entries.map(
      (entry, index) =>
        `${index === cursor ? "> " : "  "}[${selectedIndexes.has(index) ? "x" : " "}] ${formatAuthProfileEntry(
          entry
        )}`
    )
  ]);
}

function wrapIndex(index: number, length: number) {
  return ((index % length) + length) % length;
}

function isCancelKey(key: KeypressKey) {
  return key.name === "escape" || key.name === "q";
}

function addStatRangeOptions(command: Command) {
  command
    .option("--start <time>", "start time, defaults to one week before --end")
    .option("--end <time>", "end time, defaults to now")
    .option("--today", "use today as the time range")
    .option("--yesterday", "use yesterday as the time range")
    .option("--month", "use the current calendar month as the time range")
    .option("--last <duration>", "use a recent duration like 12h, 7d, 2w, or 1mo")
    .option("--codex-home <path>", "Codex home directory", process.env.CODEX_HOME)
    .option("--sessions-dir <path>", "Codex sessions directory");
}

function addStatFormatOptions(command: Command) {
  command
    .option("-f, --format <format>", "output format: table, json, csv, markdown", "table")
    .option("--json", "print JSON; alias for --format json");
}

function parseTopLimit(value: string | undefined) {
  const limit = Number(value ?? "10");

  if (!Number.isSafeInteger(limit) || limit <= 0) {
    throw new Error("Invalid --top value. Expected a positive integer.");
  }

  return limit;
}

function withTrailingNewline(text: string) {
  return text.endsWith("\n") ? text : `${text}\n`;
}

export async function run(argv = process.argv) {
  await createProgram().parseAsync(argv);
}

export function isMainModule(argvPath = process.argv[1]) {
  if (argvPath === undefined) {
    return false;
  }

  const modulePath = fileURLToPath(import.meta.url);
  const resolvedArgvPath = resolve(argvPath);

  try {
    return realpathSync(modulePath) === realpathSync(resolvedArgvPath);
  } catch {
    return modulePath === resolvedArgvPath;
  }
}

function readPackageVersion() {
  try {
    const packageJson = JSON.parse(
      readFileSync(new URL("../package.json", import.meta.url), "utf8")
    ) as { version?: unknown };

    return typeof packageJson.version === "string" ? packageJson.version : "0.0.0";
  } catch {
    return "0.0.0";
  }
}

function handleError(error: unknown) {
  const message = error instanceof Error ? error.message : String(error);
  console.error(pc.red(message));
  process.exitCode = 1;
}

if (isMainModule()) {
  run().catch(handleError);
}
