import { readFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { calculateCreditCost, normalizeModelName } from "./pricing.js";
import {
  resolveCodexHelperDir,
  writeSensitiveFile,
  type CodexHomeOptions
} from "./storage.js";
import type { StatFormat, TokenUsage, UsageDiagnostics, UsageRecord } from "./stats.js";

export const WEEKLY_CYCLE_STORE_VERSION = 1;
export const WEEKLY_CYCLE_PERIOD_HOURS = 168;
export const DEFAULT_WEEKLY_CYCLE_ACCOUNT_ID = "default";
const WEEKLY_CYCLE_PERIOD_MS = WEEKLY_CYCLE_PERIOD_HOURS * 60 * 60 * 1000;

export type WeeklyCycleAnchorSource = "manual";
export type WeeklyCycleWindowSource = "manual" | "derived" | "estimated";
export type WeeklyCycleHistoryStatus = "ok" | "unanchored";
export type WeeklyCycleCurrentStatus = "active" | "waiting_for_usage" | "unanchored";
export type WeeklyCycleDetailStatus = "ok";

export type WeeklyCycleAnchor = {
  id: string;
  at: string;
  input: string;
  timeZone: string;
  source: WeeklyCycleAnchorSource;
  note: string;
  createdAt: string;
};

export type WeeklyCycleAccountEntry = {
  weekly: {
    periodHours: typeof WEEKLY_CYCLE_PERIOD_HOURS;
    anchors: WeeklyCycleAnchor[];
  };
};

export type WeeklyCycleStore = {
  version: typeof WEEKLY_CYCLE_STORE_VERSION;
  accounts: Record<string, WeeklyCycleAccountEntry>;
};

export type WeeklyCycleStoreFileOptions = CodexHomeOptions & {
  cycleFile?: string;
};

export type WeeklyCycleAccountSource =
  | "explicit"
  | "chatgpt_account_id"
  | "token_account_id"
  | "default";

export type WeeklyCycleAccountAuthStatus = {
  summary?: {
    chatgptAccountId?: string;
    tokenAccountId?: string;
  };
};

export type WeeklyCycleAccountOptions = {
  accountId?: string;
  authStatus?: WeeklyCycleAccountAuthStatus;
};

export type WeeklyCycleAccountResolution = {
  accountId: string;
  source: WeeklyCycleAccountSource;
  isDefault: boolean;
};

export type ParsedWeeklyCycleAnchorTime = {
  at: Date;
  atIso: string;
  input: string;
  timeZone: string;
  hasExplicitOffset: boolean;
};

export type AddWeeklyCycleAnchorOptions = {
  accountId: string;
  at: string;
  note?: string;
};

export type AddWeeklyCycleAnchorResult = {
  store: WeeklyCycleStore;
  anchor: WeeklyCycleAnchor;
};

export type RemoveWeeklyCycleAnchorResult = {
  store: WeeklyCycleStore;
  removed: WeeklyCycleAnchor;
};

export type WeeklyCycleAnchorFileOptions = WeeklyCycleStoreFileOptions & WeeklyCycleAccountOptions;

export type AddWeeklyCycleAnchorToFileOptions = WeeklyCycleAnchorFileOptions & {
  at: string;
  note?: string;
};

export type WeeklyCycleAnchorMutationReport = {
  cycleFile: string;
  accountId: string;
  accountSource: WeeklyCycleAccountSource;
  anchor: WeeklyCycleAnchor;
  store: WeeklyCycleStore;
};

export type AddWeeklyCycleAnchorsToFileOptions = WeeklyCycleAnchorFileOptions & {
  at: string[];
  note?: string;
};

export type WeeklyCycleAnchorBatchMutationReport = {
  cycleFile: string;
  accountId: string;
  accountSource: WeeklyCycleAccountSource;
  anchors: WeeklyCycleAnchor[];
  store: WeeklyCycleStore;
};

export type WeeklyCycleAnchorListReport = {
  cycleFile: string;
  accountId: string;
  accountSource: WeeklyCycleAccountSource;
  anchors: WeeklyCycleAnchor[];
  store: WeeklyCycleStore;
};

export type WeeklyCycleUnpricedModelRow = {
  model: string;
  pricingKey: string;
  calls: number;
  totalTokens: number;
  pricingStub: string;
};

export type WeeklyCycleUsageTotals = {
  sessions: number;
  calls: number;
  usage: TokenUsage;
  credits: number;
  usd: number;
  pricedCalls: number;
  unpricedCalls: number;
  unpricedModels: WeeklyCycleUnpricedModelRow[];
};

export type WeeklyCycleWindow = {
  start: Date;
  resetAt: Date;
  source: WeeklyCycleWindowSource;
  anchorId?: string;
  calibrationAnchorId?: string;
};

export type WeeklyCycleReportRow = WeeklyCycleWindow &
  WeeklyCycleUsageTotals & {
    id: string;
    index: number;
    exclusiveEnd: Date;
  };

export type WeeklyCycleBreakdownRow = WeeklyCycleUsageTotals & {
  key: string;
};

export type WeeklyCycleDiagnostics = {
  anchors: number;
  usageRecords: number;
  windows: number;
  derivedWindows: number;
  estimatedWindows: number;
  includedUsageEvents: number;
  ignoredBeforeAnchorEvents: number;
  estimateBeforeAnchor: boolean;
  unanchored: boolean;
  usageDiagnostics?: UsageDiagnostics;
};

export type BuildWeeklyCycleHistoryOptions = {
  anchors: WeeklyCycleAnchor[];
  records: Iterable<UsageRecord>;
  now?: Date;
  start?: Date;
  end?: Date;
  estimateBeforeAnchor?: boolean;
  usageDiagnostics?: UsageDiagnostics;
};

export type WeeklyCycleHistoryReport = {
  status: WeeklyCycleHistoryStatus;
  periodHours: typeof WEEKLY_CYCLE_PERIOD_HOURS;
  start?: Date;
  end: Date;
  rows: WeeklyCycleReportRow[];
  totals: WeeklyCycleUsageTotals;
  diagnostics: WeeklyCycleDiagnostics;
};

export type BuildWeeklyCycleCurrentOptions = {
  anchors: WeeklyCycleAnchor[];
  records: Iterable<UsageRecord>;
  now?: Date;
  usageDiagnostics?: UsageDiagnostics;
};

export type WeeklyCycleCurrentReport = {
  status: WeeklyCycleCurrentStatus;
  periodHours: typeof WEEKLY_CYCLE_PERIOD_HOURS;
  now: Date;
  current?: WeeklyCycleReportRow;
  byDay: WeeklyCycleBreakdownRow[];
  byModel: WeeklyCycleBreakdownRow[];
  totals: WeeklyCycleUsageTotals;
  diagnostics: WeeklyCycleDiagnostics;
};

export type BuildWeeklyCycleDetailOptions = {
  history: WeeklyCycleHistoryReport;
  cycleId: string;
  records: Iterable<UsageRecord>;
  usageDiagnostics?: UsageDiagnostics;
};

export type WeeklyCycleDetailReport = {
  status: WeeklyCycleDetailStatus;
  cycleId: string;
  periodHours: typeof WEEKLY_CYCLE_PERIOD_HOURS;
  start?: Date;
  end: Date;
  row: WeeklyCycleReportRow;
  byDay: WeeklyCycleBreakdownRow[];
  byModel: WeeklyCycleBreakdownRow[];
  totals: WeeklyCycleUsageTotals;
  diagnostics: WeeklyCycleDiagnostics;
};

export type WeeklyCycleReportContext = {
  accountId?: string;
  accountSource?: WeeklyCycleAccountSource;
  cycleFile?: string;
};

type InternalWeeklyCycleWindow = WeeklyCycleWindow & {
  exclusiveEnd: Date;
};

type WeeklyCycleAnchorWithDate = WeeklyCycleAnchor & {
  atDate: Date;
};

const DATE_TIME_PATTERN =
  /^(\d{4})-(\d{2})-(\d{2})(?:(?:T| )(\d{2}):(\d{2})(?::(\d{2}))?(Z|[+-]\d{2}:\d{2})?)?$/;

const EMPTY_USAGE: TokenUsage = {
  inputTokens: 0,
  cachedInputTokens: 0,
  outputTokens: 0,
  reasoningOutputTokens: 0,
  totalTokens: 0
};

export function createEmptyWeeklyCycleStore(): WeeklyCycleStore {
  return {
    version: WEEKLY_CYCLE_STORE_VERSION,
    accounts: {}
  };
}

export function resolveWeeklyCycleStoreFile(options: WeeklyCycleStoreFileOptions = {}) {
  if (options.cycleFile !== undefined) {
    return resolve(options.cycleFile);
  }

  return join(resolveCodexHelperDir({ codexHome: options.codexHome }), "stat-cycles.json");
}

export async function readWeeklyCycleStore(cycleFile: string): Promise<WeeklyCycleStore> {
  let content: string;

  try {
    content = await readFile(cycleFile, "utf8");
  } catch (error) {
    if (isNodeError(error) && error.code === "ENOENT") {
      return createEmptyWeeklyCycleStore();
    }

    throw error;
  }

  return parseWeeklyCycleStore(content, cycleFile);
}

export async function writeWeeklyCycleStore(cycleFile: string, store: WeeklyCycleStore) {
  await writeSensitiveFile(cycleFile, `${JSON.stringify(normalizeWeeklyCycleStore(store), null, 2)}\n`);
}

export async function addWeeklyCycleAnchorToFile(
  options: AddWeeklyCycleAnchorToFileOptions,
  now = new Date()
): Promise<WeeklyCycleAnchorMutationReport> {
  const cycleFile = resolveWeeklyCycleStoreFile(options);
  const account = resolveWeeklyCycleAccount(options);
  const store = await readWeeklyCycleStore(cycleFile);
  const result = addWeeklyCycleAnchor(
    store,
    {
      accountId: account.accountId,
      at: options.at,
      note: options.note
    },
    now
  );

  await writeWeeklyCycleStore(cycleFile, result.store);

  return {
    cycleFile,
    accountId: account.accountId,
    accountSource: account.source,
    anchor: result.anchor,
    store: result.store
  };
}

export async function addWeeklyCycleAnchorsToFile(
  options: AddWeeklyCycleAnchorsToFileOptions,
  now = new Date()
): Promise<WeeklyCycleAnchorBatchMutationReport> {
  if (options.at.length === 0) {
    throw new Error("At least one weekly cycle anchor time is required.");
  }

  const cycleFile = resolveWeeklyCycleStoreFile(options);
  const account = resolveWeeklyCycleAccount(options);
  const store = await readWeeklyCycleStore(cycleFile);
  let nextStore = store;
  const anchors: WeeklyCycleAnchor[] = [];

  for (const at of options.at) {
    const result = addWeeklyCycleAnchor(
      nextStore,
      {
        accountId: account.accountId,
        at,
        note: options.note
      },
      now
    );
    nextStore = result.store;
    anchors.push(result.anchor);
  }

  await writeWeeklyCycleStore(cycleFile, nextStore);

  return {
    cycleFile,
    accountId: account.accountId,
    accountSource: account.source,
    anchors,
    store: nextStore
  };
}

export async function listWeeklyCycleAnchorsFromFile(
  options: WeeklyCycleAnchorFileOptions = {}
): Promise<WeeklyCycleAnchorListReport> {
  const cycleFile = resolveWeeklyCycleStoreFile(options);
  const account = resolveWeeklyCycleAccount(options);
  const store = await readWeeklyCycleStore(cycleFile);

  return {
    cycleFile,
    accountId: account.accountId,
    accountSource: account.source,
    anchors: listWeeklyCycleAnchors(store, account.accountId),
    store
  };
}

export async function removeWeeklyCycleAnchorFromFile(
  anchorId: string,
  options: WeeklyCycleAnchorFileOptions = {}
): Promise<WeeklyCycleAnchorMutationReport> {
  const cycleFile = resolveWeeklyCycleStoreFile(options);
  const account = resolveWeeklyCycleAccount(options);
  const store = await readWeeklyCycleStore(cycleFile);
  const result = removeWeeklyCycleAnchor(store, account.accountId, anchorId);

  await writeWeeklyCycleStore(cycleFile, result.store);

  return {
    cycleFile,
    accountId: account.accountId,
    accountSource: account.source,
    anchor: result.removed,
    store: result.store
  };
}

export function addWeeklyCycleAnchor(
  store: WeeklyCycleStore,
  options: AddWeeklyCycleAnchorOptions,
  now = new Date()
): AddWeeklyCycleAnchorResult {
  const accountId = normalizeRequiredAccountId(options.accountId, "account id");
  const parsed = parseWeeklyCycleAnchorTime(options.at);
  const current = normalizeWeeklyCycleStore(store);
  const entry = current.accounts[accountId] ?? createWeeklyCycleAccountEntry();

  if (entry.weekly.anchors.some((anchor) => anchor.at === parsed.atIso)) {
    throw new Error(`Weekly cycle anchor already exists for account ${accountId} at ${parsed.atIso}.`);
  }

  const anchor: WeeklyCycleAnchor = {
    id: weeklyCycleAnchorId(parsed.at),
    at: parsed.atIso,
    input: parsed.input,
    timeZone: parsed.timeZone,
    source: "manual",
    note: options.note ?? "",
    createdAt: now.toISOString()
  };
  const nextAnchors = sortWeeklyCycleAnchors([...entry.weekly.anchors, anchor]);

  return {
    store: {
      version: WEEKLY_CYCLE_STORE_VERSION,
      accounts: {
        ...current.accounts,
        [accountId]: {
          weekly: {
            periodHours: WEEKLY_CYCLE_PERIOD_HOURS,
            anchors: nextAnchors
          }
        }
      }
    },
    anchor
  };
}

export function listWeeklyCycleAnchors(store: WeeklyCycleStore, accountId: string) {
  const normalized = normalizeWeeklyCycleStore(store);
  return sortWeeklyCycleAnchors(normalized.accounts[accountId]?.weekly.anchors ?? []);
}

export function removeWeeklyCycleAnchor(
  store: WeeklyCycleStore,
  accountId: string,
  anchorId: string
): RemoveWeeklyCycleAnchorResult {
  const normalizedAccountId = normalizeRequiredAccountId(accountId, "account id");
  const normalizedAnchorId = normalizeRequiredAccountId(anchorId, "anchor id");
  const current = normalizeWeeklyCycleStore(store);
  const entry = current.accounts[normalizedAccountId] ?? createWeeklyCycleAccountEntry();
  const removed = entry.weekly.anchors.find((anchor) => anchor.id === normalizedAnchorId);

  if (removed === undefined) {
    throw new Error(
      `No weekly cycle anchor found for account ${normalizedAccountId}: ${normalizedAnchorId}.`
    );
  }

  const nextAnchors = entry.weekly.anchors.filter((anchor) => anchor.id !== normalizedAnchorId);

  return {
    store: {
      version: WEEKLY_CYCLE_STORE_VERSION,
      accounts: {
        ...current.accounts,
        [normalizedAccountId]: {
          weekly: {
            periodHours: WEEKLY_CYCLE_PERIOD_HOURS,
            anchors: nextAnchors
          }
        }
      }
    },
    removed
  };
}

export function buildWeeklyCycleHistoryReport(
  options: BuildWeeklyCycleHistoryOptions
): WeeklyCycleHistoryReport {
  const now = options.now ?? new Date();
  const records = sortUsageRecords([...options.records]);
  const anchors = sortAnchorsWithDates(options.anchors);
  const start = options.start;
  const end = options.end ?? now;
  const estimateBeforeAnchor = options.estimateBeforeAnchor === true;
  const emptyTotals = emptyWeeklyCycleTotals();

  if (anchors.length === 0) {
    return {
      status: "unanchored",
      periodHours: WEEKLY_CYCLE_PERIOD_HOURS,
      start,
      end,
      rows: [],
      totals: emptyTotals,
      diagnostics: createWeeklyCycleDiagnostics({
        anchors,
        records,
        rows: [],
        totals: emptyTotals,
        estimateBeforeAnchor,
        unanchored: true,
        usageDiagnostics: options.usageDiagnostics
      })
    };
  }

  const firstAnchor = anchors[0];
  if (firstAnchor === undefined) {
    throw new Error("Weekly cycle history requires at least one anchor.");
  }

  const derivedWindows = deriveAnchoredWeeklyCycleWindows(anchors, records, end);
  const estimatedWindows = estimateBeforeAnchor
    ? deriveEstimatedWeeklyCycleWindows(firstAnchor, records, start, end)
    : [];
  const windows = [...estimatedWindows, ...derivedWindows]
    .filter((window) => windowOverlapsRange(window, start, end))
    .sort(compareWindows);
  const { rows, totals } = buildCycleRows(windows, records, start, end);

  return {
    status: "ok",
    periodHours: WEEKLY_CYCLE_PERIOD_HOURS,
    start,
    end,
    rows,
    totals,
    diagnostics: createWeeklyCycleDiagnostics({
      anchors,
      records,
      rows,
      totals,
      estimateBeforeAnchor,
      unanchored: false,
      usageDiagnostics: options.usageDiagnostics
    })
  };
}

export function buildWeeklyCycleCurrentReport(
  options: BuildWeeklyCycleCurrentOptions
): WeeklyCycleCurrentReport {
  const now = options.now ?? new Date();
  const records = sortUsageRecords([...options.records].filter((record) => record.timestamp <= now));
  const anchors = sortAnchorsWithDates(options.anchors).filter((anchor) => anchor.atDate <= now);
  const emptyTotals = emptyWeeklyCycleTotals();

  if (anchors.length === 0) {
    return {
      status: "unanchored",
      periodHours: WEEKLY_CYCLE_PERIOD_HOURS,
      now,
      byDay: [],
      byModel: [],
      totals: emptyTotals,
      diagnostics: createWeeklyCycleDiagnostics({
        anchors,
        records,
        rows: [],
        totals: emptyTotals,
        estimateBeforeAnchor: false,
        unanchored: true,
        usageDiagnostics: options.usageDiagnostics
      })
    };
  }

  const windows = deriveAnchoredWeeklyCycleWindows(anchors, records, now);
  const currentWindow = windows.at(-1);

  if (currentWindow === undefined) {
    return {
      status: "unanchored",
      periodHours: WEEKLY_CYCLE_PERIOD_HOURS,
      now,
      byDay: [],
      byModel: [],
      totals: emptyTotals,
      diagnostics: createWeeklyCycleDiagnostics({
        anchors,
        records,
        rows: [],
        totals: emptyTotals,
        estimateBeforeAnchor: false,
        unanchored: true,
        usageDiagnostics: options.usageDiagnostics
      })
    };
  }

  const { rows, totals } = buildCycleRows([currentWindow], records);
  const current = rows[0];
  const currentRecords = records.filter((record) =>
    recordBelongsToWindow(record, currentWindow, undefined, undefined)
  );
  const status: WeeklyCycleCurrentStatus =
    currentWindow.resetAt <= now ? "waiting_for_usage" : "active";

  return {
    status,
    periodHours: WEEKLY_CYCLE_PERIOD_HOURS,
    now,
    current,
    byDay: buildWeeklyCycleBreakdown(currentRecords, (record) => localDateKey(record.timestamp)),
    byModel: buildWeeklyCycleBreakdown(currentRecords, (record) => record.model),
    totals,
    diagnostics: createWeeklyCycleDiagnostics({
      anchors,
      records,
      rows,
      totals,
      estimateBeforeAnchor: false,
      unanchored: false,
      usageDiagnostics: options.usageDiagnostics
    })
  };
}

export function buildWeeklyCycleDetailReport(
  options: BuildWeeklyCycleDetailOptions
): WeeklyCycleDetailReport {
  const cycleId = normalizeRequiredAccountId(options.cycleId, "cycle id");
  const row = findWeeklyCycleHistoryRow(options.history, cycleId);

  if (row === undefined) {
    throw new Error(`No weekly cycle found for id: ${cycleId}`);
  }

  const records = sortUsageRecords([...options.records]);
  const rowRecords = records.filter((record) =>
    recordBelongsToReportRow(record, row, options.history.start, options.history.end)
  );

  return {
    status: "ok",
    cycleId: row.id,
    periodHours: options.history.periodHours,
    start: options.history.start,
    end: options.history.end,
    row,
    byDay: buildWeeklyCycleBreakdown(rowRecords, (record) => localDateKey(record.timestamp)),
    byModel: buildWeeklyCycleBreakdown(rowRecords, (record) => record.model),
    totals: row,
    diagnostics: {
      ...options.history.diagnostics,
      usageRecords: records.length,
      windows: 1,
      derivedWindows: row.source === "derived" ? 1 : 0,
      estimatedWindows: row.source === "estimated" ? 1 : 0,
      includedUsageEvents: row.calls,
      usageDiagnostics: options.usageDiagnostics ?? options.history.diagnostics.usageDiagnostics
    }
  };
}

export function findWeeklyCycleHistoryRow(report: WeeklyCycleHistoryReport, cycleId: string) {
  const normalizedCycleId = normalizeRequiredAccountId(cycleId, "cycle id");
  return report.rows.find((row) => row.id === normalizedCycleId);
}

export function formatWeeklyCycleAnchorList(
  report: WeeklyCycleAnchorListReport,
  format: StatFormat = "table"
) {
  if (format === "json") {
    return `${JSON.stringify(toWeeklyCycleAnchorListJson(report), null, 2)}\n`;
  }

  const rows = [
    anchorHeaders(),
    ...report.anchors.map((anchor) => anchorRow(anchor, report.accountId))
  ];

  if (format === "csv") {
    return `${formatCsv(rows)}\n`;
  }

  if (format === "markdown") {
    return `${formatMarkdownTable(rows)}\n`;
  }

  const lines = [
    "Codex weekly cycle anchors",
    `Account: ${report.accountId} (${report.accountSource})`,
    `Cycle file: ${report.cycleFile}`,
    ""
  ];

  if (report.anchors.length === 0) {
    lines.push("No weekly cycle anchors configured.");
    return lines.join("\n");
  }

  lines.push(formatTable(rows, report.anchors.length));
  return lines.join("\n");
}

export function formatWeeklyCycleCurrent(
  report: WeeklyCycleCurrentReport,
  format: StatFormat = "table",
  context: WeeklyCycleReportContext = {}
) {
  if (format === "json") {
    return `${JSON.stringify(toWeeklyCycleCurrentJson(report, context), null, 2)}\n`;
  }

  const rows = [currentHeaders()];

  if (report.current !== undefined) {
    rows.push(currentRow(report.current, report.status));
  }

  if (format === "csv") {
    return `${formatCsv(rows)}\n`;
  }

  if (format === "markdown") {
    return `${formatMarkdownTable(rows)}\n`;
  }

  const lines = [
    "Codex weekly cycle current",
    `Status: ${report.status}`,
    `Now: ${formatDateTime(report.now)} (${report.now.toISOString()})`,
    ...formatContextLines(context),
    ""
  ];

  if (report.status === "unanchored") {
    lines.push("No weekly cycle anchors configured.");
    appendCycleDiagnostics(lines, report.diagnostics);
    return lines.join("\n");
  }

  if (report.current === undefined) {
    lines.push("No current weekly cycle could be resolved.");
    appendCycleDiagnostics(lines, report.diagnostics);
    return lines.join("\n");
  }

  lines.push("Summary:");
  lines.push(formatTable(rows, 1));
  appendCurrentBreakdown(lines, "By day:", report.byDay);
  appendCurrentBreakdown(lines, "By model:", report.byModel);
  appendUnpricedNotes(lines, report.totals);
  appendCycleDiagnostics(lines, report.diagnostics);
  return lines.join("\n");
}

export function formatWeeklyCycleDetail(
  report: WeeklyCycleDetailReport,
  format: StatFormat = "table",
  context: WeeklyCycleReportContext = {}
) {
  if (format === "json") {
    return `${JSON.stringify(toWeeklyCycleDetailJson(report, context), null, 2)}\n`;
  }

  const rows = [detailHeaders(), detailRow(report.row)];

  if (format === "csv") {
    return `${formatCsv(rows)}\n`;
  }

  if (format === "markdown") {
    return `${formatMarkdownTable(rows)}\n`;
  }

  const lines = [
    "Codex weekly cycle detail",
    `Cycle ID: ${report.cycleId}`,
    `Status: ${report.status}`,
    `Cycle: ${formatDateTime(report.row.start)} to ${formatDateTime(report.row.resetAt)}`,
    `History range: ${formatOptionalDate(report.start)} to ${formatDateTime(report.end)}`,
    ...formatContextLines(context),
    "",
    "Summary:",
    formatTable(rows, 1)
  ];

  appendCurrentBreakdown(lines, "By day:", report.byDay);
  appendCurrentBreakdown(lines, "By model:", report.byModel);
  appendUnpricedNotes(lines, report.totals);
  appendCycleDiagnostics(lines, report.diagnostics);
  return lines.join("\n");
}

export function formatWeeklyCycleHistory(
  report: WeeklyCycleHistoryReport,
  format: StatFormat = "table",
  context: WeeklyCycleReportContext = {}
) {
  if (format === "json") {
    return `${JSON.stringify(toWeeklyCycleHistoryJson(report, context), null, 2)}\n`;
  }

  const rows = [historyHeaders(), ...report.rows.map(historyRow), historyTotalRow(report.totals)];

  if (format === "csv") {
    return `${formatCsv(rows)}\n`;
  }

  if (format === "markdown") {
    return `${formatMarkdownTable(rows)}\n`;
  }

  const lines = [
    "Codex weekly cycle history",
    `Status: ${report.status}`,
    `Range: ${formatOptionalDate(report.start)} to ${formatDateTime(report.end)}`,
    ...formatContextLines(context),
    ""
  ];

  if (report.status === "unanchored") {
    lines.push("No weekly cycle anchors configured.");
    appendCycleDiagnostics(lines, report.diagnostics);
    return lines.join("\n");
  }

  if (report.rows.length === 0) {
    lines.push("No weekly cycle usage found in this range.");
    appendCycleDiagnostics(lines, report.diagnostics);
    return lines.join("\n");
  }

  lines.push(formatTable(rows, report.rows.length));
  appendUnpricedNotes(lines, report.totals);
  appendCycleDiagnostics(lines, report.diagnostics);
  return lines.join("\n");
}

export function toWeeklyCycleAnchorListJson(report: WeeklyCycleAnchorListReport) {
  return {
    accountId: report.accountId,
    accountSource: report.accountSource,
    cycleFile: report.cycleFile,
    periodHours: WEEKLY_CYCLE_PERIOD_HOURS,
    anchors: report.anchors
  };
}

export function toWeeklyCycleCurrentJson(
  report: WeeklyCycleCurrentReport,
  context: WeeklyCycleReportContext = {}
) {
  return {
    ...context,
    status: report.status,
    periodHours: report.periodHours,
    now: report.now.toISOString(),
    current: report.current === undefined ? undefined : cycleRowToJson(report.current),
    byDay: report.byDay.map(breakdownRowToJson),
    byModel: report.byModel.map(breakdownRowToJson),
    totals: usageTotalsToJson(report.totals),
    diagnostics: report.diagnostics
  };
}

export function toWeeklyCycleDetailJson(
  report: WeeklyCycleDetailReport,
  context: WeeklyCycleReportContext = {}
) {
  return {
    ...context,
    status: report.status,
    cycleId: report.cycleId,
    periodHours: report.periodHours,
    historyStart: report.start?.toISOString(),
    historyEnd: report.end.toISOString(),
    cycle: cycleRowToJson(report.row),
    byDay: report.byDay.map(breakdownRowToJson),
    byModel: report.byModel.map(breakdownRowToJson),
    totals: usageTotalsToJson(report.totals),
    diagnostics: report.diagnostics
  };
}

export function toWeeklyCycleHistoryJson(
  report: WeeklyCycleHistoryReport,
  context: WeeklyCycleReportContext = {}
) {
  return {
    ...context,
    status: report.status,
    periodHours: report.periodHours,
    start: report.start?.toISOString(),
    end: report.end.toISOString(),
    rows: report.rows.map(cycleRowToJson),
    totals: usageTotalsToJson(report.totals),
    diagnostics: report.diagnostics
  };
}

export function resolveWeeklyCycleAccount(
  options: WeeklyCycleAccountOptions = {}
): WeeklyCycleAccountResolution {
  if (options.accountId !== undefined) {
    return {
      accountId: normalizeRequiredAccountId(options.accountId, "--account-id"),
      source: "explicit",
      isDefault: false
    };
  }

  const chatgptAccountId = normalizeOptionalAccountId(options.authStatus?.summary?.chatgptAccountId);

  if (chatgptAccountId !== undefined) {
    return {
      accountId: chatgptAccountId,
      source: "chatgpt_account_id",
      isDefault: false
    };
  }

  const tokenAccountId = normalizeOptionalAccountId(options.authStatus?.summary?.tokenAccountId);

  if (tokenAccountId !== undefined) {
    return {
      accountId: tokenAccountId,
      source: "token_account_id",
      isDefault: false
    };
  }

  return {
    accountId: DEFAULT_WEEKLY_CYCLE_ACCOUNT_ID,
    source: "default",
    isDefault: true
  };
}

export function parseWeeklyCycleAnchorTime(input: string): ParsedWeeklyCycleAnchorTime {
  const trimmed = input.trim();
  const match = DATE_TIME_PATTERN.exec(trimmed);

  if (match === null) {
    throw new Error(
      `Invalid weekly cycle anchor time: ${input}. Expected YYYY-MM-DD, YYYY-MM-DD HH:mm, or an ISO time with offset.`
    );
  }

  const year = parseDatePart(match[1], "year", input);
  const month = parseDatePart(match[2], "month", input);
  const day = parseDatePart(match[3], "day", input);
  const hasTime = match[4] !== undefined;
  const hour = hasTime ? parseDatePart(match[4], "hour", input) : 0;
  const minute = hasTime ? parseDatePart(match[5], "minute", input) : 0;
  const second = match[6] === undefined ? 0 : parseDatePart(match[6], "second", input);
  const offset = match[7];
  const at =
    offset === undefined
      ? buildLocalDate(year, month, day, hour, minute, second, input)
      : buildOffsetDate(year, month, day, hour, minute, second, offset, input);

  return {
    at,
    atIso: at.toISOString(),
    input: trimmed,
    timeZone: offset === undefined ? localTimeZone() : formatOffsetTimeZone(offset),
    hasExplicitOffset: offset !== undefined
  };
}

export function weeklyCycleAnchorId(date: Date) {
  return `anc_${compactIsoTimestamp(date)}`;
}

function parseWeeklyCycleStore(content: string, cycleFile: string): WeeklyCycleStore {
  let parsed: unknown;

  try {
    parsed = JSON.parse(content);
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error);
    throw new Error(`Failed to parse ${cycleFile}: ${detail}`);
  }

  if (!isRecord(parsed)) {
    throw new Error(`Expected ${cycleFile} to contain a weekly cycle store object.`);
  }

  if (parsed.version !== WEEKLY_CYCLE_STORE_VERSION) {
    throw new Error(`Unsupported weekly cycle store version in ${cycleFile}: ${String(parsed.version)}.`);
  }

  if (!isRecord(parsed.accounts)) {
    throw new Error(`Expected ${cycleFile} accounts to be an object.`);
  }

  const accounts: Record<string, WeeklyCycleAccountEntry> = {};

  for (const [accountId, accountValue] of Object.entries(parsed.accounts)) {
    accounts[accountId] = parseWeeklyCycleAccountEntry(accountValue, `${cycleFile} accounts.${accountId}`);
  }

  return {
    version: WEEKLY_CYCLE_STORE_VERSION,
    accounts
  };
}

