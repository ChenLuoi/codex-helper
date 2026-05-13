import { createReadStream } from "node:fs";
import { open, readdir } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline";
import pc from "picocolors";
import { calculateCreditCost, normalizeModelName } from "./pricing.js";

export type StatGroupBy = "hour" | "day" | "week" | "month" | "model" | "cwd";
export type StatFormat = "table" | "json" | "csv" | "markdown";
export type StatSort = "time" | "tokens" | "credits" | "calls" | "sessions";

export type TokenUsage = {
  inputTokens: number;
  cachedInputTokens: number;
  outputTokens: number;
  reasoningOutputTokens: number;
  totalTokens: number;
};

export type UsageRecord = {
  timestamp: Date;
  sessionId: string;
  model: string;
  reasoningEffort?: string;
  cwd: string;
  filePath: string;
  usage: TokenUsage;
};

export type StatRangeOptions = {
  start: Date;
  end: Date;
  format: StatFormat;
  sessionsDir: string;
  sortBy?: StatSort;
  limit?: number;
  verbose: boolean;
};

export type UsageFileScanOptions = {
  scanAllFiles?: boolean;
};

export type StatOptions = StatRangeOptions & {
  groupBy: StatGroupBy;
  includeReasoningEffort: boolean;
};

export type RawStatOptions = {
  start?: string;
  end?: string;
  groupBy?: string;
  format?: string;
  codexHome?: string;
  sessionsDir?: string;
  today?: boolean;
  yesterday?: boolean;
  month?: boolean;
  all?: boolean;
  reasoningEffort?: boolean;
  last?: string;
  sort?: string;
  limit?: string | number;
  verbose?: boolean;
  json?: boolean;
};

export type UsageStatRow = {
  key: string;
  sessions: number;
  calls: number;
  usage: TokenUsage;
  credits: number;
  usd: number;
  pricedCalls: number;
  unpricedCalls: number;
};

export type UsageStatsReport = {
  start: Date;
  end: Date;
  groupBy: StatGroupBy;
  includeReasoningEffort: boolean;
  sortBy?: StatSort;
  limit?: number;
  sessionsDir: string;
  rows: UsageStatRow[];
  totals: UsageStatRow;
  unpricedModels: UsageUnpricedModelRow[];
  diagnostics?: UsageDiagnostics;
};

export type UsageRecordsReport = {
  start: Date;
  end: Date;
  sessionsDir: string;
  records: UsageRecord[];
  diagnostics: UsageDiagnostics;
};

export type UsageSessionRow = {
  sessionId: string;
  model: string;
  cwd: string;
  firstSeen: Date;
  lastSeen: Date;
  calls: number;
  usage: TokenUsage;
  credits: number;
  usd: number;
  pricedCalls: number;
  unpricedCalls: number;
  filePath: string;
};

export type UsageSessionEventRow = {
  timestamp: Date;
  model: string;
  reasoningEffort?: string;
  cwd: string;
  usage: TokenUsage;
  credits: number;
  usd: number;
  priced: boolean;
  filePath: string;
};

export type UsageSessionCompactRow = {
  start: Date;
  end: Date;
  events: number;
  model: string;
  reasoningEffort?: string;
  usage: TokenUsage;
  credits: number;
  usd: number;
  pricedCalls: number;
  unpricedCalls: number;
};

export type UsageSessionsReport = {
  start: Date;
  end: Date;
  sortBy?: StatSort;
  limit?: number;
  sessionsDir: string;
  rows: UsageSessionRow[];
  totals: UsageStatRow;
  unpricedModels: UsageUnpricedModelRow[];
  diagnostics?: UsageDiagnostics;
};

export type UsageSessionDetailReport = {
  start: Date;
  end: Date;
  sessionId: string;
  limit?: number;
  sessionsDir: string;
  summary?: UsageSessionRow;
  rows: UsageSessionEventRow[];
  byModel: UsageStatRow[];
  byCwd: UsageStatRow[];
  byReasoningEffort: UsageStatRow[];
  modelSwitches: number;
  cwdSwitches: number;
  reasoningEffortSwitches: number;
  totals: UsageStatRow;
  unpricedModels: UsageUnpricedModelRow[];
  diagnostics?: UsageDiagnostics;
};

export type UsageUnpricedModelRow = {
  model: string;
  pricingKey: string;
  calls: number;
  totalTokens: number;
  note?: string;
  pricingStub: string;
};

export type UsageDiagnostics = {
  scanAllFiles: boolean;
  scannedDirectories: number;
  skippedDirectories: number;
  readFiles: number;
  skippedFiles: number;
  prefilteredFiles: number;
  readLines: number;
  invalidJsonLines: number;
  tokenCountEvents: number;
  includedUsageEvents: number;
  skippedEvents: {
    missingMetadata: number;
    missingUsage: number;
    emptyUsage: number;
    outOfRange: number;
  };
  fileReadConcurrency: number;
};

const EMPTY_USAGE: TokenUsage = {
  inputTokens: 0,
  cachedInputTokens: 0,
  outputTokens: 0,
  reasoningOutputTokens: 0,
  totalTokens: 0
};
const DEFAULT_FILE_READ_CONCURRENCY = 8;
const FULL_SCAN_PREFILTER_CHUNK_SIZE = 64 * 1024;
const DEFAULT_SESSION_DETAIL_COMPACT_ROWS = 20;
const ALL_USAGE_RANGE_START = new Date(1900, 0, 1);
const ALL_USAGE_RANGE_END = new Date(9999, 11, 31, 23, 59, 59, 999);

type MutableStatRow = {
  key: string;
  sessions: Set<string>;
  calls: number;
  usage: TokenUsage;
  credits: number;
  pricedCalls: number;
  unpricedCalls: number;
};

type MutableSession = {
  sessionId: string;
  model: string;
  cwd: string;
  firstSeen: Date;
  lastSeen: Date;
  calls: number;
  usage: TokenUsage;
  credits: number;
  pricedCalls: number;
  unpricedCalls: number;
  filePath: string;
};

export function resolveStatOptions(raw: RawStatOptions = {}, now = new Date()): StatOptions {
  const rangeOptions = resolveStatRangeOptions(raw, now);
  const groupBy = resolveGroupBy(raw.groupBy, raw, rangeOptions);

  return {
    ...rangeOptions,
    groupBy,
    includeReasoningEffort: raw.reasoningEffort === true
  };
}

export function resolveStatRangeOptions(
  raw: RawStatOptions = {},
  now = new Date()
): StatRangeOptions {
  const format = raw.json === true ? "json" : parseFormat(raw.format);
  const range = resolveDateRange(raw, now);

  if (range.start.getTime() > range.end.getTime()) {
    throw new Error("The stat start time must be earlier than or equal to the end time.");
  }

  return {
    start: range.start,
    end: range.end,
    format,
    sessionsDir: raw.sessionsDir ?? join(raw.codexHome ?? defaultCodexHome(), "sessions"),
    sortBy: parseSort(raw.sort),
    limit: parseOptionalLimit(raw.limit, "--limit"),
    verbose: raw.verbose === true
  };
}

export async function readCodexUsageRecords(
  options: Pick<StatRangeOptions, "sessionsDir" | "start" | "end"> & UsageFileScanOptions
) {
  return (await readCodexUsageRecordsReport(options)).records;
}

export async function readCodexUsageRecordsReport(
  options: Pick<StatRangeOptions, "sessionsDir" | "start" | "end"> & UsageFileScanOptions
): Promise<UsageRecordsReport> {
  const records: UsageRecord[] = [];
  const diagnostics = await processCodexUsageRecords(options, (record) => records.push(record));

  return {
    start: options.start,
    end: options.end,
    sessionsDir: options.sessionsDir,
    records,
    diagnostics
  };
}

export async function readCodexUsageStats(
  options: Pick<StatOptions, "start" | "end" | "groupBy" | "sessionsDir"> &
    Partial<Pick<StatOptions, "includeReasoningEffort" | "sortBy" | "limit">> &
    UsageFileScanOptions
): Promise<UsageStatsReport> {
  const accumulator = createUsageStatsAccumulator(options);
  const diagnostics = await processCodexUsageRecords(options, (record) => accumulator.add(record));

  return accumulator.finish(diagnostics);
}

export async function readCodexUsageSessions(
  options: Pick<StatRangeOptions, "start" | "end" | "sessionsDir"> &
    Partial<Pick<StatRangeOptions, "sortBy" | "limit">> &
    UsageFileScanOptions,
  limit = 10
): Promise<UsageSessionsReport> {
  const accumulator = createUsageSessionsAccumulator(options, limit);
  const diagnostics = await processCodexUsageRecords(options, (record) => accumulator.add(record));

  return accumulator.finish(diagnostics);
}

