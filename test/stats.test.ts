import { mkdir, mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import {
  buildUsageSessionCompactRows,
  buildUsageSessionDetail,
  buildUsageSessions,
  buildUsageStats,
  formatUsageSessionDetail,
  formatUsageSessions,
  formatUsageStats,
  readCodexUsageRecords,
  readCodexUsageSessionDetail,
  readCodexUsageSessions,
  readCodexUsageStats,
  resolveStatRangeOptions,
  resolveStatOptions,
  toUsageStatsJson
} from "../src/stats.js";

describe("stats", () => {
  it("defaults to the previous seven days grouped by day", () => {
    const now = new Date("2026-05-12T12:00:00.000Z");
    const options = resolveStatOptions({ sessionsDir: "/tmp/sessions" }, now);

    expect(options.groupBy).toBe("day");
    expect(options.end.toISOString()).toBe("2026-05-12T12:00:00.000Z");
    expect(options.start.toISOString()).toBe("2026-05-05T12:00:00.000Z");
  });

  it("parses date-only end bounds as the end of the local day", () => {
    const options = resolveStatOptions({
      start: "2026-05-01",
      end: "2026-05-12",
      groupBy: "week",
      sessionsDir: "/tmp/sessions"
    });

    expect(options.groupBy).toBe("week");
    expect(options.start.getHours()).toBe(0);
    expect(options.start.getMinutes()).toBe(0);
    expect(options.end.getHours()).toBe(23);
    expect(options.end.getMinutes()).toBe(59);
  });

  it("supports quick ranges and output formats", () => {
    const now = new Date("2026-05-12T12:34:56.000Z");

    const today = resolveStatOptions({ today: true, sessionsDir: "/tmp/sessions" }, now);
    const month = resolveStatOptions({ month: true, sessionsDir: "/tmp/sessions" }, now);
    const all = resolveStatOptions({ all: true, sessionsDir: "/tmp/sessions" }, now);
    const lastTwoDays = resolveStatOptions({ last: "2d", sessionsDir: "/tmp/sessions" }, now);

    expect(today.start).toEqual(new Date("2026-05-11T16:00:00.000Z"));
    expect(today.groupBy).toBe("hour");
    expect(month.start).toEqual(new Date("2026-04-30T16:00:00.000Z"));
    expect(month.groupBy).toBe("day");
    expect(all.start.getFullYear()).toBe(1900);
    expect(all.end.getFullYear()).toBe(9999);
    expect(all.groupBy).toBe("month");
    expect(all.includeReasoningEffort).toBe(false);
    expect(lastTwoDays.start).toEqual(new Date("2026-05-10T12:34:56.000Z"));
    expect(lastTwoDays.groupBy).toBe("hour");
    expect(
      resolveStatOptions(
        { reasoningEffort: true, groupBy: "model", sessionsDir: "/tmp/sessions" },
        now
      ).includeReasoningEffort
    ).toBe(true);
    expect(resolveStatOptions({ format: "csv", sessionsDir: "/tmp/sessions" }, now).format).toBe(
      "csv"
    );
    expect(resolveStatOptions({ json: true, sessionsDir: "/tmp/sessions" }, now).format).toBe(
      "json"
    );
    expect(() =>
      resolveStatOptions({ today: true, last: "7d", sessionsDir: "/tmp/sessions" }, now)
    ).toThrow("Use only one quick range");
    expect(() =>
      resolveStatOptions({ all: true, start: "2026-05-01", sessionsDir: "/tmp/sessions" }, now)
    ).toThrow("Quick range options cannot be combined");
  });

  it("infers default group-by from the time range", () => {
    const now = new Date("2026-05-12T12:00:00.000Z");

    expect(resolveStatOptions({ yesterday: true, sessionsDir: "/tmp/sessions" }, now).groupBy).toBe(
      "hour"
    );
    expect(
      resolveStatOptions({
        start: "2026-05-10T00:00:00.000Z",
        end: "2026-05-12T00:00:00.000Z",
        sessionsDir: "/tmp/sessions"
      }).groupBy
    ).toBe("hour");
    expect(resolveStatOptions({ last: "31d", sessionsDir: "/tmp/sessions" }, now).groupBy).toBe(
      "day"
    );
    expect(resolveStatOptions({ last: "32d", sessionsDir: "/tmp/sessions" }, now).groupBy).toBe(
      "week"
    );
    expect(resolveStatOptions({ last: "6mo", sessionsDir: "/tmp/sessions" }, now).groupBy).toBe(
      "week"
    );
    expect(
      resolveStatOptions({
        start: "2025-10-01T00:00:00.000Z",
        end: "2026-05-12T00:00:00.000Z",
        sessionsDir: "/tmp/sessions"
      }).groupBy
    ).toBe("month");
    expect(
      resolveStatOptions({ last: "31d", groupBy: "cwd", sessionsDir: "/tmp/sessions" }, now)
        .groupBy
    ).toBe("cwd");
    expect(
      resolveStatRangeOptions({ last: "31d", groupBy: "invalid", sessionsDir: "/tmp/sessions" }, now)
        .format
    ).toBe("table");
  });

  it("reads Codex token_count usage and aggregates by hour, day, month, model, and cwd", async () => {
    const sessionsDir = await createFixtureSessions();
    const start = new Date("2026-05-10T00:00:00.000Z");
    const end = new Date("2026-05-11T23:59:59.999Z");
    const records = await readCodexUsageRecords({ sessionsDir, start, end });

    expect(records).toHaveLength(3);
    expect(records.map((record) => record.model)).toEqual(["gpt-5.5", "gpt-5.5", "gpt-5.4"]);
    expect(records.map((record) => record.reasoningEffort)).toEqual(["high", "xhigh", undefined]);
    expect(records.map((record) => record.cwd)).toEqual([
      "/repo/alpha",
      "/repo/alpha",
      "/repo/beta"
    ]);

    const byDay = buildUsageStats(records, {
      start,
      end,
      groupBy: "day",
      sessionsDir
    });

    expect(byDay.rows).toMatchObject([
      {
        key: "2026-05-10",
        sessions: 1,
        calls: 1,
        usage: { inputTokens: 10, cachedInputTokens: 2, outputTokens: 3, totalTokens: 13 },
        pricedCalls: 1,
        unpricedCalls: 0
      },
      {
        key: "2026-05-11",
        sessions: 2,
        calls: 2,
        usage: { inputTokens: 27, cachedInputTokens: 1, outputTokens: 7, totalTokens: 34 },
        pricedCalls: 2,
        unpricedCalls: 0
      }
    ]);
    expect(byDay.rows[0]?.credits).toBeCloseTo(0.003275);
    expect(byDay.rows[0]?.usd).toBeCloseTo(0.000131);
    expect(byDay.rows[1]?.credits).toBeCloseTo(0.007325);
    expect(byDay.rows[1]?.usd).toBeCloseTo(0.000293);
    expect(byDay.totals.usage.totalTokens).toBe(47);
    expect(byDay.totals.credits).toBeCloseTo(0.0106);
    expect(byDay.totals.usd).toBeCloseTo(0.000424);

    const streamedByDay = await readCodexUsageStats({
      start,
      end,
      groupBy: "day",
      sessionsDir
    });

    expect(streamedByDay.rows).toEqual(byDay.rows);
    expect(streamedByDay.totals).toEqual(byDay.totals);
    expect(streamedByDay.diagnostics).toMatchObject({
      readFiles: 2,
      skippedDirectories: 1,
      skippedFiles: 1,
      includedUsageEvents: 3,
      fileReadConcurrency: 8,
      skippedEvents: {
        outOfRange: 1
      }
    });

    const limitedByTokens = await readCodexUsageStats({
      start,
      end,
      groupBy: "day",
      sortBy: "tokens",
      limit: 1,
      sessionsDir
    });

    expect(limitedByTokens.rows.map((row) => row.key)).toEqual(["2026-05-11"]);
    expect(limitedByTokens.totals.calls).toBe(3);

    const byHour = buildUsageStats(records, {
      start,
      end,
      groupBy: "hour",
      sessionsDir
    });

    expect(byHour.rows.map((row) => row.key)).toEqual([
      "2026-05-10 18:00",
      "2026-05-11 18:00",
      "2026-05-11 19:00"
    ]);

    const byMonth = buildUsageStats(records, {
      start,
      end,
      groupBy: "month",
      sessionsDir
    });

    expect(byMonth.rows.map((row) => row.key)).toEqual(["2026-05"]);

    const byModel = buildUsageStats(records, {
      start,
      end,
      groupBy: "model",
      sessionsDir
    });

    expect(byModel.rows.map((row) => row.key)).toEqual(["gpt-5.5", "gpt-5.4"]);
    expect(formatUsageStats(byModel)).toContain("Codex usage");
    expect(formatUsageStats(byModel)).toContain("gpt-5.5");
    expect(formatUsageStats(byModel)).toContain("Credits");
    expect(formatUsageStats(byModel)).toContain("USD");
    expect(formatUsageStats(byModel, "csv")).toContain("Group,Sessions,Calls");
    expect(formatUsageStats(byModel, "markdown")).toContain("| Group | Sessions | Calls |");
    expect(toUsageStatsJson(byModel).rows[0]?.usd).toBeGreaterThan(0);
    expect(toUsageStatsJson(byModel).includeReasoningEffort).toBe(false);

    const byModelAndReasoningEffort = buildUsageStats(records, {
      start,
      end,
      groupBy: "model",
      includeReasoningEffort: true,
      sessionsDir
    });

    expect(byModelAndReasoningEffort.rows.map((row) => row.key)).toEqual([
      "gpt-5.5-xhigh",
      "gpt-5.5-high",
      "gpt-5.4"
    ]);
    expect(formatUsageStats(byModelAndReasoningEffort)).toContain("Grouped by: model + reasoning_effort");
    expect(toUsageStatsJson(byModelAndReasoningEffort).includeReasoningEffort).toBe(true);

    const byCwd = buildUsageStats(records, {
      start,
      end,
      groupBy: "cwd",
      sessionsDir
    });

    expect(byCwd.rows.map((row) => row.key)).toEqual(["/repo/alpha", "/repo/beta"]);

    const accountHistory = {
      defaultAccountId: "account-a",
      switches: [
        {
          timestamp: new Date("2026-05-11T10:30:00.000Z"),
          fromAccountId: "account-a",
          toAccountId: "account-b"
        }
      ]
    };
    const accountRecords = await readCodexUsageRecords({
      sessionsDir,
      start,
      end,
      accountHistory
    });
    const byAccount = buildUsageStats(accountRecords, {
      start,
      end,
      groupBy: "account",
      sessionsDir
    });
    const accountB = await readCodexUsageStats({
      start,
      end,
      groupBy: "account",
      sessionsDir,
      accountHistory,
      accountId: "account-b"
    });

    expect(accountRecords.map((record) => record.accountId)).toEqual([
      "account-a",
      "account-a",
      "account-b"
    ]);
    expect(byAccount.rows.map((row) => `${row.key}:${row.calls}`)).toEqual([
      "account-a:2",
      "account-b:1"
    ]);
    expect(accountB.rows.map((row) => row.key)).toEqual(["account-b"]);
    expect(accountB.totals.calls).toBe(1);
    expect(accountB.diagnostics?.skippedEvents.accountMismatch).toBe(2);
  });

  it("reads all usage records when --all disables date pruning", async () => {
    const sessionsDir = await createFixtureSessions();
    const options = resolveStatOptions(
      { all: true, groupBy: "month", sessionsDir },
      new Date("2026-05-12T12:34:56.000Z")
    );

    const records = await readCodexUsageRecords(options);
    const report = await readCodexUsageStats(options);

    expect(records).toHaveLength(6);
    expect(report.rows.map((row) => row.key)).toEqual(["2026-05"]);
    expect(report.totals.calls).toBe(6);
    expect(report.diagnostics).toMatchObject({
      skippedDirectories: 0,
      skippedFiles: 0,
      skippedEvents: {
        outOfRange: 0
      }
    });
    expect(formatUsageStats(report)).toContain("Range: all");
    expect(formatUsageStats(report)).not.toContain("Use -F, --full-scan");
    expect(toUsageStatsJson(report).start).toBe(options.start.toISOString());
    expect(toUsageStatsJson(report).warnings).toEqual([]);
  });

  it("can scan all JSONL files for long sessions while preserving event range filtering", async () => {
    const sessionsDir = await createLongSessionFixture();
    const start = new Date("2026-05-10T00:00:00.000Z");
    const end = new Date("2026-05-11T23:59:59.999Z");

    const defaultRecords = await readCodexUsageRecords({ sessionsDir, start, end });
    const defaultReport = await readCodexUsageStats({
      start,
      end,
      groupBy: "day",
      sessionsDir
    });

    expect(defaultRecords).toHaveLength(0);
    expect(defaultReport.diagnostics).toMatchObject({
      scanAllFiles: false,
      readFiles: 0,
      skippedFiles: 1,
      includedUsageEvents: 0
    });
    expect(formatUsageStats(defaultReport)).toContain("Use -F, --full-scan");

    const preciseRecords = await readCodexUsageRecords({
      sessionsDir,
      start,
      end,
      scanAllFiles: true
    });
    const preciseReport = await readCodexUsageStats({
      start,
      end,
      groupBy: "day",
      sessionsDir,
      scanAllFiles: true
    });

    expect(preciseRecords).toHaveLength(1);
    expect(preciseRecords[0]).toMatchObject({
      sessionId: "long-session",
      model: "gpt-5.5",
      cwd: "/repo/long",
      usage: { inputTokens: 100, cachedInputTokens: 10, outputTokens: 20, totalTokens: 120 }
    });
    expect(preciseReport.rows.map((row) => row.key)).toEqual(["2026-05-10"]);
    expect(preciseReport.totals.calls).toBe(1);
    expect(preciseReport.diagnostics).toMatchObject({
      scanAllFiles: true,
      readFiles: 1,
      skippedFiles: 0,
      includedUsageEvents: 1,
      skippedEvents: {
        outOfRange: 1
      }
    });
    expect(formatUsageStats(preciseReport)).not.toContain("Use -F, --full-scan");
  });

  it("balanced scanning checks only the bounded lookback before the requested range", async () => {
    const sessionsDir = await createBalancedScanFixture();
    const start = new Date("2026-05-10T00:00:00.000Z");
    const end = new Date("2026-05-10T23:59:59.999Z");

    const defaultReport = await readCodexUsageStats({
      start,
      end,
      groupBy: "day",
      sessionsDir
    });

    expect(defaultReport.rows.map((row) => `${row.key}:${row.calls}`)).toEqual([
      "2026-05-10:2"
    ]);
    expect(defaultReport.diagnostics).toMatchObject({
      scanAllFiles: false,
      readFiles: 2,
      skippedDirectories: 1,
      skippedFiles: 1,
      prefilteredFiles: 1,
      includedUsageEvents: 2
    });

    const fullScanReport = await readCodexUsageStats({
      start,
      end,
      groupBy: "day",
      sessionsDir,
      scanAllFiles: true
    });

    expect(fullScanReport.rows.map((row) => `${row.key}:${row.calls}`)).toEqual([
      "2026-05-10:3"
    ]);
    expect(fullScanReport.diagnostics).toMatchObject({
      scanAllFiles: true,
      readFiles: 3,
      skippedDirectories: 0,
      skippedFiles: 1,
      prefilteredFiles: 1,
      includedUsageEvents: 3
    });
  });

  it("warns on every balanced scan even when no files were skipped", async () => {
    const sessionsDir = await createSingleSessionFixture();
    const start = new Date("2026-05-10T00:00:00.000Z");
    const end = new Date("2026-05-10T23:59:59.999Z");
    const report = await readCodexUsageStats({
      start,
      end,
      groupBy: "day",
      sessionsDir
    });

    expect(report.diagnostics).toMatchObject({
      scanAllFiles: false,
      skippedDirectories: 0,
      skippedFiles: 0
    });
    expect(formatUsageStats(report)).toContain("Use -F, --full-scan");
    expect(formatUsageStats(report, "table", { verbose: true })).toContain(
      "Use -F, --full-scan"
    );
    expect(formatUsageStats(report, "markdown")).toContain("Use -F, --full-scan");
    const sessionsReport = await readCodexUsageSessions({ start, end, sessionsDir });
    expect(formatUsageSessions(sessionsReport)).toContain("Use -F, --full-scan");
    expect(toUsageStatsJson(report).warnings[0]).toContain("Use -F, --full-scan");

    const fullScanReport = await readCodexUsageStats({
      start,
      end,
      groupBy: "day",
      sessionsDir,
      scanAllFiles: true
    });

    expect(formatUsageStats(fullScanReport)).not.toContain("Use -F, --full-scan");
    expect(toUsageStatsJson(fullScanReport).warnings).toEqual([]);
  });

  it("prefilters full-scan files whose last usage is before the requested range", async () => {
    const sessionsDir = await createFullScanPrefilterFixture();
    const start = new Date("2026-05-10T00:00:00.000Z");
    const end = new Date("2026-05-10T23:59:59.999Z");
    const report = await readCodexUsageStats({
      start,
      end,
      groupBy: "day",
      sessionsDir,
      scanAllFiles: true
    });

    expect(report.rows.map((row) => row.key)).toEqual(["2026-05-10"]);
    expect(report.totals.calls).toBe(1);
    expect(report.diagnostics).toMatchObject({
      scanAllFiles: true,
      prefilteredFiles: 1,
      readFiles: 1,
      readLines: 2,
      includedUsageEvents: 1
    });
    expect(formatUsageStats(report, "table", { verbose: true })).toContain(
      "Files skipped by last-usage prefilter: 1"
    );
  });

  it("builds top session reports", async () => {
    const sessionsDir = await createFixtureSessions();
    const start = new Date("2026-05-10T00:00:00.000Z");
    const end = new Date("2026-05-11T23:59:59.999Z");
    const records = await readCodexUsageRecords({ sessionsDir, start, end });
    const report = buildUsageSessions(records, { start, end, sessionsDir }, 1);

    expect(report.rows).toHaveLength(1);
    expect(report.rows[0]?.sessionId).toBe("session-a");
    expect(report.rows[0]?.cwd).toBe("/repo/alpha");
    expect(report.totals.sessions).toBe(2);
    expect(formatUsageSessions(report)).toContain("Codex usage sessions");
    expect(formatUsageSessions(report, "json")).toContain("\"sessionId\": \"session-a\"");
    expect(formatUsageSessions(report, "csv")).toContain("Session,Model,CWD");

    const streamedReport = await readCodexUsageSessions({ start, end, sessionsDir }, 1);

    expect(streamedReport.rows).toEqual(report.rows);
    expect(streamedReport.totals).toEqual(report.totals);

    const latestReport = await readCodexUsageSessions(
      { start, end, sortBy: "time", sessionsDir },
      1
    );

    expect(latestReport.rows[0]?.sessionId).toBe("session-b");
  });

  it("builds session detail reports", async () => {
    const sessionsDir = await createFixtureSessions();
    const start = new Date("2026-05-10T00:00:00.000Z");
    const end = new Date("2026-05-11T23:59:59.999Z");
    const records = await readCodexUsageRecords({ sessionsDir, start, end });
    const report = buildUsageSessionDetail(records, { start, end, sessionsDir }, "session-a");

    expect(report.sessionId).toBe("session-a");
    expect(report.summary?.calls).toBe(2);
    expect(report.rows.map((row) => row.timestamp.toISOString())).toEqual([
      "2026-05-10T10:00:02.000Z",
      "2026-05-11T10:00:03.000Z"
    ]);
    expect(report.rows.map((row) => row.reasoningEffort)).toEqual(["high", "xhigh"]);
    expect(report.byModel.map((row) => row.key)).toEqual(["gpt-5.5"]);
    expect(report.byCwd.map((row) => row.key)).toEqual(["/repo/alpha"]);
    expect(report.byReasoningEffort.map((row) => row.key)).toEqual(["xhigh", "high"]);
    expect(report.modelSwitches).toBe(0);
    expect(report.cwdSwitches).toBe(0);
    expect(report.reasoningEffortSwitches).toBe(1);
    expect(report.totals.usage.totalTokens).toBe(38);
    expect(formatUsageSessionDetail(report)).toContain("Codex usage session detail");
    expect(formatUsageSessionDetail(report)).toContain("By model:");
    expect(formatUsageSessionDetail(report)).toContain("By cwd:");
    expect(formatUsageSessionDetail(report)).toContain("By reasoning effort:");
    expect(formatUsageSessionDetail(report, "json")).toContain("\"sessionId\": \"session-a\"");
    expect(formatUsageSessionDetail(report, "json")).toContain("\"byReasoningEffort\"");
    expect(formatUsageSessionDetail(report, "csv")).toContain("Range,Events,Model,Effort");
    expect(formatUsageSessionDetail(report, "csv", { detail: true })).toContain(
      "Time,Model,Effort,CWD"
    );

    const streamedReport = await readCodexUsageSessionDetail(
      { start, end, limit: 1, sessionsDir },
      "session-a"
    );

    expect(streamedReport.rows).toHaveLength(1);
    expect(streamedReport.totals.calls).toBe(2);
  });

  it("compacts long session detail tables while preserving model and effort runs", () => {
    const start = new Date("2026-05-10T00:00:00.000Z");
    const end = new Date("2026-05-10T23:59:59.999Z");
    const records = Array.from({ length: 30 }, (_, index) => ({
      timestamp: new Date(Date.UTC(2026, 4, 10, 10, index, 0)),
      sessionId: "session-long",
      model: index < 15 ? "gpt-5.5" : "gpt-5.4",
      reasoningEffort: index < 10 ? "high" : index < 20 ? "xhigh" : undefined,
      cwd: index < 18 ? "/repo/alpha" : "/repo/beta",
      filePath: "/tmp/session-long.jsonl",
      usage: {
        inputTokens: 10,
        cachedInputTokens: 1,
        outputTokens: 2,
        reasoningOutputTokens: 1,
        totalTokens: 12
      }
    }));
    const report = buildUsageSessionDetail(records, { start, end, sessionsDir: "/tmp/sessions" }, "session-long");
    const compactRows = buildUsageSessionCompactRows(report.rows);
    const manyChangeRows = buildUsageSessionCompactRows(
      records.map((record, index) => ({
        timestamp: record.timestamp,
        model: record.model,
        reasoningEffort: `effort-${index}`,
        cwd: record.cwd,
        usage: record.usage,
        credits: 0,
        usd: 0,
        priced: true,
        filePath: record.filePath
      }))
    );

    expect(compactRows.length).toBeLessThanOrEqual(20);
    expect(compactRows.map((row) => `${row.model}:${row.reasoningEffort ?? "unknown"}`)).toContain(
      "gpt-5.5:high"
    );
    expect(compactRows.map((row) => `${row.model}:${row.reasoningEffort ?? "unknown"}`)).toContain(
      "gpt-5.4:unknown"
    );
    expect(formatUsageSessionDetail(report)).toContain("Compact view:");
    expect(formatUsageSessionDetail(report)).toContain("Use --detail");
    expect(formatUsageSessionDetail(report, "table", { detail: true })).not.toContain(
      "Compact view:"
    );
    expect(manyChangeRows).toHaveLength(30);
  });

  it("reports unpriced models", () => {
    const start = new Date("2026-05-10T00:00:00.000Z");
    const end = new Date("2026-05-10T23:59:59.999Z");
    const report = buildUsageStats(
      [
        {
          timestamp: new Date("2026-05-10T10:00:00.000Z"),
          sessionId: "session-unpriced",
          model: "custom-model",
          cwd: "/repo/custom",
          filePath: "/tmp/session.jsonl",
          usage: {
            inputTokens: 100,
            cachedInputTokens: 0,
            outputTokens: 20,
            reasoningOutputTokens: 0,
            totalTokens: 120
          }
        }
      ],
      {
        start,
        end,
        groupBy: "model",
        sessionsDir: "/tmp/sessions"
      }
    );

    expect(report.totals.unpricedCalls).toBe(1);
    expect(report.unpricedModels).toMatchObject([
      { model: "custom-model", pricingKey: "custom-model", calls: 1, totalTokens: 120 }
    ]);
    expect(report.unpricedModels[0]?.pricingStub).toContain('"custom-model": {');
    expect(formatUsageStats(report)).toContain("custom-model: 1 calls, 120 tokens");
    expect(formatUsageStats(report)).toContain("Pricing stubs for src/pricing.ts");
  });
});

async function createFixtureSessions() {
  const root = await mkdtemp(join(tmpdir(), "codex-helper-stats-"));
  const sessionsDir = join(root, "sessions");
  const dayDir = join(sessionsDir, "2026", "05", "11");
  await mkdir(dayDir, { recursive: true });

  await writeFile(
    join(dayDir, "rollout-2026-05-11T10-00-00-session-a.jsonl"),
    [
      JSON.stringify({
        timestamp: "2026-05-11T10:00:00.000Z",
        type: "session_meta",
        payload: { id: "session-a", cwd: "/repo/alpha" }
      }),
      JSON.stringify({
        timestamp: "2026-05-11T10:00:01.000Z",
        type: "turn_context",
        payload: { model: "gpt-5.5", reasoning_effort: "high" }
      }),
      JSON.stringify({
        timestamp: "2026-05-10T10:00:02.000Z",
        type: "event_msg",
        payload: {
          type: "token_count",
          info: {
            last_token_usage: usage(10, 2, 3, 1, 13),
            total_token_usage: usage(10, 2, 3, 1, 13)
          }
        }
      }),
      JSON.stringify({
        timestamp: "2026-05-11T10:00:02.500Z",
        type: "turn_context",
        payload: {
          model: "gpt-5.5",
          collaboration_mode: { settings: { reasoning_effort: "xhigh" } }
        }
      }),
      JSON.stringify({
        timestamp: "2026-05-11T10:00:03.000Z",
        type: "event_msg",
        payload: {
          type: "token_count",
          info: {
            last_token_usage: usage(20, 1, 5, 2, 25),
            total_token_usage: usage(30, 3, 8, 3, 38)
          }
        }
      })
    ].join("\n")
  );

  await writeFile(
    join(dayDir, "rollout-2026-05-11T11-00-00-session-b.jsonl"),
    [
      JSON.stringify({
        timestamp: "2026-05-11T11:00:00.000Z",
        type: "session_meta",
        payload: { id: "session-b", model: "gpt-5.4", cwd: "/repo/beta" }
      }),
      JSON.stringify({
        timestamp: "2026-05-11T11:00:01.000Z",
        type: "event_msg",
        payload: {
          type: "token_count",
          info: {
            total_token_usage: usage(7, 0, 2, 0, 9)
          }
        }
      }),
      JSON.stringify({
        timestamp: "2026-05-12T11:00:02.000Z",
        type: "event_msg",
        payload: {
          type: "token_count",
          info: {
            last_token_usage: usage(100, 0, 20, 0, 120),
            total_token_usage: usage(107, 0, 22, 0, 129)
          }
        }
      })
    ].join("\n")
  );

  const prunedDayDir = join(sessionsDir, "2026", "05", "14");
  await mkdir(prunedDayDir, { recursive: true });
  await writeFile(
    join(prunedDayDir, "rollout-2026-05-14T10-00-00-pruned-session.jsonl"),
    [
      JSON.stringify({
        timestamp: "2026-05-14T10:00:00.000Z",
        type: "session_meta",
        payload: { id: "pruned-session", model: "gpt-5.4", cwd: "/repo/pruned" }
      }),
      JSON.stringify({
        timestamp: "2026-05-10T10:00:00.000Z",
        type: "event_msg",
        payload: {
          type: "token_count",
          info: {
            total_token_usage: usage(1000, 0, 1000, 0, 2000)
          }
        }
      })
    ].join("\n")
  );

  await writeFile(
    join(dayDir, "rollout-2026-05-14T10-00-00-filename-pruned-session.jsonl"),
    [
      JSON.stringify({
        timestamp: "2026-05-10T10:00:00.000Z",
        type: "session_meta",
        payload: { id: "filename-pruned-session", model: "gpt-5.4", cwd: "/repo/pruned" }
      }),
      JSON.stringify({
        timestamp: "2026-05-10T10:00:01.000Z",
        type: "event_msg",
        payload: {
          type: "token_count",
          info: {
            total_token_usage: usage(1000, 0, 1000, 0, 2000)
          }
        }
      })
    ].join("\n")
  );

  return sessionsDir;
}

async function createSingleSessionFixture() {
  const root = await mkdtemp(join(tmpdir(), "codex-helper-stats-single-"));
  const sessionsDir = join(root, "sessions");
  const dayDir = join(sessionsDir, "2026", "05", "10");
  await mkdir(dayDir, { recursive: true });

  await writeFile(
    join(dayDir, "rollout-2026-05-10T09-00-00-single-session.jsonl"),
    [
      JSON.stringify({
        timestamp: "2026-05-10T09:00:00.000Z",
        type: "session_meta",
        payload: { id: "single-session", model: "gpt-5.5", cwd: "/repo/single" }
      }),
      JSON.stringify({
        timestamp: "2026-05-10T09:00:01.000Z",
        type: "event_msg",
        payload: {
          type: "token_count",
          info: {
            last_token_usage: usage(10, 1, 2, 0, 12),
            total_token_usage: usage(10, 1, 2, 0, 12)
          }
        }
      })
    ].join("\n")
  );

  return sessionsDir;
}

async function createBalancedScanFixture() {
  const root = await mkdtemp(join(tmpdir(), "codex-helper-stats-balanced-"));
  const sessionsDir = join(root, "sessions");
  const inRangeDir = join(sessionsDir, "2026", "05", "10");
  const lookbackDir = join(sessionsDir, "2026", "05", "08");
  const olderDir = join(sessionsDir, "2026", "05", "07");
  const futureDir = join(sessionsDir, "2026", "05", "11");
  await mkdir(inRangeDir, { recursive: true });
  await mkdir(lookbackDir, { recursive: true });
  await mkdir(olderDir, { recursive: true });
  await mkdir(futureDir, { recursive: true });

  await writeFile(
    join(inRangeDir, "rollout-2026-05-10T09-00-00-in-range.jsonl"),
    [
      JSON.stringify({
        timestamp: "2026-05-10T09:00:00.000Z",
        type: "session_meta",
        payload: { id: "in-range", model: "gpt-5.5", cwd: "/repo/in-range" }
      }),
      tokenCountLine("2026-05-10T09:00:01.000Z", usage(10, 1, 2, 0, 12))
    ].join("\n")
  );

  await writeFile(
    join(lookbackDir, "rollout-2026-05-08T09-00-00-lookback-live.jsonl"),
    [
      JSON.stringify({
        timestamp: "2026-05-08T09:00:00.000Z",
        type: "session_meta",
        payload: { id: "lookback-live", model: "gpt-5.5", cwd: "/repo/lookback" }
      }),
      tokenCountLine("2026-05-09T09:00:00.000Z", usage(100, 0, 10, 0, 110)),
      tokenCountLine("2026-05-10T10:00:00.000Z", usage(120, 0, 12, 0, 132))
    ].join("\n")
  );

  await writeFile(
    join(lookbackDir, "rollout-2026-05-08T10-00-00-lookback-old.jsonl"),
    [
      JSON.stringify({
        timestamp: "2026-05-08T10:00:00.000Z",
        type: "session_meta",
        payload: { id: "lookback-old", model: "gpt-5.5", cwd: "/repo/lookback" }
      }),
      tokenCountLine("2026-05-09T10:00:00.000Z", usage(1000, 0, 100, 0, 1100))
    ].join("\n")
  );

  await writeFile(
    join(olderDir, "rollout-2026-05-07T09-00-00-older-live.jsonl"),
    [
      JSON.stringify({
        timestamp: "2026-05-07T09:00:00.000Z",
        type: "session_meta",
        payload: { id: "older-live", model: "gpt-5.5", cwd: "/repo/older" }
      }),
      tokenCountLine("2026-05-10T11:00:00.000Z", usage(130, 0, 13, 0, 143))
    ].join("\n")
  );

  await writeFile(
    join(futureDir, "rollout-2026-05-11T09-00-00-future.jsonl"),
    [
      JSON.stringify({
        timestamp: "2026-05-11T09:00:00.000Z",
        type: "session_meta",
        payload: { id: "future", model: "gpt-5.5", cwd: "/repo/future" }
      }),
      tokenCountLine("2026-05-11T09:00:01.000Z", usage(10000, 0, 1000, 0, 11000))
    ].join("\n")
  );

  return sessionsDir;
}

async function createLongSessionFixture() {
  const root = await mkdtemp(join(tmpdir(), "codex-helper-stats-long-"));
  const sessionsDir = join(root, "sessions");
  const uncategorizedDir = join(sessionsDir, "uncategorized");
  await mkdir(uncategorizedDir, { recursive: true });

  await writeFile(
    join(uncategorizedDir, "rollout-2026-05-01T09-00-00-long-session.jsonl"),
    [
      JSON.stringify({
        timestamp: "2026-05-01T09:00:00.000Z",
        type: "session_meta",
        payload: { id: "long-session", model: "gpt-5.5", cwd: "/repo/long" }
      }),
      JSON.stringify({
        timestamp: "2026-05-10T10:00:00.000Z",
        type: "event_msg",
        payload: {
          type: "token_count",
          info: {
            last_token_usage: usage(100, 10, 20, 5, 120),
            total_token_usage: usage(100, 10, 20, 5, 120)
          }
        }
      }),
      JSON.stringify({
        timestamp: "2026-05-13T10:00:00.000Z",
        type: "event_msg",
        payload: {
          type: "token_count",
          info: {
            last_token_usage: usage(1, 0, 1, 0, 2),
            total_token_usage: usage(101, 10, 21, 5, 122)
          }
        }
      })
    ].join("\n")
  );

  return sessionsDir;
}

async function createFullScanPrefilterFixture() {
  const root = await mkdtemp(join(tmpdir(), "codex-helper-stats-prefilter-"));
  const sessionsDir = join(root, "sessions");
  const dir = join(sessionsDir, "uncategorized");
  await mkdir(dir, { recursive: true });

  await writeFile(
    join(dir, "rollout-2026-05-01T09-00-00-old-session.jsonl"),
    [
      JSON.stringify({
        timestamp: "2026-05-01T09:00:00.000Z",
        type: "session_meta",
        payload: { id: "old-session", model: "gpt-5.5", cwd: "/repo/old" }
      }),
      JSON.stringify({
        timestamp: "2026-05-02T09:00:00.000Z",
        type: "event_msg",
        payload: {
          type: "token_count",
          info: {
            last_token_usage: usage(1000, 0, 1000, 0, 2000),
            total_token_usage: usage(1000, 0, 1000, 0, 2000)
          }
        }
      })
    ].join("\n")
  );

  await writeFile(
    join(dir, "rollout-2026-05-01T09-00-00-in-range-session.jsonl"),
    [
      JSON.stringify({
        timestamp: "2026-05-01T09:00:00.000Z",
        type: "session_meta",
        payload: { id: "in-range-session", model: "gpt-5.5", cwd: "/repo/in-range" }
      }),
      JSON.stringify({
        timestamp: "2026-05-10T09:00:00.000Z",
        type: "event_msg",
        payload: {
          type: "token_count",
          info: {
            last_token_usage: usage(100, 10, 20, 5, 120),
            total_token_usage: usage(100, 10, 20, 5, 120)
          }
        }
      })
    ].join("\n")
  );

  return sessionsDir;
}

function usage(
  input_tokens: number,
  cached_input_tokens: number,
  output_tokens: number,
  reasoning_output_tokens: number,
  total_tokens: number
) {
  return {
    input_tokens,
    cached_input_tokens,
    output_tokens,
    reasoning_output_tokens,
    total_tokens
  };
}

function tokenCountLine(timestamp: string, total_token_usage: ReturnType<typeof usage>) {
  return JSON.stringify({
    timestamp,
    type: "event_msg",
    payload: {
      type: "token_count",
      info: {
        total_token_usage
      }
    }
  });
}