function parseWeeklyCycleAccountEntry(value: unknown, path: string): WeeklyCycleAccountEntry {
  if (!isRecord(value) || !isRecord(value.weekly)) {
    throw new Error(`Expected ${path}.weekly to be an object.`);
  }

  if (value.weekly.periodHours !== WEEKLY_CYCLE_PERIOD_HOURS) {
    throw new Error(`Expected ${path}.weekly.periodHours to be ${WEEKLY_CYCLE_PERIOD_HOURS}.`);
  }

  if (!Array.isArray(value.weekly.anchors)) {
    throw new Error(`Expected ${path}.weekly.anchors to be an array.`);
  }

  return {
    weekly: {
      periodHours: WEEKLY_CYCLE_PERIOD_HOURS,
      anchors: sortWeeklyCycleAnchors(
        value.weekly.anchors.map((anchor, index) =>
          parseWeeklyCycleAnchor(anchor, `${path}.weekly.anchors[${index}]`)
        )
      )
    }
  };
}

function parseWeeklyCycleAnchor(value: unknown, path: string): WeeklyCycleAnchor {
  if (!isRecord(value)) {
    throw new Error(`Expected ${path} to be an object.`);
  }

  const anchor = {
    id: readRequiredString(value.id, `${path}.id`),
    at: readRequiredString(value.at, `${path}.at`),
    input: readRequiredString(value.input, `${path}.input`),
    timeZone: readRequiredString(value.timeZone, `${path}.timeZone`),
    source: readRequiredString(value.source, `${path}.source`),
    note: readOptionalString(value.note) ?? "",
    createdAt: readRequiredString(value.createdAt, `${path}.createdAt`)
  };

  if (anchor.source !== "manual") {
    throw new Error(`Expected ${path}.source to be manual.`);
  }

  assertIsoDate(anchor.at, `${path}.at`);
  assertIsoDate(anchor.createdAt, `${path}.createdAt`);

  return {
    ...anchor,
    source: "manual"
  };
}