export async function readCodexUsageSessionDetail(
  options: Pick<StatRangeOptions, "start" | "end" | "sessionsDir"> &
    Partial<Pick<StatRangeOptions, "limit">> &
    UsageFileScanOptions,
  sessionId: string
): Promise<UsageSessionDetailReport> {
  const accumulator = createUsageSessionDetailAccumulator(options, sessionId);
  const diagnostics = await processCodexUsageRecords(options, (record) => accumulator.add(record));

  return accumulator.finish(diagnostics);
}

export function buildUsageStats(
  records: Iterable<UsageRecord>,
  options: Pick<StatOptions, "start" | "end" | "groupBy" | "sessionsDir"> &
    Partial<Pick<StatOptions, "includeReasoningEffort" | "sortBy" | "limit">>
): UsageStatsReport {
  const accumulator = createUsageStatsAccumulator(options);
  for (const record of records) {
    accumulator.add(record);
  }

  return accumulator.finish();
}

export function buildUsageSessions(
  records: Iterable<UsageRecord>,
  options: Pick<StatRangeOptions, "start" | "end" | "sessionsDir"> &
    Partial<Pick<StatRangeOptions, "sortBy" | "limit">>,
  limit = 10
): UsageSessionsReport {
  const accumulator = createUsageSessionsAccumulator(options, limit);
  for (const record of records) {
    accumulator.add(record);
  }

  return accumulator.finish();
}

export function buildUsageSessionDetail(
  records: Iterable<UsageRecord>,
  options: Pick<StatRangeOptions, "start" | "end" | "sessionsDir"> &
    Partial<Pick<StatRangeOptions, "limit">>,
  sessionId: string
): UsageSessionDetailReport {
  const accumulator = createUsageSessionDetailAccumulator(options, sessionId);
  for (const record of records) {
    accumulator.add(record);
  }

  return accumulator.finish();
}

function createUsageStatsAccumulator(
  options: Pick<StatOptions, "start" | "end" | "groupBy" | "sessionsDir"> &
    Partial<Pick<StatOptions, "includeReasoningEffort" | "sortBy" | "limit">>
) {
  const rows = new Map<string, MutableStatRow>();
  const totalSessions = new Set<string>();
  const unpricedModels = new Map<string, UsageUnpricedModelRow>();
  const totals = { ...EMPTY_USAGE };
  let calls = 0;

  return {
    add(record: UsageRecord) {
      const key = getGroupKey(record, options.groupBy, options.includeReasoningEffort === true);
      const row =
        rows.get(key) ??
        {
          key,
          sessions: new Set<string>(),
          calls: 0,
          usage: { ...EMPTY_USAGE },
          credits: 0,
          pricedCalls: 0,
          unpricedCalls: 0
        };
      const cost = calculateCreditCost(record.model, record.usage);

      row.sessions.add(record.sessionId);
      row.calls += 1;
      addUsage(row.usage, record.usage);
      row.credits += cost.credits;

      if (cost.priced) {
        row.pricedCalls += 1;
      } else {
        row.unpricedCalls += 1;
        addUnpricedModel(unpricedModels, record);
      }

      rows.set(key, row);
      totalSessions.add(record.sessionId);
      addUsage(totals, record.usage);
      calls += 1;
    },

    finish(diagnostics?: UsageDiagnostics): UsageStatsReport {
      const formattedRows = [...rows.values()].map((row) => ({
        key: row.key,
        sessions: row.sessions.size,
        calls: row.calls,
        usage: row.usage,
        credits: roundCredits(row.credits),
        usd: creditsToUsd(row.credits),
        pricedCalls: row.pricedCalls,
        unpricedCalls: row.unpricedCalls
      }));

      const sortBy = options.sortBy;
      formattedRows.sort((left, right) => compareStatRows(left, right, sortBy, options.groupBy));
      const outputRows =
        options.limit === undefined ? formattedRows : formattedRows.slice(0, options.limit);

      return {
        start: options.start,
        end: options.end,
        groupBy: options.groupBy,
        includeReasoningEffort: options.includeReasoningEffort === true,
        sortBy,
        limit: options.limit,
        sessionsDir: options.sessionsDir,
        rows: outputRows,
        totals: {
          key: "Total",
          sessions: totalSessions.size,
          calls,
          usage: totals,
          credits: roundCredits(formattedRows.reduce((sum, row) => sum + row.credits, 0)),
          usd: creditsToUsd(formattedRows.reduce((sum, row) => sum + row.credits, 0)),
          pricedCalls: formattedRows.reduce((sum, row) => sum + row.pricedCalls, 0),
          unpricedCalls: formattedRows.reduce((sum, row) => sum + row.unpricedCalls, 0)
        },
        unpricedModels: formatUnpricedModels(unpricedModels),
        diagnostics
      };
    }
  };
}

function createUsageSessionsAccumulator(
  options: Pick<StatRangeOptions, "start" | "end" | "sessionsDir"> &
    Partial<Pick<StatRangeOptions, "sortBy">>,
  limit = 10
) {
  const sessions = new Map<string, MutableSession>();
  const unpricedModels = new Map<string, UsageUnpricedModelRow>();
  const totals = { ...EMPTY_USAGE };
  let calls = 0;

  return {
    add(record: UsageRecord) {
      const session =
        sessions.get(record.sessionId) ??
        {
          sessionId: record.sessionId,
          model: record.model,
          cwd: record.cwd,
          firstSeen: record.timestamp,
          lastSeen: record.timestamp,
          calls: 0,
          usage: { ...EMPTY_USAGE },
          credits: 0,
          pricedCalls: 0,
          unpricedCalls: 0,
          filePath: record.filePath
        };
      const cost = calculateCreditCost(record.model, record.usage);

      session.model = record.model === "unknown" ? session.model : record.model;
      session.cwd = record.cwd === "unknown" ? session.cwd : record.cwd;
      session.firstSeen = record.timestamp < session.firstSeen ? record.timestamp : session.firstSeen;
      session.lastSeen = record.timestamp > session.lastSeen ? record.timestamp : session.lastSeen;
      session.calls += 1;
      addUsage(session.usage, record.usage);
      session.credits += cost.credits;

      if (cost.priced) {
        session.pricedCalls += 1;
      } else {
        session.unpricedCalls += 1;
        addUnpricedModel(unpricedModels, record);
      }

      sessions.set(record.sessionId, session);
      addUsage(totals, record.usage);
      calls += 1;
    },

    finish(diagnostics?: UsageDiagnostics): UsageSessionsReport {
      const sessionRows = [...sessions.values()];
      const sortBy = options.sortBy;
      const rows = sessionRows
        .map((session) => ({
          ...session,
          credits: roundCredits(session.credits),
          usd: creditsToUsd(session.credits)
        }))
        .sort((left, right) => compareSessionRows(left, right, sortBy))
        .slice(0, Math.max(0, limit));

      return {
        start: options.start,
        end: options.end,
        sortBy,
        limit,
        sessionsDir: options.sessionsDir,
        rows,
        totals: {
          key: "Total",
          sessions: sessions.size,
          calls,
          usage: totals,
          credits: roundCredits(sessionRows.reduce((sum, row) => sum + row.credits, 0)),
          usd: creditsToUsd(sessionRows.reduce((sum, row) => sum + row.credits, 0)),
          pricedCalls: sessionRows.reduce((sum, row) => sum + row.pricedCalls, 0),
          unpricedCalls: sessionRows.reduce((sum, row) => sum + row.unpricedCalls, 0)
        },
        unpricedModels: formatUnpricedModels(unpricedModels),
        diagnostics
      };
    }
  };
}

