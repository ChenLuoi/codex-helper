import { mkdir, mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import {
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
    const lastTwoDays = resolveStatOptions({ last: "2d", sessionsDir: "/tmp/sessions" }, now);

    expect(today.start).toEqual(new Date("2026-05-11T16:00:00.000Z"));
    expect(today.groupBy).toBe("hour");
    expect(month.start).toEqual(new Date("2026-04-30T16:00:00.000Z"));
    expect(month.groupBy).toBe("day");
    expect(lastTwoDays.start).toEqual(new Date("2026-05-10T12:34:56.000Z"));
    expect(lastTwoDays.groupBy).toBe("hour");
    expect(resolveStatOptions({ format: "csv", sessionsDir: "/tmp/sessions" }, now).format).toBe(
      "csv"
    );
    expect(resolveStatOptions({ json: true, sessionsDir: "/tmp/sessions" }, now).format).toBe(
      "json"
    );
    expect(() =>
      resolveStatOptions({ today: true, last: "7d", sessionsDir: "/tmp/sessions" }, now)
    ).toThrow("Use only one quick range");
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

    const byCwd = buildUsageStats(records, {
      start,
      end,
      groupBy: "cwd",
      sessionsDir
    });

    expect(byCwd.rows.map((row) => row.key)).toEqual(["/repo/alpha", "/repo/beta"]);
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
    expect(report.totals.usage.totalTokens).toBe(38);
    expect(formatUsageSessionDetail(report)).toContain("Codex usage session detail");
    expect(formatUsageSessionDetail(report, "json")).toContain("\"sessionId\": \"session-a\"");
    expect(formatUsageSessionDetail(report, "csv")).toContain("Time,Model,CWD");

    const streamedReport = await readCodexUsageSessionDetail(
      { start, end, limit: 1, sessionsDir },
      "session-a"
    );

    expect(streamedReport.rows).toHaveLength(1);
    expect(streamedReport.totals.calls).toBe(2);
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
        payload: { model: "gpt-5.5" }
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