function normalizeWeeklyCycleStore(store: WeeklyCycleStore): WeeklyCycleStore {
  const accounts: Record<string, WeeklyCycleAccountEntry> = {};

  for (const [accountId, entry] of Object.entries(store.accounts)) {
    accounts[accountId] = {
      weekly: {
        periodHours: WEEKLY_CYCLE_PERIOD_HOURS,
        anchors: sortWeeklyCycleAnchors(entry.weekly.anchors)
      }
    };
  }

  return {
    version: WEEKLY_CYCLE_STORE_VERSION,
    accounts
  };
}

function deriveAnchoredWeeklyCycleWindows(
  anchors: WeeklyCycleAnchorWithDate[],
  records: UsageRecord[],
  until: Date
): InternalWeeklyCycleWindow[] {
  const windows: InternalWeeklyCycleWindow[] = [];

  for (let index = 0; index < anchors.length; index += 1) {
    const anchor = anchors[index];
    const nextAnchor = anchors[index + 1];

    if (anchor === undefined || anchor.atDate > until) {
      continue;
    }

    let start = anchor.atDate;
    let source: WeeklyCycleWindowSource = "manual";
    let anchorId: string | undefined = anchor.id;

    while (start <= until) {
      const resetAt = addWeeklyCyclePeriod(start);
      const exclusiveEnd =
        nextAnchor === undefined || nextAnchor.atDate < resetAt ? nextAnchor?.atDate ?? resetAt : resetAt;

      windows.push({
        start,
        resetAt,
        exclusiveEnd,
        source,
        anchorId,
        calibrationAnchorId: anchor.id
      });

      const nextStart = records.find(
        (record) =>
          record.timestamp >= resetAt &&
          record.timestamp <= until &&
          (nextAnchor === undefined || record.timestamp < nextAnchor.atDate)
      )?.timestamp;

      if (nextStart === undefined) {
        break;
      }

      start = nextStart;
      source = "derived";
      anchorId = undefined;
    }
  }

  return windows.sort(compareWindows);
}