function createUsageSessionDetailAccumulator(
  options: Pick<StatRangeOptions, "start" | "end" | "sessionsDir"> &
    Partial<Pick<StatRangeOptions, "limit">>,
  sessionId: string
) {
  const rows: UsageSessionEventRow[] = [];
  const unpricedModels = new Map<string, UsageUnpricedModelRow>();
  const totals = { ...EMPTY_USAGE };
  let summary: MutableSession | undefined;
  let calls = 0;
  let credits = 0;
  let pricedCalls = 0;
  let unpricedCalls = 0;

  return {
    add(record: UsageRecord) {
      if (record.sessionId !== sessionId) {
        return;
      }

      const cost = calculateCreditCost(record.model, record.usage);

      summary ??= {
        sessionId: record.sessionId,
        model: record.model,
        cwd: record.cwd,
        firstSeen: record.timestamp,
        lastSeen: record.timestamp,
        calls: 0,
        usage: { ...EMPTY_USAGE },
        credits: 0,
        pricedCalls: 0,
        unpricedCalls: 0,
        filePath: record.filePath
      };

      summary.model = record.model === "unknown" ? summary.model : record.model;
      summary.cwd = record.cwd === "unknown" ? summary.cwd : record.cwd;
      summary.firstSeen = record.timestamp < summary.firstSeen ? record.timestamp : summary.firstSeen;
      summary.lastSeen = record.timestamp > summary.lastSeen ? record.timestamp : summary.lastSeen;
      summary.calls += 1;
      addUsage(summary.usage, record.usage);
      summary.credits += cost.credits;

      calls += 1;
      credits += cost.credits;
      addUsage(totals, record.usage);

      if (cost.priced) {
        pricedCalls += 1;
        summary.pricedCalls += 1;
      } else {
        unpricedCalls += 1;
        summary.unpricedCalls += 1;
        addUnpricedModel(unpricedModels, record);
      }

      rows.push({
        timestamp: record.timestamp,
        model: record.model,
        reasoningEffort: record.reasoningEffort,
        cwd: record.cwd,
        usage: record.usage,
        credits: roundCredits(cost.credits),
        usd: creditsToUsd(cost.credits),
        priced: cost.priced,
        filePath: record.filePath
      });
    },

    finish(diagnostics?: UsageDiagnostics): UsageSessionDetailReport {
      const sortedRows = rows.sort(
        (left, right) =>
          left.timestamp.getTime() - right.timestamp.getTime() ||
          left.model.localeCompare(right.model) ||
          left.filePath.localeCompare(right.filePath)
      );
      const outputRows =
        options.limit === undefined ? sortedRows : sortedRows.slice(0, options.limit);
      const byModel = buildSessionEventBreakdown(sortedRows, (row) => row.model);
      const byCwd = buildSessionEventBreakdown(sortedRows, (row) => row.cwd);
      const byReasoningEffort = buildSessionEventBreakdown(sortedRows, (row) =>
        row.reasoningEffort ?? "unknown"
      );

      return {
        start: options.start,
        end: options.end,
        sessionId,
        limit: options.limit,
        sessionsDir: options.sessionsDir,
        summary:
          summary === undefined
            ? undefined
            : {
                ...summary,
                credits: roundCredits(summary.credits),
                usd: creditsToUsd(summary.credits)
              },
        rows: outputRows,
        byModel,
        byCwd,
        byReasoningEffort,
        modelSwitches: countValueSwitches(sortedRows, (row) => row.model),
        cwdSwitches: countValueSwitches(sortedRows, (row) => row.cwd),
        reasoningEffortSwitches: countValueSwitches(
          sortedRows,
          (row) => row.reasoningEffort ?? "unknown"
        ),
        totals: {
          key: "Total",
          sessions: summary === undefined ? 0 : 1,
          calls,
          usage: totals,
          credits: roundCredits(credits),
          usd: creditsToUsd(credits),
          pricedCalls,
          unpricedCalls
        },
        unpricedModels: formatUnpricedModels(unpricedModels),
        diagnostics
      };
    }
  };
}

function compareStatRows(
  left: UsageStatRow,
  right: UsageStatRow,
  sortBy: StatSort | undefined,
  groupBy: StatGroupBy
) {
  if (sortBy === undefined) {
    if (groupBy === "model") {
      return byTokensDesc(left, right) || left.key.localeCompare(right.key);
    }

    return left.key.localeCompare(right.key);
  }

  switch (sortBy) {
    case "time":
      return left.key.localeCompare(right.key);
    case "tokens":
      return byTokensDesc(left, right) || left.key.localeCompare(right.key);
    case "credits":
      return byCreditsDesc(left, right) || left.key.localeCompare(right.key);
    case "calls":
      return right.calls - left.calls || left.key.localeCompare(right.key);
    case "sessions":
      return right.sessions - left.sessions || left.key.localeCompare(right.key);
    default:
      return left.key.localeCompare(right.key);
  }
}

function compareSessionRows(
  left: UsageSessionRow,
  right: UsageSessionRow,
  sortBy: StatSort | undefined
) {
  switch (sortBy) {
    case "time":
      return right.lastSeen.getTime() - left.lastSeen.getTime() || left.sessionId.localeCompare(right.sessionId);
    case "tokens":
      return byTokensDesc(left, right) || left.sessionId.localeCompare(right.sessionId);
    case "credits":
    case undefined:
      return (
        byCreditsDesc(left, right) ||
        byTokensDesc(left, right) ||
        left.sessionId.localeCompare(right.sessionId)
      );
    case "calls":
      return right.calls - left.calls || left.sessionId.localeCompare(right.sessionId);
    case "sessions":
      return left.sessionId.localeCompare(right.sessionId);
    default:
      return left.sessionId.localeCompare(right.sessionId);
  }
}

function byTokensDesc(left: { usage: TokenUsage }, right: { usage: TokenUsage }) {
  return right.usage.totalTokens - left.usage.totalTokens;
}

function byCreditsDesc(left: { credits: number }, right: { credits: number }) {
  return right.credits - left.credits;
}

function buildSessionEventBreakdown(
  rows: UsageSessionEventRow[],
  keyForRow: (row: UsageSessionEventRow) => string
): UsageStatRow[] {
  const grouped = new Map<string, UsageSessionEventRow[]>();

  for (const row of rows) {
    const key = keyForRow(row);
    const groupRows = grouped.get(key) ?? [];
    groupRows.push(row);
    grouped.set(key, groupRows);
  }

  return [...grouped.entries()]
    .map(([key, groupRows]) => {
      const usage = { ...EMPTY_USAGE };
      let credits = 0;
      let pricedCalls = 0;
      let unpricedCalls = 0;

      for (const row of groupRows) {
        addUsage(usage, row.usage);
        credits += row.credits;

        if (row.priced) {
          pricedCalls += 1;
        } else {
          unpricedCalls += 1;
        }
      }

      return {
        key,
        sessions: 1,
        calls: groupRows.length,
        usage,
        credits: roundCredits(credits),
        usd: creditsToUsd(credits),
        pricedCalls,
        unpricedCalls
      };
    })
    .sort(
      (left, right) =>
        byCreditsDesc(left, right) || byTokensDesc(left, right) || left.key.localeCompare(right.key)
    );
}

function countValueSwitches<T>(rows: T[], valueForRow: (row: T) => string) {
  let switches = 0;
  let previous: string | undefined;

  for (const row of rows) {
    const value = valueForRow(row);

    if (previous !== undefined && value !== previous) {
      switches += 1;
    }

    previous = value;
  }

  return switches;
}

function splitSessionRowsByModelAndEffort(rows: UsageSessionEventRow[]) {
  const runs: UsageSessionEventRow[][] = [];

  for (const row of rows) {
    const previousRun = runs.at(-1);
    const previous = previousRun?.at(-1);

    if (
      previous === undefined ||
      previous.model !== row.model ||
      (previous.reasoningEffort ?? "") !== (row.reasoningEffort ?? "")
    ) {
      runs.push([row]);
      continue;
    }

    previousRun?.push(row);
  }

  return runs;
}

function allocateCompactBuckets(runs: UsageSessionEventRow[][], maxRows: number) {
  const totalEvents = runs.reduce((sum, run) => sum + run.length, 0);
  const buckets = runs.map(() => 1);
  let remaining = Math.max(0, maxRows - runs.length);

  while (remaining > 0) {
    let bestIndex = -1;
    let bestDeficit = Number.NEGATIVE_INFINITY;

    for (let index = 0; index < runs.length; index += 1) {
      const run = runs[index];
      const bucket = buckets[index];

      if (run === undefined || bucket === undefined || bucket >= run.length) {
        continue;
      }

      const desired = (run.length / totalEvents) * maxRows;
      const deficit = desired - bucket;

      if (deficit > bestDeficit) {
        bestDeficit = deficit;
        bestIndex = index;
      }
    }

    if (bestIndex === -1) {
      break;
    }

    buckets[bestIndex] = (buckets[bestIndex] ?? 1) + 1;
    remaining -= 1;
  }

  return buckets;
}

function splitSessionRun(rows: UsageSessionEventRow[], bucketCount: number) {
  const buckets: UsageSessionCompactRow[] = [];
  const safeBucketCount = Math.max(1, Math.min(rows.length, Math.floor(bucketCount)));

  for (let bucketIndex = 0; bucketIndex < safeBucketCount; bucketIndex += 1) {
    const start = Math.floor((bucketIndex * rows.length) / safeBucketCount);
    const end = Math.floor(((bucketIndex + 1) * rows.length) / safeBucketCount);
    const chunk = rows.slice(start, Math.max(start + 1, end));
    buckets.push(aggregateSessionCompactRows(chunk));
  }

  return buckets;
}

function aggregateSessionCompactRows(rows: UsageSessionEventRow[]): UsageSessionCompactRow {
  const first = rows[0];
  const last = rows.at(-1);

  if (first === undefined || last === undefined) {
    throw new Error("Cannot aggregate an empty session detail row group.");
  }

  const usage = { ...EMPTY_USAGE };
  let credits = 0;
  let pricedCalls = 0;
  let unpricedCalls = 0;

  for (const row of rows) {
    addUsage(usage, row.usage);
    credits += row.credits;

    if (row.priced) {
      pricedCalls += 1;
    } else {
      unpricedCalls += 1;
    }
  }

  return {
    start: first.timestamp,
    end: last.timestamp,
    events: rows.length,
    model: first.model,
    reasoningEffort: first.reasoningEffort,
    usage,
    credits: roundCredits(credits),
    usd: creditsToUsd(credits),
    pricedCalls,
    unpricedCalls
  };
}

