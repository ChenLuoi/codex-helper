#!/usr/bin/env node

import { Command } from "commander";
import { readFileSync, realpathSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import ora from "ora";
import pc from "picocolors";
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