function deriveEstimatedWeeklyCycleWindows(
  firstAnchor: WeeklyCycleAnchorWithDate,
  records: UsageRecord[],
  start: Date | undefined,
  end: Date
): InternalWeeklyCycleWindow[] {
  const windows = new Map<number, InternalWeeklyCycleWindow>();

  for (const record of records) {
    if (record.timestamp >= firstAnchor.atDate || record.timestamp > end) {
      continue;
    }

    if (start !== undefined && record.timestamp < start) {
      continue;
    }

    const periodsBeforeAnchor = Math.max(
      1,
      Math.ceil((firstAnchor.atDate.getTime() - record.timestamp.getTime()) / WEEKLY_CYCLE_PERIOD_MS)
    );
    const windowStart = new Date(
      firstAnchor.atDate.getTime() - periodsBeforeAnchor * WEEKLY_CYCLE_PERIOD_MS
    );
    const key = windowStart.getTime();

    if (!windows.has(key)) {
      const resetAt = addWeeklyCyclePeriod(windowStart);
      windows.set(key, {
        start: windowStart,
        resetAt,
        exclusiveEnd: resetAt,
        source: "estimated"
      });
    }
  }

  return [...windows.values()].sort(compareWindows);
}

function buildCycleRows(
  windows: InternalWeeklyCycleWindow[],
  records: UsageRecord[],
  rangeStart?: Date,
  rangeEnd?: Date
) {
  const includedRecords: UsageRecord[] = [];
  const rows = windows.map((window, index) => {
    const windowRecords = records.filter((record) =>
      recordBelongsToWindow(record, window, rangeStart, rangeEnd)
    );
    includedRecords.push(...windowRecords);
    return {
      id: weeklyCycleWindowId(window),
      index: index + 1,
      exclusiveEnd: window.exclusiveEnd,
      ...toPublicWindow(window),
      ...aggregateWeeklyCycleRecords(windowRecords)
    };
  });

  return {
    rows,
    totals: aggregateWeeklyCycleRecords(includedRecords)
  };
}