function addUnpricedModel(unpricedModels: Map<string, UsageUnpricedModelRow>, record: UsageRecord) {
  const pricingKey = normalizeModelName(record.model);
  const cost = calculateCreditCost(record.model, record.usage);
  const row =
    unpricedModels.get(pricingKey) ??
    {
      model: record.model,
      pricingKey,
      calls: 0,
      totalTokens: 0,
      note: cost.unpricedReason,
      pricingStub: formatPricingStub(record.model)
    };

  row.calls += 1;
  row.totalTokens += record.usage.totalTokens;
  unpricedModels.set(pricingKey, row);
}

function formatUnpricedModels(unpricedModels: Map<string, UsageUnpricedModelRow>) {
  return [...unpricedModels.values()].sort(
    (left, right) =>
      right.calls - left.calls ||
      right.totalTokens - left.totalTokens ||
      left.pricingKey.localeCompare(right.pricingKey)
  );
}

function formatPricingStub(model: string) {
  const key = normalizeModelName(model);
  return [
    `"${key}": {`,
    `  label: "${escapeDoubleQuoted(model)}",`,
    "  inputCreditsPerMillion: 0,",
    "  cachedInputCreditsPerMillion: 0,",
    "  outputCreditsPerMillion: 0",
    "}"
  ].join("\n");
}

function escapeDoubleQuoted(value: string) {
  return value.replaceAll("\\", "\\\\").replaceAll('"', '\\"');
}

export function formatUsageStats(
  report: UsageStatsReport,
  format: StatFormat = "table",
  options: { verbose?: boolean } = {}
): string {
  if (format === "json") {
    return `${JSON.stringify(toUsageStatsJson(report), null, 2)}\n`;
  }

  const rows = [
    usageHeaders(),
    ...report.rows.map((row) => usageRow(row)),
    usageRow(report.totals)
  ];

  if (format === "csv") {
    return `${formatCsv(rows)}\n`;
  }

  if (format === "markdown") {
    return `${formatMarkdownTable(rows)}\n`;
  }

  const lines = [
    pc.bold("Codex usage"),
    `Range: ${formatReportRange(report)}`,
    `Grouped by: ${formatGroupBy(report)}`,
    `Sessions dir: ${report.sessionsDir}`,
    ""
  ];

  if (report.rows.length === 0) {
    lines.push("No token usage records found in this range.");
    appendUsageNotes(lines, report, options);
    return lines.join("\n");
  }

  lines.push(formatTable(rows, report.rows.length));
  appendUsageNotes(lines, report, options);

  return lines.join("\n");
}

export function formatUsageSessions(
  report: UsageSessionsReport,
  format: StatFormat = "table",
  options: { verbose?: boolean } = {}
): string {
  if (format === "json") {
    return `${JSON.stringify(toUsageSessionsJson(report), null, 2)}\n`;
  }

  const rows = [sessionHeaders(), ...report.rows.map((row) => sessionRow(row))];

  if (format === "csv") {
    return `${formatCsv(rows)}\n`;
  }

  if (format === "markdown") {
    return `${formatMarkdownTable(rows)}\n`;
  }

  const lines = [
    pc.bold("Codex usage sessions"),
    `Range: ${formatReportRange(report)}`,
    `Sessions dir: ${report.sessionsDir}`,
    ""
  ];

  if (report.rows.length === 0) {
    lines.push("No token usage records found in this range.");
    appendUsageNotes(lines, report, options);
    return lines.join("\n");
  }

  lines.push(formatTable(rows, report.rows.length));
  appendUsageNotes(lines, report, options);

  return lines.join("\n");
}

export function formatUsageSessionDetail(
  report: UsageSessionDetailReport,
  format: StatFormat = "table",
  options: { verbose?: boolean; detail?: boolean } = {}
): string {
  if (format === "json") {
    return `${JSON.stringify(toUsageSessionDetailJson(report), null, 2)}\n`;
  }

  const compactRows = buildUsageSessionCompactRows(
    report.rows,
    DEFAULT_SESSION_DETAIL_COMPACT_ROWS
  );
  const rows =
    options.detail === true
      ? [sessionDetailHeaders(), ...report.rows.map((row) => sessionDetailRow(row))]
      : [sessionCompactHeaders(), ...compactRows.map((row) => sessionCompactRow(row))];

  if (format === "csv") {
    return `${formatCsv(rows)}\n`;
  }

  if (format === "markdown") {
    return `${formatMarkdownTable(rows)}\n`;
  }

  const lines = [
    pc.bold("Codex usage session detail"),
    `Session: ${report.sessionId}`,
    `Range: ${formatReportRange(report)}`,
    `Sessions dir: ${report.sessionsDir}`,
    ""
  ];

  if (report.summary !== undefined) {
    lines.push(
      `Model: ${report.summary.model}`,
      `CWD: ${report.summary.cwd}`,
      `First seen: ${formatDateTime(report.summary.firstSeen)}`,
      `Last seen: ${formatDateTime(report.summary.lastSeen)}`,
      `Changes: model ${formatInteger(report.modelSwitches)}, cwd ${formatInteger(
        report.cwdSwitches
      )}, reasoning effort ${formatInteger(report.reasoningEffortSwitches)}`,
      ""
    );
  }

  if (report.rows.length === 0) {
    lines.push("No token usage records found for this session in this range.");
    appendUsageNotes(lines, report, options);
    return lines.join("\n");
  }

  if (options.detail === true) {
    lines.push(formatTable([...rows, sessionDetailTotalRow(report.totals)], report.rows.length));
  } else {
    lines.push(formatTable([...rows, sessionCompactTotalRow(report.totals)], compactRows.length));

    if (report.rows.length > DEFAULT_SESSION_DETAIL_COMPACT_ROWS) {
      lines.push(
        "",
        `Compact view: ${formatInteger(compactRows.length)} row(s) from ${formatInteger(
          report.rows.length
        )} event(s). Use --detail for full event-level rows.`
      );
    }
  }

  appendSessionDetailBreakdown(lines, "By model:", report.byModel);
  appendSessionDetailBreakdown(lines, "By cwd:", report.byCwd);
  appendSessionDetailBreakdown(lines, "By reasoning effort:", report.byReasoningEffort);
  appendUsageNotes(lines, report, options);

  return lines.join("\n");
}

export function buildUsageSessionCompactRows(
  rows: UsageSessionEventRow[],
  maxRows = DEFAULT_SESSION_DETAIL_COMPACT_ROWS
): UsageSessionCompactRow[] {
  if (rows.length === 0) {
    return [];
  }

  const safeMaxRows = Math.max(1, Math.floor(maxRows));
  const runs = splitSessionRowsByModelAndEffort(rows);

  if (rows.length <= safeMaxRows) {
    return rows.map((row) => aggregateSessionCompactRows([row]));
  }

  if (runs.length >= safeMaxRows) {
    return runs.map((run) => aggregateSessionCompactRows(run));
  }

  const bucketCounts = allocateCompactBuckets(runs, safeMaxRows);
  return runs.flatMap((run, index) => splitSessionRun(run, bucketCounts[index] ?? 1));
}

function appendSessionDetailBreakdown(lines: string[], label: string, rows: UsageStatRow[]) {
  if (rows.length === 0) {
    return;
  }

  lines.push(
    "",
    label,
    formatTable([usageHeaders(), ...rows.map((row) => usageRow(row))], rows.length)
  );
}

