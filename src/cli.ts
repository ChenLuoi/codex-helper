#!/usr/bin/env node

import { Command } from "commander";
import inquirer, { type Answers, type Question } from "inquirer";
import { readFileSync, realpathSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import ora from "ora";
import pc from "picocolors";
import { formatDoctorReport, readDoctorReport } from "./doctor.js";
import {
  addWeeklyCycleAnchorsToFile,
  buildWeeklyCycleDetailReport,
  buildWeeklyCycleCurrentReport,
  buildWeeklyCycleHistoryReport,
  formatWeeklyCycleAnchorList,
  formatWeeklyCycleCurrent,
  formatWeeklyCycleDetail,
  formatWeeklyCycleHistory,
  listWeeklyCycleAnchorsFromFile,
  removeWeeklyCycleAnchorFromFile,
  type WeeklyCycleAccountSource,
  type WeeklyCycleAnchor,
  type WeeklyCycleReportRow
} from "./cycles.js";
import {
  formatAuthProfileEntry,
  formatAuthProfileList,
  formatAuthStatus,
  ensureCodexAuthAccountHistory,
  listCodexAuthProfiles,
  readCodexAuthAccountHistory,
  readCodexAuthStatus,
  removeCodexAuthProfile,
  saveCurrentCodexAuthProfile,
  switchCodexAuthProfile,
  toAuthAccountUsageHistory,
  type AuthProfileEntry,
  type AuthStatusSummary
} from "./index.js";
import {
  formatUsageSessionDetail,
  formatUsageSessions,
  formatUsageStats,
  readCodexUsageRecordsReport,
  readCodexUsageSessionDetail,
  readCodexUsageSessions,
  readCodexUsageStats,
  resolveStatRangeOptions,
  resolveStatOptions,
  type StatFormat,
  type UsageAccountHistory,
  type UsageDiagnostics,
  type UsageRecord
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
    .option("-j, --json", "print JSON")
    .option("--include-token-claims", "include decoded JWT header and claims in JSON output")
    .action(async (options: AuthStatusCommandOptions) => {
      const report = await readCodexAuthStatus({
        authFile: options.authFile,
        codexHome: options.codexHome
      });
      output.write(
        withTrailingNewline(
          formatAuthStatus(report, options.json === true ? "json" : "table", {
            includeTokenClaims: options.includeTokenClaims === true
          })
        )
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
    .option("--account-history-file <path>", "auth account history file")
    .option("-A, --account-id <id>", "activate a specific persisted account id")
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
    .option("-A, --account-id <id>", "remove a specific persisted account id")
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
    .option("--auth-file <path>", "path to auth.json")
    .option("--codex-home <path>", "Codex home directory", process.env.CODEX_HOME)
    .option("--sessions-dir <path>", "Codex sessions directory")
    .option("--cycle-file <path>", "weekly cycle anchor store file")
    .option("-j, --json", "print JSON")
    .action(async (options: DoctorCommandOptions) => {
      const spinner = options.json === true ? undefined : ora("Checking local environment").start();
      try {
        const report = await readDoctorReport({
          authFile: options.authFile,
          codexHome: options.codexHome,
          sessionsDir: options.sessionsDir,
          cycleFile: options.cycleFile
        });
        const errors = report.checks.filter((check) => check.status === "error").length;
        const warnings = report.checks.filter((check) => check.status === "warn").length;
        spinner?.succeed(`Finished with ${errors} error(s), ${warnings} warning(s).`);
        output.write(withTrailingNewline(formatDoctorReport(report, options.json === true ? "json" : "table")));
      } catch (error) {
        spinner?.fail("Failed to check local environment.");
        throw error;
      }
    });

  const statCommand = program
    .command("stat [view] [session]")
    .description("Show Codex session token usage statistics.")
    .option("-g, --group-by <group>", "aggregation: hour, day, week, month, model, cwd, account")
    .option("-S, --sort <sort>", "sort rows by: time, tokens, credits, calls, sessions")
    .option("-n, --limit <n>", "maximum number of rows to show")
    .option("-T, --top <n>", "number of sessions to show when view is sessions")
    .option("-d, --detail", "show full event-level rows for stat sessions <session-id>")
    .option("-F, --full-scan", "scan all session files instead of pruning by date")
    .option("-a, --all", "include all session usage instead of a date range")
    .option("-r, --reasoning-effort", "include reasoning effort in model grouping")
    .option("-A, --account-id <id>", "only include usage attributed to an account id")
    .option("--auth-file <path>", "path to auth.json for account history initialization")
    .option("--account-history-file <path>", "auth account history file")
    .option("-v, --verbose", "show scan and parsing diagnostics");
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
          const statOptions = {
            ...resolveStatOptions(commandOptions),
            scanAllFiles: commandOptions.fullScan === true
          };
          const accountOptions = await resolveUsageAccountOptions(commandOptions, {
            groupBy: statOptions.groupBy
          });
          spinner =
            statOptions.format === "table" ? ora("Reading Codex session usage").start() : undefined;
          const report = await readCodexUsageStats({ ...statOptions, ...accountOptions });
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
          const accountOptions = await resolveUsageAccountOptions(commandOptions);

          if (session !== undefined) {
            const detailOptions = {
              ...sessionOptions,
              ...accountOptions,
              scanAllFiles: true
            };
            spinner =
              sessionOptions.format === "table"
                ? ora("Reading Codex session usage").start()
                : undefined;
            const report = await readCodexUsageSessionDetail(detailOptions, session);
            spinner?.succeed(`Read ${report.totals.calls} usage events.`);
            output.write(
              withTrailingNewline(
                formatUsageSessionDetail(report, sessionOptions.format, {
                  verbose: sessionOptions.verbose,
                  detail: commandOptions.detail === true
                })
              )
            );
            return;
          }

          const top =
            commandOptions.top === undefined
              ? sessionOptions.limit ?? 10
              : parseTopLimit(commandOptions.top);
          const listOptions = {
            ...sessionOptions,
            ...accountOptions,
            scanAllFiles: commandOptions.fullScan === true
          };
          spinner =
            sessionOptions.format === "table" ? ora("Reading Codex session usage").start() : undefined;
          const report = await readCodexUsageSessions(listOptions, top);
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
  addCycleCommands(program, output);

  return program;
}

type StatCommandOptions = {
  start?: string;
  end?: string;
  groupBy?: string;
  format?: string;
  codexHome?: string;
  sessionsDir?: string;
  authFile?: string;
  accountHistoryFile?: string;
  today?: boolean;
  yesterday?: boolean;
  month?: boolean;
  all?: boolean;
  reasoningEffort?: boolean;
  accountId?: string;
  last?: string;
  sort?: string;
  limit?: string;
  detail?: boolean;
  fullScan?: boolean;
  verbose?: boolean;
  json?: boolean;
};

type CycleBaseOptions = {
  authFile?: string;
  codexHome?: string;
  cycleFile?: string;
  accountHistoryFile?: string;
  accountId?: string;
};

type CycleFormattedOptions = CycleBaseOptions & {
  format?: string;
  json?: boolean;
};

type CycleAnchorAddOptions = CycleBaseOptions & {
  note?: string;
};

type CycleUsageOptions = CycleFormattedOptions & {
  sessionsDir?: string;
  start?: string;
  end?: string;
  today?: boolean;
  yesterday?: boolean;
  month?: boolean;
  all?: boolean;
  last?: string;
  estimateBeforeAnchor?: boolean;
  select?: boolean;
};

type CycleUsageReadResult = {
  records: UsageRecord[];
  diagnostics?: UsageDiagnostics;
};

async function resolveUsageAccountOptions(
  options: Pick<StatCommandOptions, "accountId" | "authFile" | "codexHome" | "accountHistoryFile">,
  context: { groupBy?: string } = {}
): Promise<{ accountHistory?: UsageAccountHistory; accountId?: string }> {
  if (options.accountId === undefined && context.groupBy !== "account") {
    return {};
  }

  const report = await ensureCodexAuthAccountHistory({
    authFile: options.authFile,
    codexHome: options.codexHome,
    accountHistoryFile: options.accountHistoryFile
  });

  return {
    accountHistory: toAuthAccountUsageHistory(report.store),
    accountId: options.accountId
  };
}

type AuthStatusCommandOptions = {
  authFile?: string;
  codexHome?: string;
  json?: boolean;
  includeTokenClaims?: boolean;
};

type DoctorCommandOptions = {
  authFile?: string;
  codexHome?: string;
  sessionsDir?: string;
  cycleFile?: string;
  json?: boolean;
};

type AuthProfileCommandOptions = {
  authFile?: string;
  codexHome?: string;
  storeDir?: string;
  accountHistoryFile?: string;
  accountId?: string;
  yes?: boolean;
};

function toAuthProfileOptions(options: AuthProfileCommandOptions) {
  return {
    authFile: options.authFile,
    codexHome: options.codexHome,
    storeDir: options.storeDir,
    accountHistoryFile: options.accountHistoryFile
  };
}

function addCycleCommands(program: Command, output: NodeJS.WritableStream) {
  const cycleCommand = program
    .command("cycle")
    .description("Manage Codex weekly limit cycle anchors and usage reports.");

  const addCommand = cycleCommand
    .command("add")
    .description("Add a weekly cycle anchor.")
    .argument("<time...>", "weekly cycle start time")
    .option("-n, --note <text>", "anchor note");
  addCycleStateOptions(addCommand);
  addCommand.action(async (timeParts: string[], options: CycleAnchorAddOptions) => {
    const report = await addWeeklyCycleAnchorsToFile({
      ...(await toCycleStateOptions(options)),
      at: parseCycleAddTimes(timeParts),
      note: options.note
    });
    const accountLabel = await resolveCycleAccountLabel(report.accountId, options);

    if (report.anchors.length === 1) {
      const anchor = report.anchors[0];
      if (anchor === undefined) {
        throw new Error("No weekly cycle anchor was added.");
      }
      output.write(`Added weekly cycle anchor: ${anchor.id}\n`);
      output.write(`At: ${anchor.at}\n`);
    } else {
      output.write(`Added ${report.anchors.length} weekly cycle anchors:\n`);
      for (const anchor of report.anchors) {
        output.write(`- ${anchor.id} at ${anchor.at}\n`);
      }
    }

    output.write(formatCycleAccountLine(report.accountId, report.accountSource, accountLabel));
    output.write(`Cycle file: ${report.cycleFile}\n`);
  });

  const listCommand = cycleCommand
    .command("list")
    .description("List weekly cycle anchors.");
  addCycleStateOptions(listCommand);
  addStatFormatOptions(listCommand);
  listCommand.action(async (options: CycleFormattedOptions) => {
    const report = await listWeeklyCycleAnchorsFromFile(await toCycleStateOptions(options));
    const context = await toCycleReportContext(report, options);
    output.write(
      withTrailingNewline(formatWeeklyCycleAnchorList(report, resolveCycleFormat(options), context))
    );
  });

  const removeCommand = cycleCommand
    .command("remove")
    .description("Remove a weekly cycle anchor.")
    .argument("<id>", "anchor id");
  addCycleStateOptions(removeCommand);
  removeCommand.action(async (anchorId: string, options: CycleBaseOptions) => {
    const report = await removeWeeklyCycleAnchorFromFile(
      anchorId,
      await toCycleStateOptions(options)
    );
    const accountLabel = await resolveCycleAccountLabel(report.accountId, options);

    output.write(`Removed weekly cycle anchor: ${report.anchor.id}\n`);
    output.write(formatCycleAccountLine(report.accountId, report.accountSource, accountLabel));
    output.write(`Cycle file: ${report.cycleFile}\n`);
  });

  const currentCommand = cycleCommand
    .command("current")
    .description("Show the current weekly cycle.");
  addCycleStateOptions(currentCommand);
  currentCommand.option("--sessions-dir <path>", "Codex sessions directory");
  addStatFormatOptions(currentCommand);
  currentCommand.action(async (options: CycleUsageOptions) => {
    const format = resolveCycleFormat(options);
    const anchorReport = await listWeeklyCycleAnchorsFromFile(await toCycleStateOptions(options));
    const context = await toCycleReportContext(anchorReport, options);
    const now = new Date();
    const usageReport = await readWeeklyCycleUsageForCurrent(
      anchorReport.anchors,
      anchorReport.accountId,
      options,
      now
    );
    const report = buildWeeklyCycleCurrentReport({
      anchors: anchorReport.anchors,
      records: usageReport.records,
      now,
      usageDiagnostics: usageReport.diagnostics
    });

    output.write(
      withTrailingNewline(
        formatWeeklyCycleCurrent(report, format, {
          ...context
        })
      )
    );
  });

  const historyCommand = cycleCommand
    .command("history")
    .description("Show weekly cycle history.")
    .argument("[cycle-id]", "cycle id to show in detail")
    .option("-i, --select", "interactively select a cycle to show in detail")
    .option("--estimate-before-anchor", "include estimated cycles before the earliest anchor");
  addCycleStateOptions(historyCommand, { codexHome: false });
  addStatRangeOptions(historyCommand);
  addStatFormatOptions(historyCommand);
  historyCommand.action(async (cycleId: string | undefined, options: CycleUsageOptions) => {
    const format = resolveCycleFormat(options);
    const rangeOptions = resolveCycleHistoryRangeOptions(options);
    const anchorReport = await listWeeklyCycleAnchorsFromFile(await toCycleStateOptions(options));
    const context = await toCycleReportContext(anchorReport, options);
    const usageReport = await readWeeklyCycleUsageForHistory(
      anchorReport.anchors,
      anchorReport.accountId,
      options,
      rangeOptions
    );
    const report = buildWeeklyCycleHistoryReport({
      anchors: anchorReport.anchors,
      records: usageReport.records,
      start: rangeOptions.start,
      end: rangeOptions.end,
      estimateBeforeAnchor: options.estimateBeforeAnchor === true,
      usageDiagnostics: usageReport.diagnostics
    });

    const selectedCycleId = await resolveCycleHistoryDetailId(cycleId, options, report.rows, output);
    if (selectedCycleId !== undefined) {
      const detail = buildWeeklyCycleDetailReport({
        history: report,
        cycleId: selectedCycleId,
        records: usageReport.records,
        usageDiagnostics: usageReport.diagnostics
      });

      output.write(
        withTrailingNewline(
          formatWeeklyCycleDetail(detail, format, {
            ...context
          })
        )
      );
      return;
    }

    output.write(
      withTrailingNewline(
        formatWeeklyCycleHistory(report, format, {
          ...context
        })
      )
    );
  });
}

function resolveCycleHistoryRangeOptions(options: CycleUsageOptions) {
  return resolveStatRangeOptions({
    ...options,
    all: hasExplicitCycleHistoryRange(options) ? options.all : true
  });
}

function hasExplicitCycleHistoryRange(options: CycleUsageOptions) {
  return (
    options.all === true ||
    options.today === true ||
    options.yesterday === true ||
    options.month === true ||
    options.last !== undefined ||
    options.start !== undefined ||
    options.end !== undefined
  );
}

function parseCycleAddTimes(timeParts: string[]) {
  const tokens = timeParts.flatMap((part) =>
    part
      .split(",")
      .map((token) => token.trim())
      .filter((token) => token.length > 0)
  );
  const times: string[] = [];

  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index];
    const next = tokens[index + 1];

    if (token === undefined) {
      continue;
    }

    if (next !== undefined && isDateOnlyToken(token) && isTimeOnlyToken(next)) {
      times.push(`${token} ${next}`);
      index += 1;
      continue;
    }

    times.push(token);
  }

  if (times.length === 0) {
    throw new Error("cycle add requires at least one weekly cycle start time.");
  }

  return times;
}

function isDateOnlyToken(value: string) {
  return /^\d{4}-\d{2}-\d{2}$/.test(value);
}

function isTimeOnlyToken(value: string) {
  return /^\d{2}:\d{2}(?::\d{2})?(?:Z|[+-]\d{2}:\d{2})?$/.test(value);
}

function addCycleStateOptions(command: Command, options: { codexHome?: boolean } = {}) {
  command.option("--auth-file <path>", "path to auth.json");

  if (options.codexHome !== false) {
    command.option("--codex-home <path>", "Codex home directory", process.env.CODEX_HOME);
  }

  command
    .option("--cycle-file <path>", "weekly cycle anchor store file")
    .option("--account-history-file <path>", "auth account history file")
    .option("-A, --account-id <id>", "weekly cycle account id");
}

async function toCycleStateOptions(options: CycleBaseOptions) {
  return {
    authStatus: await readCycleAuthStatus(options),
    codexHome: options.codexHome,
    cycleFile: options.cycleFile,
    accountId: options.accountId
  };
}

async function toCycleReportContext(
  report: { accountId: string; accountSource: WeeklyCycleAccountSource; cycleFile: string },
  options: Pick<CycleBaseOptions, "authFile" | "codexHome" | "accountHistoryFile">
) {
  return {
    accountId: report.accountId,
    accountLabel: await resolveCycleAccountLabel(report.accountId, options),
    accountSource: report.accountSource,
    cycleFile: report.cycleFile
  };
}

async function resolveCycleAccountLabel(
  accountId: string,
  options: Pick<CycleBaseOptions, "authFile" | "codexHome" | "accountHistoryFile">
) {
  const authStatus = await readOptionalCodexAuthStatus(options);
  const authAccountId =
    authStatus?.summary.chatgptAccountId ?? authStatus?.summary.tokenAccountId;

  if (authStatus !== undefined && authAccountId === accountId) {
    return formatCycleAccountLabel(accountId, authStatus.summary);
  }

  const profileLabel = await readStoredCycleAccountLabel(accountId, options);

  if (profileLabel !== undefined) {
    return profileLabel;
  }

  return readHistoryDefaultCycleAccountLabel(accountId, options);
}

async function readOptionalCodexAuthStatus(
  options: Pick<CycleBaseOptions, "authFile" | "codexHome">
) {
  try {
    return await readCodexAuthStatus({
      authFile: options.authFile,
      codexHome: options.codexHome
    });
  } catch {
    return undefined;
  }
}

async function readStoredCycleAccountLabel(
  accountId: string,
  options: Pick<CycleBaseOptions, "authFile" | "codexHome">
) {
  try {
    const profiles = await listCodexAuthProfiles({
      authFile: options.authFile,
      codexHome: options.codexHome
    });
    const profile = [profiles.current, ...profiles.stored].find(
      (entry) => entry?.accountId === accountId
    );

    return profile === undefined ? undefined : formatCycleAccountLabel(accountId, profile.summary);
  } catch {
    return undefined;
  }
}

async function readHistoryDefaultCycleAccountLabel(
  accountId: string,
  options: Pick<CycleBaseOptions, "codexHome" | "accountHistoryFile">
) {
  try {
    const history = await readCodexAuthAccountHistory({
      codexHome: options.codexHome,
      accountHistoryFile: options.accountHistoryFile
    });
    const defaultAccount = history.store.defaultAccount;

    if (defaultAccount?.accountId !== accountId) {
      return undefined;
    }

    return formatCycleAccountLabel(accountId, defaultAccount);
  } catch {
    return undefined;
  }
}

function formatCycleAccountLabel(
  accountId: string,
  account: Pick<AuthStatusSummary, "email" | "name"> | { email?: string; name?: string }
) {
  const label = account.email ?? account.name;
  return label === undefined || label.length === 0 ? undefined : `${label}(${accountId})`;
}

function formatCycleAccountLine(
  accountId: string,
  _accountSource: WeeklyCycleAccountSource,
  accountLabel: string | undefined
) {
  return `Account: ${accountLabel ?? accountId}\n`;
}

async function readCycleAuthStatus(options: CycleBaseOptions) {
  if (options.accountId !== undefined) {
    return undefined;
  }

  try {
    return await readCodexAuthStatus({
      authFile: options.authFile,
      codexHome: options.codexHome
    });
  } catch {
    return undefined;
  }
}

async function readWeeklyCycleUsageForCurrent(
  anchors: WeeklyCycleAnchor[],
  accountId: string,
  options: Pick<CycleUsageOptions, "authFile" | "codexHome" | "sessionsDir" | "accountHistoryFile">,
  now: Date
): Promise<CycleUsageReadResult> {
  const earliestAnchor = earliestAnchorDate(anchors);

  if (earliestAnchor === undefined || earliestAnchor > now) {
    return { records: [] };
  }

  const report = await readCodexUsageRecordsReport({
    sessionsDir: resolveCycleSessionsDir(options),
    start: earliestAnchor,
    end: now,
    scanAllFiles: true,
    ...(await resolveCycleUsageAccountOptions(options, accountId))
  });

  return {
    records: report.records,
    diagnostics: report.diagnostics
  };
}

async function readWeeklyCycleUsageForHistory(
  anchors: WeeklyCycleAnchor[],
  accountId: string,
  options: CycleUsageOptions,
  rangeOptions: ReturnType<typeof resolveStatRangeOptions>
): Promise<CycleUsageReadResult> {
  const earliestAnchor = earliestAnchorDate(anchors);
  const scanStart =
    earliestAnchor === undefined
      ? undefined
      : options.estimateBeforeAnchor === true && rangeOptions.start < earliestAnchor
        ? rangeOptions.start
        : earliestAnchor;

  if (scanStart === undefined || scanStart > rangeOptions.end) {
    return { records: [] };
  }

  const report = await readCodexUsageRecordsReport({
    sessionsDir: rangeOptions.sessionsDir,
    start: scanStart,
    end: rangeOptions.end,
    scanAllFiles: true,
    ...(await resolveCycleUsageAccountOptions(options, accountId))
  });

  return {
    records: report.records,
    diagnostics: report.diagnostics
  };
}

async function resolveCycleUsageAccountOptions(
  options: Pick<CycleUsageOptions, "authFile" | "codexHome" | "accountHistoryFile">,
  accountId: string
) {
  try {
    const report = await readCodexAuthAccountHistory({
      authFile: options.authFile,
      codexHome: options.codexHome,
      accountHistoryFile: options.accountHistoryFile
    });
    const accountHistory = toAuthAccountUsageHistory(report.store);

    if (accountHistory.defaultAccountId === undefined && accountHistory.switches.length === 0) {
      return {};
    }

    return {
      accountHistory,
      accountId
    };
  } catch (error) {
    if (!isNotFoundError(error)) {
      throw error;
    }

    return {};
  }
}

function earliestAnchorDate(anchors: WeeklyCycleAnchor[]) {
  return anchors
    .map((anchor) => new Date(anchor.at))
    .filter((date) => !Number.isNaN(date.getTime()))
    .sort((left, right) => left.getTime() - right.getTime())[0];
}

function resolveCycleSessionsDir(options: Pick<CycleUsageOptions, "codexHome" | "sessionsDir">) {
  return resolveStatRangeOptions({
    codexHome: options.codexHome,
    sessionsDir: options.sessionsDir,
    all: true
  }).sessionsDir;
}

function resolveCycleFormat(options: Pick<CycleFormattedOptions, "format" | "json">): StatFormat {
  const format = options.json === true ? "json" : options.format ?? "table";

  if (format === "table" || format === "json" || format === "csv" || format === "markdown") {
    return format;
  }

  throw new Error("Invalid format value. Expected one of: table, json, csv, markdown.");
}

async function resolveCycleHistoryDetailId(
  cycleId: string | undefined,
  options: Pick<CycleUsageOptions, "select">,
  rows: WeeklyCycleReportRow[],
  output: NodeJS.WritableStream
) {
  if (cycleId !== undefined && options.select === true) {
    throw new Error("cycle history accepts either a cycle id or --select, not both.");
  }

  if (cycleId !== undefined) {
    return cycleId;
  }

  if (options.select !== true) {
    return undefined;
  }

  if (rows.length === 0) {
    output.write("No weekly cycles to select.\n");
    return undefined;
  }

  if (!canPromptList()) {
    throw new Error("cycle history --select requires an interactive terminal unless a cycle id is supplied.");
  }

  const selected = await promptWeeklyCycleSelection("Select weekly cycle to show", rows, output);

  if (selected === undefined) {
    output.write("Cancelled.\n");
  }

  return selected?.id;
}

function canPromptLine() {
  return process.stdin.isTTY === true && process.stdout.isTTY === true;
}

function canPromptList() {
  return canPromptLine() && typeof process.stdin.setRawMode === "function";
}

const cancelPromptValue = "__codex_helper_cancel__";

async function promptWeeklyCycleSelection(
  label: string,
  rows: WeeklyCycleReportRow[],
  output: NodeJS.WritableStream
) {
  const answers = await runInquirerPrompt<{ cycleId: string }>(
    output,
    [
      {
        type: "select",
        name: "cycleId",
        message: label,
        pageSize: 12,
        choices: [
          ...rows.map((row) => ({
            name: formatWeeklyCycleChoice(row),
            value: row.id,
            short: row.id
          })),
          new inquirer.Separator(),
          { name: "Cancel", value: cancelPromptValue, short: "Cancel" }
        ]
      }
    ]
  );

  if (answers === undefined || answers.cycleId === cancelPromptValue) {
    return undefined;
  }

  return rows.find((row) => row.id === answers.cycleId);
}

async function promptAuthProfileSelection(
  label: string,
  entries: AuthProfileEntry[],
  output: NodeJS.WritableStream
) {
  const answers = await runInquirerPrompt<{ accountId: string }>(
    output,
    [
      {
        type: "select",
        name: "accountId",
        message: label,
        pageSize: 12,
        choices: [
          ...entries.map((entry) => ({
            name: formatAuthProfileEntry(entry),
            value: entry.accountId,
            short: entry.accountId
          })),
          new inquirer.Separator(),
          { name: "Cancel", value: cancelPromptValue, short: "Cancel" }
        ]
      }
    ]
  );

  if (answers === undefined || answers.accountId === cancelPromptValue) {
    return undefined;
  }

  return entries.find((entry) => entry.accountId === answers.accountId);
}

async function promptAuthProfileMultiSelection(
  label: string,
  entries: AuthProfileEntry[],
  output: NodeJS.WritableStream
) {
  const answers = await runInquirerPrompt<{ accountIds: string[] }>(
    output,
    [
      {
        type: "checkbox",
        name: "accountIds",
        message: label,
        pageSize: 12,
        choices: entries.map((entry) => ({
          name: formatAuthProfileEntry(entry),
          value: entry.accountId,
          short: entry.accountId
        }))
      }
    ]
  );

  if (answers === undefined) {
    return [];
  }

  const selectedAccountIds = new Set(answers.accountIds);

  return entries.filter((entry) => selectedAccountIds.has(entry.accountId));
}

async function promptConfirmation(question: string, output: NodeJS.WritableStream) {
  const answers = await runInquirerPrompt<{ confirmed: boolean }>(
    output,
    [
      {
        type: "confirm",
        name: "confirmed",
        message: question,
        default: false
      }
    ]
  );

  return answers?.confirmed === true;
}

async function runInquirerPrompt<T extends Answers>(
  output: NodeJS.WritableStream,
  questions: readonly InquirerQuestion<T>[]
) {
  const prompt = inquirer.createPromptModule({
    input: process.stdin,
    output
  });

  try {
    return await prompt(questions);
  } catch (error) {
    if (isInquirerExitPromptError(error)) {
      process.exitCode = 130;
      return undefined;
    }

    throw error;
  }
}

function isInquirerExitPromptError(error: unknown) {
  return error instanceof Error && error.name === "ExitPromptError";
}

type InquirerQuestion<T extends Answers> = Question<T> & Record<string, unknown>;

function formatWeeklyCycleChoice(row: WeeklyCycleReportRow) {
  return [
    row.id,
    row.source,
    `${formatLocalDateTime(row.start)} -> ${formatLocalDateTime(row.resetAt)}`,
    `${row.calls} call(s)`
  ].join(" | ");
}

function formatLocalDateTime(date: Date) {
  return [
    date.getFullYear(),
    pad2(date.getMonth() + 1),
    pad2(date.getDate())
  ].join("-") + ` ${pad2(date.getHours())}:${pad2(date.getMinutes())}:${pad2(date.getSeconds())}`;
}

function pad2(value: number) {
  return String(value).padStart(2, "0");
}

function addStatRangeOptions(command: Command) {
  command
    .option("-s, --start <time>", "start time, defaults to one week before --end")
    .option("-e, --end <time>", "end time, defaults to now")
    .option("-t, --today", "use today as the time range")
    .option("--yesterday", "use yesterday as the time range")
    .option("-m, --month", "use the current calendar month as the time range")
    .option("-L, --last <duration>", "use a recent duration like 12h, 7d, 2w, or 1mo")
    .option("--codex-home <path>", "Codex home directory", process.env.CODEX_HOME)
    .option("--sessions-dir <path>", "Codex sessions directory");
}

function addStatFormatOptions(command: Command) {
  command
    .option("-f, --format <format>", "output format: table, json, csv, markdown", "table")
    .option("-j, --json", "print JSON; alias for --format json");
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

function isNotFoundError(error: unknown): error is NodeJS.ErrnoException {
  return error instanceof Error && "code" in error && error.code === "ENOENT";
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