function recordBelongsToWindow(
  record: UsageRecord,
  window: InternalWeeklyCycleWindow,
  rangeStart: Date | undefined,
  rangeEnd: Date | undefined
) {
  return (
    record.timestamp >= window.start &&
    record.timestamp < window.exclusiveEnd &&
    (rangeStart === undefined || record.timestamp >= rangeStart) &&
    (rangeEnd === undefined || record.timestamp <= rangeEnd)
  );
}

function recordBelongsToReportRow(
  record: UsageRecord,
  row: WeeklyCycleReportRow,
  rangeStart: Date | undefined,
  rangeEnd: Date | undefined
) {
  return (
    record.timestamp >= row.start &&
    record.timestamp < row.exclusiveEnd &&
    (rangeStart === undefined || record.timestamp >= rangeStart) &&
    (rangeEnd === undefined || record.timestamp <= rangeEnd)
  );
}

function aggregateWeeklyCycleRecords(records: UsageRecord[]): WeeklyCycleUsageTotals {
  const sessions = new Set<string>();
  const usage = { ...EMPTY_USAGE };
  const unpricedModels = new Map<string, WeeklyCycleUnpricedModelRow>();
  let credits = 0;
  let pricedCalls = 0;
  let unpricedCalls = 0;

  for (const record of records) {
    const cost = calculateCreditCost(record.model, record.usage);
    sessions.add(record.sessionId);
    addUsage(usage, record.usage);
    credits += cost.credits;

    if (cost.priced) {
      pricedCalls += 1;
    } else {
      unpricedCalls += 1;
      addUnpricedModel(unpricedModels, record);
    }
  }

  return {
    sessions: sessions.size,
    calls: records.length,
    usage,
    credits: roundCredits(credits),
    usd: creditsToUsd(credits),
    pricedCalls,
    unpricedCalls,
    unpricedModels: formatUnpricedModels(unpricedModels)
  };
}