function appendUsageNotes(
  lines: string[],
  report: Pick<
    UsageStatsReport | UsageSessionsReport | UsageSessionDetailReport,
    "totals" | "unpricedModels" | "diagnostics"
  >,
  options: { verbose?: boolean }
) {
  if (report.totals.unpricedCalls > 0) {
    lines.push(
      "",
      `Note: ${formatInteger(
        report.totals.unpricedCalls
      )} usage events had no credit price and are excluded from Credits.`
    );

    if (report.unpricedModels.length > 0) {
      lines.push("Unpriced models:");
      for (const row of report.unpricedModels) {
        lines.push(
          `  ${row.model}: ${formatInteger(row.calls)} calls, ${formatInteger(
            row.totalTokens
          )} tokens`
        );
      }
      lines.push("Pricing stubs for src/pricing.ts:");
      for (const row of report.unpricedModels) {
        lines.push(indentBlock(row.pricingStub, "  "));
      }
    }
  }

  if (options.verbose === true && report.diagnostics !== undefined) {
    const diagnostics = report.diagnostics;
    lines.push(
      "",
      "Diagnostics:",
      `  Full file scan: ${diagnostics.scanAllFiles ? "yes" : "no"}`,
      `  Directories scanned: ${formatInteger(diagnostics.scannedDirectories)}`,
      `  Directories skipped by date: ${formatInteger(diagnostics.skippedDirectories)}`,
      `  Files read: ${formatInteger(diagnostics.readFiles)}`,
      `  Files skipped by date: ${formatInteger(diagnostics.skippedFiles)}`,
      `  Files skipped by full-scan prefilter: ${formatInteger(diagnostics.prefilteredFiles)}`,
      `  File read concurrency: ${formatInteger(diagnostics.fileReadConcurrency)}`,
      `  Lines read: ${formatInteger(diagnostics.readLines)}`,
      `  Invalid JSON lines: ${formatInteger(diagnostics.invalidJsonLines)}`,
      `  Token count events: ${formatInteger(diagnostics.tokenCountEvents)}`,
      `  Usage events included: ${formatInteger(diagnostics.includedUsageEvents)}`,
      `  Skipped events: missing metadata ${formatInteger(
        diagnostics.skippedEvents.missingMetadata
      )}, missing usage ${formatInteger(diagnostics.skippedEvents.missingUsage)}, empty usage ${formatInteger(
        diagnostics.skippedEvents.emptyUsage
      )}, out of range ${formatInteger(diagnostics.skippedEvents.outOfRange)}`
    );
  } else if (shouldSuggestFullScan(report.diagnostics)) {
    const diagnostics = report.diagnostics;

    if (diagnostics === undefined) {
      return;
    }

    lines.push(
      "",
      `Note: ${formatInteger(
        diagnostics.skippedFiles
      )} session file(s) were skipped by rollout date. Use --full-scan if you expect long sessions with in-range events inside older files.`
    );
  }
}

function shouldSuggestFullScan(diagnostics: UsageDiagnostics | undefined) {
  return (
    diagnostics !== undefined &&
    diagnostics.scanAllFiles === false &&
    diagnostics.skippedFiles > 0
  );
}

export function toUsageStatsJson(report: UsageStatsReport) {
  return {
    start: report.start.toISOString(),
    end: report.end.toISOString(),
    groupBy: report.groupBy,
    includeReasoningEffort: report.includeReasoningEffort,
    sortBy: report.sortBy,
    limit: report.limit,
    sessionsDir: report.sessionsDir,
    rows: report.rows,
    totals: report.totals,
    unpricedModels: report.unpricedModels,
    diagnostics: report.diagnostics
  };
}

export function toUsageSessionsJson(report: UsageSessionsReport) {
  return {
    start: report.start.toISOString(),
    end: report.end.toISOString(),
    sortBy: report.sortBy,
    limit: report.limit,
    sessionsDir: report.sessionsDir,
    rows: report.rows.map((row) => ({
      ...row,
      firstSeen: row.firstSeen.toISOString(),
      lastSeen: row.lastSeen.toISOString()
    })),
    totals: report.totals,
    unpricedModels: report.unpricedModels,
    diagnostics: report.diagnostics
  };
}

export function toUsageSessionDetailJson(report: UsageSessionDetailReport) {
  return {
    start: report.start.toISOString(),
    end: report.end.toISOString(),
    sessionId: report.sessionId,
    limit: report.limit,
    sessionsDir: report.sessionsDir,
    summary:
      report.summary === undefined
        ? undefined
        : {
            ...report.summary,
            firstSeen: report.summary.firstSeen.toISOString(),
            lastSeen: report.summary.lastSeen.toISOString()
          },
    rows: report.rows.map((row) => ({
      ...row,
      timestamp: row.timestamp.toISOString()
    })),
    byModel: report.byModel,
    byCwd: report.byCwd,
    byReasoningEffort: report.byReasoningEffort,
    modelSwitches: report.modelSwitches,
    cwdSwitches: report.cwdSwitches,
    reasoningEffortSwitches: report.reasoningEffortSwitches,
    totals: report.totals,
    unpricedModels: report.unpricedModels,
    diagnostics: report.diagnostics
  };
}

async function processCodexUsageRecords(
  options: Pick<StatRangeOptions, "sessionsDir" | "start" | "end"> & UsageFileScanOptions,
  onRecord: (record: UsageRecord) => void
) {
  const diagnostics = createUsageDiagnostics(DEFAULT_FILE_READ_CONCURRENCY, options.scanAllFiles === true);
  let files = await listJsonlFiles(options.sessionsDir, options, [], diagnostics);

  if (options.scanAllFiles === true) {
    files = await prefilterFullScanFiles(files, options.start, diagnostics);
  }

  diagnostics.readFiles = files.length;

  for (let index = 0; index < files.length; index += DEFAULT_FILE_READ_CONCURRENCY) {
    const batch = files.slice(index, index + DEFAULT_FILE_READ_CONCURRENCY);
    const results = await Promise.all(
      batch.map((filePath) => readUsageRecordsFromFile(filePath, options.start, options.end))
    );

    for (const result of results) {
      mergeUsageDiagnostics(diagnostics, result.diagnostics);
      for (const record of result.records) {
        onRecord(record);
      }
    }
  }

  return diagnostics;
}

async function readUsageRecordsFromFile(filePath: string, start: Date, end: Date) {
  const diagnostics = createUsageDiagnostics(0);
  const records: UsageRecord[] = [];
  const stream = createReadStream(filePath, { encoding: "utf8" });
  const lines = createInterface({ input: stream, crlfDelay: Infinity });
  let sessionId = sessionIdFromPath(filePath);
  let model = "unknown";
  let reasoningEffort: string | undefined;
  let cwd = "unknown";
  let previousTotal: TokenUsage | undefined;

  for await (const line of lines) {
    diagnostics.readLines += 1;

    if (
      !line.includes('"token_count"') &&
      !line.includes('"session_meta"') &&
      !line.includes('"turn_context"')
    ) {
      continue;
    }

    const event = parseJsonObject(line);

    if (event === undefined) {
      diagnostics.invalidJsonLines += 1;
      continue;
    }

    if (event.type === "session_meta") {
      const payload = asRecord(event.payload);
      sessionId = readString(payload?.id) ?? sessionId;
      model = readString(payload?.model) ?? model;
      reasoningEffort = readReasoningEffort(payload) ?? reasoningEffort;
      cwd = readString(payload?.cwd) ?? cwd;
      continue;
    }

    if (event.type === "turn_context") {
      const payload = asRecord(event.payload);
      model = readString(payload?.model) ?? model;
      reasoningEffort = readReasoningEffort(payload) ?? reasoningEffort;
      cwd = readString(payload?.cwd) ?? cwd;
      continue;
    }

    const payload = asRecord(event.payload);

    if (event.type !== "event_msg" || payload?.type !== "token_count") {
      continue;
    }

    diagnostics.tokenCountEvents += 1;
    const timestamp = readDate(event.timestamp);
    const info = asRecord(payload.info);

    if (timestamp === undefined || info === undefined) {
      diagnostics.skippedEvents.missingMetadata += 1;
      continue;
    }

    const totalUsage = readTokenUsage(info.total_token_usage);
    const usage = readTokenUsage(info.last_token_usage) ?? diffUsage(totalUsage, previousTotal);

    if (totalUsage !== undefined) {
      previousTotal = totalUsage;
    }

    if (usage === undefined) {
      diagnostics.skippedEvents.missingUsage += 1;
      continue;
    }

    if (isEmptyUsage(usage)) {
      diagnostics.skippedEvents.emptyUsage += 1;
      continue;
    }

    if (timestamp < start || timestamp > end) {
      diagnostics.skippedEvents.outOfRange += 1;
      continue;
    }

    diagnostics.includedUsageEvents += 1;
    records.push({
      timestamp,
      sessionId,
      model,
      reasoningEffort,
      cwd,
      filePath,
      usage
    });
  }

  return { records, diagnostics };
}

async function listJsonlFiles(
  root: string,
  range: Pick<StatRangeOptions, "start" | "end"> & UsageFileScanOptions,
  dateParts: string[] | undefined = [],
  diagnostics = createUsageDiagnostics()
): Promise<string[]> {
  let entries;
  diagnostics.scannedDirectories += 1;

  try {
    entries = await readdir(root, { withFileTypes: true });
  } catch (error) {
    if (isNotFoundError(error)) {
      return [];
    }

    throw error;
  }

  const files: string[] = [];
  const scanAllFiles = range.scanAllFiles === true;
  const scanWindow = scanAllFiles ? undefined : createDirectoryScanWindow(range);

  for (const entry of entries) {
    const path = join(root, entry.name);

    if (entry.isDirectory()) {
      const nextDateParts = appendDatePathPart(dateParts, entry.name);

      if (
        scanWindow !== undefined &&
        nextDateParts !== undefined &&
        isDatePathOutsideWindow(nextDateParts, scanWindow)
      ) {
        diagnostics.skippedDirectories += 1;
        continue;
      }

      files.push(...(await listJsonlFiles(path, range, nextDateParts, diagnostics)));
    } else if (entry.isFile() && entry.name.endsWith(".jsonl")) {
      if (scanWindow !== undefined && isRolloutFileOutsideWindow(entry.name, scanWindow)) {
        diagnostics.skippedFiles += 1;
        continue;
      }

      files.push(path);
    }
  }

  return files.sort();
}