function buildWeeklyCycleBreakdown(
  records: UsageRecord[],
  keyForRecord: (record: UsageRecord) => string
): WeeklyCycleBreakdownRow[] {
  const grouped = new Map<string, UsageRecord[]>();

  for (const record of records) {
    const key = keyForRecord(record);
    const groupRecords = grouped.get(key) ?? [];
    groupRecords.push(record);
    grouped.set(key, groupRecords);
  }

  return [...grouped.entries()]
    .map(([key, groupRecords]) => ({
      key,
      ...aggregateWeeklyCycleRecords(groupRecords)
    }))
    .sort((left, right) => left.key.localeCompare(right.key));
}

function createWeeklyCycleDiagnostics(options: {
  anchors: WeeklyCycleAnchorWithDate[];
  records: UsageRecord[];
  rows: WeeklyCycleReportRow[];
  totals: WeeklyCycleUsageTotals;
  estimateBeforeAnchor: boolean;
  unanchored: boolean;
  usageDiagnostics?: UsageDiagnostics;
}): WeeklyCycleDiagnostics {
  const firstAnchor = options.anchors[0];

  return {
    anchors: options.anchors.length,
    usageRecords: options.records.length,
    windows: options.rows.length,
    derivedWindows: options.rows.filter((row) => row.source === "derived").length,
    estimatedWindows: options.rows.filter((row) => row.source === "estimated").length,
    includedUsageEvents: options.totals.calls,
    ignoredBeforeAnchorEvents:
      firstAnchor === undefined || options.estimateBeforeAnchor
        ? 0
        : options.records.filter((record) => record.timestamp < firstAnchor.atDate).length,
    estimateBeforeAnchor: options.estimateBeforeAnchor,
    unanchored: options.unanchored,
    usageDiagnostics: options.usageDiagnostics
  };
}

function toPublicWindow(window: InternalWeeklyCycleWindow): WeeklyCycleWindow {
  const publicWindow: WeeklyCycleWindow = {
    start: window.start,
    resetAt: window.resetAt,
    source: window.source
  };

  if (window.anchorId !== undefined) {
    publicWindow.anchorId = window.anchorId;
  }

  if (window.calibrationAnchorId !== undefined) {
    publicWindow.calibrationAnchorId = window.calibrationAnchorId;
  }

  return publicWindow;
}

function sortAnchorsWithDates(anchors: WeeklyCycleAnchor[]): WeeklyCycleAnchorWithDate[] {
  return anchors
    .map((anchor) => ({
      ...anchor,
      atDate: new Date(anchor.at)
    }))
    .filter((anchor) => !Number.isNaN(anchor.atDate.getTime()))
    .sort((left, right) => left.atDate.getTime() - right.atDate.getTime() || left.id.localeCompare(right.id));
}

function sortUsageRecords(records: UsageRecord[]) {
  return records.sort(
    (left, right) =>
      left.timestamp.getTime() - right.timestamp.getTime() ||
      left.sessionId.localeCompare(right.sessionId) ||
      left.filePath.localeCompare(right.filePath)
  );
}

function compareWindows(left: WeeklyCycleWindow, right: WeeklyCycleWindow) {
  return (
    left.start.getTime() - right.start.getTime() ||
    sourceSortKey(left.source) - sourceSortKey(right.source) ||
    (left.anchorId ?? "").localeCompare(right.anchorId ?? "")
  );
}