async function prefilterFullScanFiles(
  files: string[],
  start: Date,
  diagnostics: UsageDiagnostics
) {
  const keptFiles: string[] = [];

  for (let index = 0; index < files.length; index += DEFAULT_FILE_READ_CONCURRENCY) {
    const batch = files.slice(index, index + DEFAULT_FILE_READ_CONCURRENCY);
    const results = await Promise.all(
      batch.map(async (filePath) => {
        const lastUsageAt = await readLastTokenCountTimestamp(filePath);

        if (lastUsageAt !== undefined && lastUsageAt < start) {
          diagnostics.prefilteredFiles += 1;
          return undefined;
        }

        return filePath;
      })
    );

    for (const filePath of results) {
      if (filePath !== undefined) {
        keptFiles.push(filePath);
      }
    }
  }

  return keptFiles;
}

async function readLastTokenCountTimestamp(filePath: string) {
  const handle = await open(filePath, "r");

  try {
    const { size } = await handle.stat();
    let position = size;
    let carry = "";

    while (position > 0) {
      const readSize = Math.min(FULL_SCAN_PREFILTER_CHUNK_SIZE, position);
      position -= readSize;

      const buffer = Buffer.allocUnsafe(readSize);
      await handle.read(buffer, 0, readSize, position);

      const text = `${buffer.toString("utf8")}${carry}`;
      const includesFileStart = position === 0;
      const lines = text.split(/\r?\n/);
      const firstCompleteLineIndex = includesFileStart ? 0 : 1;

      for (let lineIndex = lines.length - 1; lineIndex >= firstCompleteLineIndex; lineIndex -= 1) {
        const timestamp = readTokenCountTimestampFromLine(lines[lineIndex] ?? "");

        if (timestamp !== undefined) {
          return timestamp;
        }
      }

      carry = includesFileStart ? "" : lines[0] ?? "";
    }

    return readTokenCountTimestampFromLine(carry);
  } finally {
    await handle.close();
  }
}

function readTokenCountTimestampFromLine(line: string) {
  if (!line.includes('"token_count"')) {
    return undefined;
  }

  const event = parseJsonObject(line);
  const payload = asRecord(event?.payload);

  if (event?.type !== "event_msg" || payload?.type !== "token_count") {
    return undefined;
  }

  return readDate(event.timestamp);
}

function createUsageDiagnostics(
  fileReadConcurrency = DEFAULT_FILE_READ_CONCURRENCY,
  scanAllFiles = false
): UsageDiagnostics {
  return {
    scanAllFiles,
    scannedDirectories: 0,
    skippedDirectories: 0,
    readFiles: 0,
    skippedFiles: 0,
    prefilteredFiles: 0,
    readLines: 0,
    invalidJsonLines: 0,
    tokenCountEvents: 0,
    includedUsageEvents: 0,
    skippedEvents: {
      missingMetadata: 0,
      missingUsage: 0,
      emptyUsage: 0,
      outOfRange: 0
    },
    fileReadConcurrency
  };
}

function mergeUsageDiagnostics(target: UsageDiagnostics, source: UsageDiagnostics) {
  target.prefilteredFiles += source.prefilteredFiles;
  target.readLines += source.readLines;
  target.invalidJsonLines += source.invalidJsonLines;
  target.tokenCountEvents += source.tokenCountEvents;
  target.includedUsageEvents += source.includedUsageEvents;
  target.skippedEvents.missingMetadata += source.skippedEvents.missingMetadata;
  target.skippedEvents.missingUsage += source.skippedEvents.missingUsage;
  target.skippedEvents.emptyUsage += source.skippedEvents.emptyUsage;
  target.skippedEvents.outOfRange += source.skippedEvents.outOfRange;
}

function appendDatePathPart(parts: string[] | undefined, name: string) {
  if (parts === undefined || parts.length >= 3) {
    return parts;
  }

  if (parts.length === 0 && /^\d{4}$/.test(name)) {
    return [name];
  }

  if ((parts.length === 1 || parts.length === 2) && /^\d{2}$/.test(name)) {
    return [...parts, name];
  }

  return undefined;
}

function createDirectoryScanWindow(range: Pick<StatRangeOptions, "start" | "end">) {
  // Session files are anchored by rollout start; keep a cushion for cross-day events.
  return {
    start: addDays(startOfLocalDay(range.start), -1),
    end: endOfLocalDay(addDays(range.end, 1))
  };
}

function isDatePathOutsideWindow(
  parts: string[],
  window: { start: Date; end: Date }
) {
  const range = datePathRange(parts);
  return range !== undefined && (range.end < window.start || range.start > window.end);
}

function isRolloutFileOutsideWindow(name: string, window: { start: Date; end: Date }) {
  const timestamp = rolloutTimestampFromFileName(name);
  return timestamp !== undefined && (timestamp < window.start || timestamp > window.end);
}

function rolloutTimestampFromFileName(name: string) {
  const match = /^rollout-(\d{4})-(\d{2})-(\d{2})T(\d{2})-(\d{2})-(\d{2})-.+\.jsonl$/.exec(
    name
  );

  if (match === null) {
    return undefined;
  }

  const [, year, month, day, hour, minute, second] = match;
  const timestamp = new Date(
    Number(year),
    Number(month) - 1,
    Number(day),
    Number(hour),
    Number(minute),
    Number(second)
  );

  if (
    timestamp.getFullYear() !== Number(year) ||
    timestamp.getMonth() !== Number(month) - 1 ||
    timestamp.getDate() !== Number(day) ||
    timestamp.getHours() !== Number(hour) ||
    timestamp.getMinutes() !== Number(minute) ||
    timestamp.getSeconds() !== Number(second)
  ) {
    return undefined;
  }

  return timestamp;
}

function datePathRange(parts: string[]) {
  const year = Number(parts[0]);

  if (!Number.isSafeInteger(year)) {
    return undefined;
  }

  if (parts.length === 1) {
    return {
      start: new Date(year, 0, 1),
      end: new Date(year + 1, 0, 1, 0, 0, 0, -1)
    };
  }

  const month = Number(parts[1]);

  if (!Number.isSafeInteger(month) || month < 1 || month > 12) {
    return undefined;
  }

  if (parts.length === 2) {
    return {
      start: new Date(year, month - 1, 1),
      end: new Date(year, month, 1, 0, 0, 0, -1)
    };
  }

  const day = Number(parts[2]);
  const start = new Date(year, month - 1, day);

  if (
    !Number.isSafeInteger(day) ||
    start.getFullYear() !== year ||
    start.getMonth() !== month - 1 ||
    start.getDate() !== day
  ) {
    return undefined;
  }

  return {
    start,
    end: endOfLocalDay(start)
  };
}

function parseGroupBy(value: string | undefined): StatGroupBy {
  if (value === undefined) {
    return "day";
  }

  if (
    value === "hour" ||
    value === "day" ||
    value === "week" ||
    value === "month" ||
    value === "model" ||
    value === "cwd"
  ) {
    return value;
  }

  throw new Error("Invalid group-by value. Expected one of: hour, day, week, month, model, cwd.");
}

function parseSort(value: string | undefined): StatSort | undefined {
  if (value === undefined) {
    return undefined;
  }

  if (
    value === "time" ||
    value === "tokens" ||
    value === "credits" ||
    value === "calls" ||
    value === "sessions"
  ) {
    return value;
  }

  throw new Error("Invalid sort value. Expected one of: time, tokens, credits, calls, sessions.");
}

function parseOptionalLimit(value: string | number | undefined, name: string) {
  if (value === undefined) {
    return undefined;
  }

  const limit = typeof value === "number" ? value : Number(value);

  if (!Number.isSafeInteger(limit) || limit <= 0) {
    throw new Error(`Invalid ${name} value. Expected a positive integer.`);
  }

  return limit;
}

function resolveGroupBy(
  value: string | undefined,
  raw: RawStatOptions,
  range: { start: Date; end: Date }
): StatGroupBy {
  if (value !== undefined) {
    return parseGroupBy(value);
  }

  if (raw.all === true) {
    return "month";
  }

  if (raw.month === true) {
    return "day";
  }

  const durationMs = range.end.getTime() - range.start.getTime();

  if (durationMs <= 48 * 60 * 60 * 1000) {
    return "hour";
  }

  if (durationMs <= 31 * 24 * 60 * 60 * 1000) {
    return "day";
  }

  if (range.end <= addMonths(range.start, 6)) {
    return "week";
  }

  return "month";
}

function parseFormat(value: string | undefined): StatFormat {
  if (value === undefined) {
    return "table";
  }

  if (value === "table" || value === "json" || value === "csv" || value === "markdown") {
    return value;
  }

  throw new Error("Invalid format value. Expected one of: table, json, csv, markdown.");
}

function parseDateBound(value: string, bound: "start" | "end") {
  const dateOnly = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);

  if (dateOnly !== null) {
    const [, year, month, day] = dateOnly;
    return new Date(
      Number(year),
      Number(month) - 1,
      Number(day),
      bound === "start" ? 0 : 23,
      bound === "start" ? 0 : 59,
      bound === "start" ? 0 : 59,
      bound === "start" ? 0 : 999
    );
  }

  const date = new Date(value);

  if (Number.isNaN(date.getTime())) {
    throw new Error(`Invalid ${bound} time: ${value}`);
  }

  return date;
}

function resolveDateRange(raw: RawStatOptions, now: Date) {
  const quickRanges = [raw.all, raw.today, raw.yesterday, raw.month, raw.last !== undefined].filter(
    Boolean
  );

  if (quickRanges.length > 1) {
    throw new Error("Use only one quick range option: --all, --today, --yesterday, --month, or --last.");
  }

  if (quickRanges.length === 1 && (raw.start !== undefined || raw.end !== undefined)) {
    throw new Error("Quick range options cannot be combined with --start or --end.");
  }

  if (raw.all === true) {
    return {
      start: new Date(ALL_USAGE_RANGE_START),
      end: new Date(ALL_USAGE_RANGE_END)
    };
  }

  if (raw.today === true) {
    return {
      start: startOfLocalDay(now),
      end: now
    };
  }

  if (raw.yesterday === true) {
    const yesterday = addDays(startOfLocalDay(now), -1);

    return {
      start: yesterday,
      end: endOfLocalDay(yesterday)
    };
  }

  if (raw.month === true) {
    return {
      start: new Date(now.getFullYear(), now.getMonth(), 1),
      end: now
    };
  }

  if (raw.last !== undefined) {
    return {
      start: new Date(now.getTime() - parseDuration(raw.last)),
      end: now
    };
  }

  const end = raw.end === undefined ? now : parseDateBound(raw.end, "end");
  const start =
    raw.start === undefined
      ? new Date(end.getTime() - 7 * 24 * 60 * 60 * 1000)
      : parseDateBound(raw.start, "start");

  return { start, end };
}

function parseDuration(value: string) {
  const match = /^(\d+)(h|d|w|mo)$/.exec(value.trim());

  if (match === null) {
    throw new Error("Invalid --last value. Use a duration like 12h, 7d, 2w, or 1mo.");
  }

  const amount = Number(match[1]);
  const unit = match[2];

  if (!Number.isSafeInteger(amount) || amount <= 0) {
    throw new Error("Invalid --last value. Duration must be a positive integer.");
  }

  switch (unit) {
    case "h":
      return amount * 60 * 60 * 1000;
    case "d":
      return amount * 24 * 60 * 60 * 1000;
    case "w":
      return amount * 7 * 24 * 60 * 60 * 1000;
    case "mo":
      return amount * 30 * 24 * 60 * 60 * 1000;
    default:
      throw new Error("Invalid --last value. Use a duration like 12h, 7d, 2w, or 1mo.");
  }
}

function startOfLocalDay(date: Date) {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate());
}

function endOfLocalDay(date: Date) {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate(), 23, 59, 59, 999);
}

function addDays(date: Date, days: number) {
  const next = new Date(date);
  next.setDate(next.getDate() + days);
  return next;
}

function addMonths(date: Date, months: number) {
  const next = new Date(
    date.getFullYear(),
    date.getMonth() + months,
    1,
    date.getHours(),
    date.getMinutes(),
    date.getSeconds(),
    date.getMilliseconds()
  );
  next.setDate(Math.min(date.getDate(), daysInMonth(next.getFullYear(), next.getMonth())));
  return next;
}

function daysInMonth(year: number, month: number) {
  return new Date(year, month + 1, 0).getDate();
}

function defaultCodexHome() {
  return process.env.CODEX_HOME ?? join(homedir(), ".codex");
}

function getGroupKey(
  record: UsageRecord,
  groupBy: StatGroupBy,
  includeReasoningEffort: boolean
) {
  if (groupBy === "model") {
    return includeReasoningEffort ? modelGroupKey(record) : record.model;
  }

  if (groupBy === "cwd") {
    return record.cwd;
  }

  if (groupBy === "week") {
    return isoWeekKey(record.timestamp);
  }

  if (groupBy === "month") {
    return localMonthKey(record.timestamp);
  }

  if (groupBy === "hour") {
    return localHourKey(record.timestamp);
  }

  return localDateKey(record.timestamp);
}

function modelGroupKey(record: Pick<UsageRecord, "model" | "reasoningEffort">) {
  const effort = normalizeReasoningEffort(record.reasoningEffort);

  if (record.model === "unknown" || effort === undefined) {
    return record.model;
  }

  return `${record.model}-${effort}`;
}

function normalizeReasoningEffort(value: string | undefined) {
  const normalized = value
    ?.trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return normalized === "" ? undefined : normalized;
}

function localHourKey(date: Date) {
  return `${localDateKey(date)} ${pad2(date.getHours())}:00`;
}

function localDateKey(date: Date) {
  return [
    date.getFullYear(),
    pad2(date.getMonth() + 1),
    pad2(date.getDate())
  ].join("-");
}

function localMonthKey(date: Date) {
  return [date.getFullYear(), pad2(date.getMonth() + 1)].join("-");
}

function isoWeekKey(date: Date) {
  const local = new Date(date.getFullYear(), date.getMonth(), date.getDate());
  const day = local.getDay() || 7;
  local.setDate(local.getDate() + 4 - day);
  const yearStart = new Date(local.getFullYear(), 0, 1);
  const week = Math.ceil(((local.getTime() - yearStart.getTime()) / 86_400_000 + 1) / 7);

  return `${local.getFullYear()}-W${pad2(week)}`;
}

function formatDateTime(date: Date) {
  return `${localDateKey(date)} ${pad2(date.getHours())}:${pad2(date.getMinutes())}:${pad2(
    date.getSeconds()
  )}`;
}

function formatReportRange(
  report: Pick<UsageStatsReport | UsageSessionsReport | UsageSessionDetailReport, "start" | "end">
) {
  if (isAllUsageRange(report)) {
    return "all";
  }

  return `${formatDateTime(report.start)} to ${formatDateTime(report.end)}`;
}

function formatGroupBy(report: Pick<UsageStatsReport, "groupBy" | "includeReasoningEffort">) {
  if (report.groupBy === "model" && report.includeReasoningEffort) {
    return "model + reasoning_effort";
  }

  return report.groupBy;
}

function isAllUsageRange(range: Pick<StatRangeOptions, "start" | "end">) {
  return (
    range.start.getTime() === ALL_USAGE_RANGE_START.getTime() &&
    range.end.getTime() === ALL_USAGE_RANGE_END.getTime()
  );
}

function usageHeaders() {
  return [
    "Group",
    "Sessions",
    "Calls",
    "Input",
    "Cached",
    "Output",
    "Reasoning",
    "Total",
    "Credits",
    "USD"
  ];
}

function usageRow(row: UsageStatRow) {
  return [
    row.key,
    formatInteger(row.sessions),
    formatInteger(row.calls),
    formatInteger(row.usage.inputTokens),
    formatInteger(row.usage.cachedInputTokens),
    formatInteger(row.usage.outputTokens),
    formatInteger(row.usage.reasoningOutputTokens),
    formatInteger(row.usage.totalTokens),
    formatCredits(row.credits),
    formatUsd(row.usd)
  ];
}

function sessionHeaders() {
  return [
    "Session",
    "Model",
    "CWD",
    "First seen",
    "Last seen",
    "Calls",
    "Input",
    "Cached",
    "Output",
    "Total",
    "Credits",
    "USD"
  ];
}

function sessionRow(row: UsageSessionRow) {
  return [
    row.sessionId,
    row.model,
    row.cwd,
    formatDateTime(row.firstSeen),
    formatDateTime(row.lastSeen),
    formatInteger(row.calls),
    formatInteger(row.usage.inputTokens),
    formatInteger(row.usage.cachedInputTokens),
    formatInteger(row.usage.outputTokens),
    formatInteger(row.usage.totalTokens),
    formatCredits(row.credits),
    formatUsd(row.usd)
  ];
}

function sessionDetailHeaders() {
  return [
    "Time",
    "Model",
    "Effort",
    "CWD",
    "Input",
    "Cached",
    "Output",
    "Reasoning",
    "Total",
    "Credits",
    "USD"
  ];
}

function sessionCompactHeaders() {
  return [
    "Range",
    "Events",
    "Model",
    "Effort",
    "Input",
    "Cached",
    "Output",
    "Reasoning",
    "Total",
    "Credits",
    "USD"
  ];
}