function sourceSortKey(source: WeeklyCycleWindowSource) {
  switch (source) {
    case "estimated":
      return 0;
    case "manual":
      return 1;
    case "derived":
      return 2;
  }
}

function windowOverlapsRange(
  window: InternalWeeklyCycleWindow,
  rangeStart: Date | undefined,
  rangeEnd: Date
) {
  return window.start <= rangeEnd && (rangeStart === undefined || window.exclusiveEnd > rangeStart);
}

function addWeeklyCyclePeriod(date: Date) {
  return new Date(date.getTime() + WEEKLY_CYCLE_PERIOD_MS);
}

function addUsage(target: TokenUsage, source: TokenUsage) {
  target.inputTokens += source.inputTokens;
  target.cachedInputTokens += source.cachedInputTokens;
  target.outputTokens += source.outputTokens;
  target.reasoningOutputTokens += source.reasoningOutputTokens;
  target.totalTokens += source.totalTokens;
}

function addUnpricedModel(unpricedModels: Map<string, WeeklyCycleUnpricedModelRow>, record: UsageRecord) {
  const pricingKey = normalizeModelName(record.model);
  const row =
    unpricedModels.get(pricingKey) ??
    {
      model: record.model,
      pricingKey,
      calls: 0,
      totalTokens: 0,
      pricingStub: formatPricingStub(record.model)
    };

  row.calls += 1;
  row.totalTokens += record.usage.totalTokens;
  unpricedModels.set(pricingKey, row);
}