function sessionDetailRow(row: UsageSessionEventRow) {
  return [
    formatDateTime(row.timestamp),
    row.model,
    row.reasoningEffort ?? "",
    row.cwd,
    formatInteger(row.usage.inputTokens),
    formatInteger(row.usage.cachedInputTokens),
    formatInteger(row.usage.outputTokens),
    formatInteger(row.usage.reasoningOutputTokens),
    formatInteger(row.usage.totalTokens),
    row.priced ? formatCredits(row.credits) : "unpriced",
    row.priced ? formatUsd(row.usd) : "unpriced"
  ];
}

function sessionCompactRow(row: UsageSessionCompactRow) {
  return [
    formatCompactRange(row),
    formatInteger(row.events),
    row.model,
    row.reasoningEffort ?? "",
    formatInteger(row.usage.inputTokens),
    formatInteger(row.usage.cachedInputTokens),
    formatInteger(row.usage.outputTokens),
    formatInteger(row.usage.reasoningOutputTokens),
    formatInteger(row.usage.totalTokens),
    row.unpricedCalls === 0 ? formatCredits(row.credits) : "partial",
    row.unpricedCalls === 0 ? formatUsd(row.usd) : "partial"
  ];
}

function sessionDetailTotalRow(row: UsageStatRow) {
  return [
    "Total",
    "",
    "",
    "",
    formatInteger(row.usage.inputTokens),
    formatInteger(row.usage.cachedInputTokens),
    formatInteger(row.usage.outputTokens),
    formatInteger(row.usage.reasoningOutputTokens),
    formatInteger(row.usage.totalTokens),
    formatCredits(row.credits),
    formatUsd(row.usd)
  ];
}

function sessionCompactTotalRow(row: UsageStatRow) {
  return [
    "Total",
    formatInteger(row.calls),
    "",
    "",
    formatInteger(row.usage.inputTokens),
    formatInteger(row.usage.cachedInputTokens),
    formatInteger(row.usage.outputTokens),
    formatInteger(row.usage.reasoningOutputTokens),
    formatInteger(row.usage.totalTokens),
    formatCredits(row.credits),
    formatUsd(row.usd)
  ];
}

function formatCompactRange(row: Pick<UsageSessionCompactRow, "start" | "end">) {
  const start = formatDateTime(row.start);
  const end = formatDateTime(row.end);

  return start === end ? start : `${start} -> ${end}`;
}

function formatTable(rows: string[][], dataRowCount: number) {
  const widths = rows[0]?.map((_, columnIndex) =>
    Math.max(...rows.map((row) => row[columnIndex]?.length ?? 0))
  );

  if (widths === undefined) {
    return "";
  }

  return rows
    .map((row, rowIndex) => {
      const text = row
        .map((cell, columnIndex) => {
          const width = widths[columnIndex] ?? cell.length;
          return columnIndex === 0 ? cell.padEnd(width) : cell.padStart(width);
        })
        .join("  ");

      if (rowIndex === 0) {
        return pc.bold(text);
      }

      if (rowIndex === dataRowCount + 1) {
        return pc.bold(text);
      }

      return text;
    })
    .join("\n");
}

function formatCsv(rows: string[][]) {
  return rows.map((row) => row.map(escapeCsvCell).join(",")).join("\n");
}

function escapeCsvCell(value: string) {
  if (!/[",\n\r]/.test(value)) {
    return value;
  }

  return `"${value.replaceAll('"', '""')}"`;
}

function formatMarkdownTable(rows: string[][]) {
  const [header, ...body] = rows;

  if (header === undefined) {
    return "";
  }

  return [
    `| ${header.map(escapeMarkdownCell).join(" | ")} |`,
    `| ${header.map(() => "---").join(" | ")} |`,
    ...body.map((row) => `| ${row.map(escapeMarkdownCell).join(" | ")} |`)
  ].join("\n");
}

function escapeMarkdownCell(value: string) {
  return value.replaceAll("|", "\\|");
}

function indentBlock(value: string, prefix: string) {
  return value
    .split("\n")
    .map((line) => `${prefix}${line}`)
    .join("\n");
}

function readTokenUsage(value: unknown): TokenUsage | undefined {
  const record = asRecord(value);

  if (record === undefined) {
    return undefined;
  }

  const inputTokens = readNumber(record.input_tokens ?? record.inputTokens);
  const outputTokens = readNumber(record.output_tokens ?? record.outputTokens);
  const totalTokens = readNumber(record.total_tokens ?? record.totalTokens);

  if (inputTokens === undefined && outputTokens === undefined && totalTokens === undefined) {
    return undefined;
  }

  return {
    inputTokens: inputTokens ?? 0,
    cachedInputTokens: readNumber(record.cached_input_tokens ?? record.cachedInputTokens) ?? 0,
    outputTokens: outputTokens ?? 0,
    reasoningOutputTokens:
      readNumber(record.reasoning_output_tokens ?? record.reasoningOutputTokens) ?? 0,
    totalTokens: totalTokens ?? (inputTokens ?? 0) + (outputTokens ?? 0)
  };
}

function diffUsage(current: TokenUsage | undefined, previous: TokenUsage | undefined) {
  if (current === undefined) {
    return undefined;
  }

  if (previous === undefined) {
    return current;
  }

  return {
    inputTokens: Math.max(0, current.inputTokens - previous.inputTokens),
    cachedInputTokens: Math.max(0, current.cachedInputTokens - previous.cachedInputTokens),
    outputTokens: Math.max(0, current.outputTokens - previous.outputTokens),
    reasoningOutputTokens: Math.max(
      0,
      current.reasoningOutputTokens - previous.reasoningOutputTokens
    ),
    totalTokens: Math.max(0, current.totalTokens - previous.totalTokens)
  };
}

function addUsage(target: TokenUsage, usage: TokenUsage) {
  target.inputTokens += usage.inputTokens;
  target.cachedInputTokens += usage.cachedInputTokens;
  target.outputTokens += usage.outputTokens;
  target.reasoningOutputTokens += usage.reasoningOutputTokens;
  target.totalTokens += usage.totalTokens;
}

function isEmptyUsage(usage: TokenUsage) {
  return (
    usage.inputTokens === 0 &&
    usage.cachedInputTokens === 0 &&
    usage.outputTokens === 0 &&
    usage.reasoningOutputTokens === 0 &&
    usage.totalTokens === 0
  );
}

function sessionIdFromPath(filePath: string) {
  const match = /rollout-\d{4}-\d{2}-\d{2}T\d{2}-\d{2}-\d{2}-(.+)\.jsonl$/.exec(
    filePath
  );
  return match?.[1] ?? filePath;
}

function parseJsonObject(line: string) {
  try {
    return asRecord(JSON.parse(line));
  } catch {
    return undefined;
  }
}

function asRecord(value: unknown): Record<string, unknown> | undefined {
  return typeof value === "object" && value !== null ? (value as Record<string, unknown>) : undefined;
}

function readReasoningEffort(payload: Record<string, unknown> | undefined) {
  if (payload === undefined) {
    return undefined;
  }

  const modelConfig = asRecord(payload.model_config ?? payload.modelConfig);
  const reasoning = asRecord(payload.reasoning);
  const collaborationMode = asRecord(payload.collaboration_mode ?? payload.collaborationMode);
  const collaborationSettings = asRecord(collaborationMode?.settings);

  return (
    readString(
      payload.reasoning_effort ??
        payload.reasoningEffort ??
        payload.model_reasoning_effort ??
        payload.modelReasoningEffort
    ) ??
    readString(modelConfig?.reasoning_effort ?? modelConfig?.reasoningEffort) ??
    readString(reasoning?.effort ?? reasoning?.reasoning_effort ?? reasoning?.reasoningEffort) ??
    readString(
      collaborationSettings?.reasoning_effort ?? collaborationSettings?.reasoningEffort
    )
  );
}

function readDate(value: unknown) {
  if (typeof value !== "string" && typeof value !== "number") {
    return undefined;
  }

  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? undefined : date;
}

function readNumber(value: unknown) {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function readString(value: unknown) {
  return typeof value === "string" && value.trim() !== "" ? value : undefined;
}

function formatInteger(value: number) {
  return new Intl.NumberFormat("en-US").format(value);
}

function formatCredits(value: number) {
  return new Intl.NumberFormat("en-US", {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2
  }).format(value);
}

function roundCredits(value: number) {
  return Math.round((value + Number.EPSILON) * 1_000_000) / 1_000_000;
}

function creditsToUsd(credits: number) {
  return Math.round(((credits / 25) + Number.EPSILON) * 1_000_000) / 1_000_000;
}

function formatUsd(value: number) {
  return `$${new Intl.NumberFormat("en-US", {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2
  }).format(value)}`;
}

function pad2(value: number) {
  return String(value).padStart(2, "0");
}

function isNotFoundError(error: unknown) {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    (error as { code?: unknown }).code === "ENOENT"
  );
}