function formatUnpricedModels(unpricedModels: Map<string, WeeklyCycleUnpricedModelRow>) {
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

function roundCredits(value: number) {
  return Math.round(value * 1_000_000) / 1_000_000;
}

function creditsToUsd(credits: number) {
  return Math.round((credits / 25) * 1_000_000) / 1_000_000;
}

function emptyWeeklyCycleTotals(): WeeklyCycleUsageTotals {
  return {
    sessions: 0,
    calls: 0,
    usage: { ...EMPTY_USAGE },
    credits: 0,
    usd: 0,
    pricedCalls: 0,
    unpricedCalls: 0,
    unpricedModels: []
  };
}

function anchorHeaders() {
  return ["Account", "ID", "Local time", "UTC at", "Source", "Note", "Created at"];
}

function anchorRow(anchor: WeeklyCycleAnchor, accountId: string) {
  return [
    accountId,
    anchor.id,
    formatDateTime(new Date(anchor.at)),
    anchor.at,
    anchor.source,
    anchor.note,
    anchor.createdAt
  ];
}

function currentHeaders() {
  return ["Status", "Start", "Reset at", "Source", "Sessions", "Calls", "Total", "Credits", "USD"];
}

function currentRow(row: WeeklyCycleReportRow, status: WeeklyCycleCurrentStatus) {
  return [
    status,
    formatDateTime(row.start),
    formatDateTime(row.resetAt),
    row.source,
    formatInteger(row.sessions),
    formatInteger(row.calls),
    formatInteger(row.usage.totalTokens),
    formatCredits(row.credits),
    formatUsd(row.usd)
  ];
}

function currentBreakdownHeaders() {
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

function currentBreakdownRow(row: WeeklyCycleBreakdownRow) {
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

function historyHeaders() {
  return [
    "ID",
    "Start",
    "Reset at",
    "Source",
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

function historyRow(row: WeeklyCycleReportRow) {
  return [
    row.id,
    formatDateTime(row.start),
    formatDateTime(row.resetAt),
    row.source,
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

function historyTotalRow(totals: WeeklyCycleUsageTotals) {
  return [
    "Total",
    "",
    "",
    "",
    formatInteger(totals.sessions),
    formatInteger(totals.calls),
    formatInteger(totals.usage.inputTokens),
    formatInteger(totals.usage.cachedInputTokens),
    formatInteger(totals.usage.outputTokens),
    formatInteger(totals.usage.reasoningOutputTokens),
    formatInteger(totals.usage.totalTokens),
    formatCredits(totals.credits),
    formatUsd(totals.usd)
  ];
}

function detailHeaders() {
  return ["ID", "Start", "Reset at", "Source", "Sessions", "Calls", "Total", "Credits", "USD"];
}

function detailRow(row: WeeklyCycleReportRow) {
  return [
    row.id,
    formatDateTime(row.start),
    formatDateTime(row.resetAt),
    row.source,
    formatInteger(row.sessions),
    formatInteger(row.calls),
    formatInteger(row.usage.totalTokens),
    formatCredits(row.credits),
    formatUsd(row.usd)
  ];
}

function formatContextLines(context: WeeklyCycleReportContext) {
  const lines: string[] = [];

  if (context.accountId !== undefined) {
    lines.push(`Account: ${context.accountId}${formatAccountSource(context.accountSource)}`);
  }

  if (context.cycleFile !== undefined) {
    lines.push(`Cycle file: ${context.cycleFile}`);
  }

  return lines;
}

function formatAccountSource(source: WeeklyCycleAccountSource | undefined) {
  return source === undefined ? "" : ` (${source})`;
}

function appendCycleDiagnostics(lines: string[], diagnostics: WeeklyCycleDiagnostics) {
  lines.push(
    "",
    "Diagnostics:",
    `  Anchors: ${formatInteger(diagnostics.anchors)}`,
    `  Windows: ${formatInteger(diagnostics.windows)}`,
    `  Derived windows: ${formatInteger(diagnostics.derivedWindows)}`,
    `  Estimated windows: ${formatInteger(diagnostics.estimatedWindows)}`,
    `  Usage records: ${formatInteger(diagnostics.usageRecords)}`,
    `  Usage events included: ${formatInteger(diagnostics.includedUsageEvents)}`,
    `  Ignored before anchor: ${formatInteger(diagnostics.ignoredBeforeAnchorEvents)}`
  );
}

function appendUnpricedNotes(lines: string[], totals: WeeklyCycleUsageTotals) {
  if (totals.unpricedCalls === 0) {
    return;
  }

  lines.push(
    "",
    `Note: ${formatInteger(
      totals.unpricedCalls
    )} usage events had no credit price and are excluded from Credits.`,
    "Unpriced models:"
  );

  for (const row of totals.unpricedModels) {
    lines.push(
      `  ${row.model}: ${formatInteger(row.calls)} calls, ${formatInteger(row.totalTokens)} tokens`
    );
  }
}

function appendCurrentBreakdown(lines: string[], title: string, rows: WeeklyCycleBreakdownRow[]) {
  lines.push("", title);

  if (rows.length === 0) {
    lines.push("No usage events in this cycle.");
    return;
  }

  lines.push(formatTable([currentBreakdownHeaders(), ...rows.map(currentBreakdownRow)], rows.length));
}

function cycleRowToJson(row: WeeklyCycleReportRow) {
  return {
    ...usageTotalsToJson(row),
    id: row.id,
    index: row.index,
    start: row.start.toISOString(),
    resetAt: row.resetAt.toISOString(),
    exclusiveEnd: row.exclusiveEnd.toISOString(),
    source: row.source,
    anchorId: row.anchorId,
    calibrationAnchorId: row.calibrationAnchorId
  };
}

function weeklyCycleWindowId(window: WeeklyCycleWindow) {
  if (window.source === "manual" && window.anchorId !== undefined) {
    return window.anchorId;
  }

  const prefix = window.source === "estimated" ? "est" : "cyc";
  return `${prefix}_${compactIsoTimestamp(window.start)}`;
}

function compactIsoTimestamp(date: Date) {
  return date.toISOString().replace(/[-:]/g, "").replace(".", "");
}

function usageTotalsToJson(totals: WeeklyCycleUsageTotals) {
  return {
    sessions: totals.sessions,
    calls: totals.calls,
    usage: totals.usage,
    credits: totals.credits,
    usd: totals.usd,
    pricedCalls: totals.pricedCalls,
    unpricedCalls: totals.unpricedCalls,
    unpricedModels: totals.unpricedModels
  };
}

function breakdownRowToJson(row: WeeklyCycleBreakdownRow) {
  return {
    key: row.key,
    ...usageTotalsToJson(row)
  };
}

function formatTable(rows: string[][], bodyRows: number) {
  const widths = columnWidths(rows);
  const header = rows[0] ?? [];
  const body = rows.slice(1);
  const lines = [formatTableRow(header, widths), formatTableSeparator(widths)];

  for (const [index, row] of body.entries()) {
    if (index === bodyRows) {
      lines.push(formatTableSeparator(widths));
    }

    lines.push(formatTableRow(row, widths));
  }

  return lines.join("\n");
}

function formatTableRow(row: string[], widths: number[]) {
  return row.map((cell, index) => cell.padEnd(widths[index] ?? 0)).join("  ");
}

function formatTableSeparator(widths: number[]) {
  return widths.map((width) => "-".repeat(width)).join("  ");
}

function columnWidths(rows: string[][]) {
  const widthCount = Math.max(0, ...rows.map((row) => row.length));
  return Array.from({ length: widthCount }, (_, index) =>
    Math.max(...rows.map((row) => row[index]?.length ?? 0))
  );
}

function formatCsv(rows: string[][]) {
  return rows.map((row) => row.map(csvCell).join(",")).join("\n");
}

function csvCell(value: string) {
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
    markdownRow(header),
    markdownRow(header.map(() => "---")),
    ...body.map(markdownRow)
  ].join("\n");
}

function markdownRow(row: string[]) {
  return `| ${row.map(markdownCell).join(" | ")} |`;
}

function markdownCell(value: string) {
  return value.replaceAll("|", "\\|");
}

function formatDateTime(date: Date) {
  return `${localDateKey(date)} ${pad2(date.getHours())}:${pad2(date.getMinutes())}:${pad2(
    date.getSeconds()
  )}`;
}

function formatOptionalDate(date: Date | undefined) {
  return date === undefined ? "beginning" : formatDateTime(date);
}

function localDateKey(date: Date) {
  return [
    date.getFullYear(),
    pad2(date.getMonth() + 1),
    pad2(date.getDate())
  ].join("-");
}

function pad2(value: number) {
  return String(value).padStart(2, "0");
}

function formatInteger(value: number) {
  return Math.round(value).toLocaleString("en-US");
}

function formatCredits(value: number) {
  return value.toFixed(6).replace(/\.?0+$/, "");
}

function formatUsd(value: number) {
  return `$${value.toFixed(6).replace(/\.?0+$/, "")}`;
}

function createWeeklyCycleAccountEntry(): WeeklyCycleAccountEntry {
  return {
    weekly: {
      periodHours: WEEKLY_CYCLE_PERIOD_HOURS,
      anchors: []
    }
  };
}

function sortWeeklyCycleAnchors(anchors: WeeklyCycleAnchor[]) {
  return [...anchors].sort((left, right) => {
    const byTime = left.at.localeCompare(right.at);
    return byTime === 0 ? left.id.localeCompare(right.id) : byTime;
  });
}

function parseDatePart(value: string | undefined, label: string, input: string) {
  const parsed = Number(value);

  if (value === undefined || !Number.isInteger(parsed)) {
    throw new Error(`Invalid ${label} in weekly cycle anchor time: ${input}.`);
  }

  return parsed;
}

function buildLocalDate(
  year: number,
  month: number,
  day: number,
  hour: number,
  minute: number,
  second: number,
  input: string
) {
  const date = new Date(year, month - 1, day, hour, minute, second, 0);

  if (
    date.getFullYear() !== year ||
    date.getMonth() !== month - 1 ||
    date.getDate() !== day ||
    date.getHours() !== hour ||
    date.getMinutes() !== minute ||
    date.getSeconds() !== second
  ) {
    throw new Error(`Invalid local weekly cycle anchor time: ${input}.`);
  }

  return date;
}

function buildOffsetDate(
  year: number,
  month: number,
  day: number,
  hour: number,
  minute: number,
  second: number,
  offset: string,
  input: string
) {
  const offsetMinutes = parseOffsetMinutes(offset);
  const wallClockMillis = Date.UTC(year, month - 1, day, hour, minute, second, 0);
  const wallClock = new Date(wallClockMillis);

  if (
    wallClock.getUTCFullYear() !== year ||
    wallClock.getUTCMonth() !== month - 1 ||
    wallClock.getUTCDate() !== day ||
    wallClock.getUTCHours() !== hour ||
    wallClock.getUTCMinutes() !== minute ||
    wallClock.getUTCSeconds() !== second
  ) {
    throw new Error(`Invalid offset weekly cycle anchor time: ${input}.`);
  }

  return new Date(wallClockMillis - offsetMinutes * 60_000);
}

function parseOffsetMinutes(offset: string) {
  if (offset === "Z") {
    return 0;
  }

  const sign = offset.startsWith("-") ? -1 : 1;
  const hour = Number(offset.slice(1, 3));
  const minute = Number(offset.slice(4, 6));

  if (!Number.isInteger(hour) || !Number.isInteger(minute) || hour > 23 || minute > 59) {
    throw new Error(`Invalid timezone offset: ${offset}.`);
  }

  return sign * (hour * 60 + minute);
}

function formatOffsetTimeZone(offset: string) {
  return offset === "Z" ? "UTC" : `UTC${offset}`;
}

function localTimeZone() {
  return Intl.DateTimeFormat().resolvedOptions().timeZone ?? "local";
}

function normalizeRequiredAccountId(value: string, label: string) {
  const normalized = value.trim();

  if (normalized.length === 0) {
    throw new Error(`Weekly cycle ${label} cannot be empty.`);
  }

  return normalized;
}

function normalizeOptionalAccountId(value: string | undefined) {
  const normalized = value?.trim();
  return normalized === undefined || normalized.length === 0 ? undefined : normalized;
}

function assertIsoDate(value: string, path: string) {
  const date = new Date(value);

  if (Number.isNaN(date.getTime()) || date.toISOString() !== value) {
    throw new Error(`Expected ${path} to be a UTC ISO timestamp.`);
  }
}

function readRequiredString(value: unknown, path: string) {
  if (typeof value !== "string" || value.length === 0) {
    throw new Error(`Expected ${path} to be a non-empty string.`);
  }

  return value;
}

function readOptionalString(value: unknown) {
  return typeof value === "string" ? value : undefined;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function isNodeError(error: unknown): error is NodeJS.ErrnoException {
  return error instanceof Error && "code" in error;
}
