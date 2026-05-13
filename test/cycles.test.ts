import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { describe, expect, it } from "vitest";
import {
  addWeeklyCycleAnchor,
  addWeeklyCycleAnchorToFile,
  buildWeeklyCycleCurrentReport,
  buildWeeklyCycleDetailReport,
  buildWeeklyCycleHistoryReport,
  createEmptyWeeklyCycleStore,
  formatWeeklyCycleAnchorList,
  formatWeeklyCycleCurrent,
  formatWeeklyCycleDetail,
  formatWeeklyCycleHistory,
  listWeeklyCycleAnchors,
  listWeeklyCycleAnchorsFromFile,
  parseWeeklyCycleAnchorTime,
  readWeeklyCycleStore,
  removeWeeklyCycleAnchor,
  removeWeeklyCycleAnchorFromFile,
  resolveWeeklyCycleAccount,
  resolveWeeklyCycleStoreFile,
  toWeeklyCycleCurrentJson,
  toWeeklyCycleDetailJson,
  toWeeklyCycleHistoryJson,
  WEEKLY_CYCLE_PERIOD_HOURS
} from "../src/cycles.js";
import type { TokenUsage, UsageRecord } from "../src/stats.js";

describe("weekly cycle anchors", () => {
  it("resolves the cycle store file under the helper directory", () => {
    const codexHome = "/tmp/codex-home";

    expect(resolveWeeklyCycleStoreFile({ codexHome })).toBe(
      join(codexHome, "codex-helper", "stat-cycles.json")
    );
    expect(resolveWeeklyCycleStoreFile({ codexHome, cycleFile: "/tmp/custom-cycles.json" })).toBe(
      "/tmp/custom-cycles.json"
    );
    expect(resolveWeeklyCycleStoreFile({ codexHome, cycleFile: "custom-cycles.json" })).toBe(
      resolve("custom-cycles.json")
    );
  });

  it("parses offset and local anchor times without requiring UTC input", () => {
    const offsetWithSeconds = parseWeeklyCycleAnchorTime("2026-05-01T08:00:30+08:00");
    const offsetWithoutSeconds = parseWeeklyCycleAnchorTime("2026-05-01T08:00+08:00");
    const utc = parseWeeklyCycleAnchorTime("2026-05-01T00:00:00Z");
    const localWithSeconds = parseWeeklyCycleAnchorTime("2026-05-01 08:00:30");
    const localWithoutSeconds = parseWeeklyCycleAnchorTime("2026-05-01 08:00");
    const localDateOnly = parseWeeklyCycleAnchorTime("2026-05-01");

    expect(offsetWithSeconds).toMatchObject({
      atIso: "2026-05-01T00:00:30.000Z",
      input: "2026-05-01T08:00:30+08:00",
      timeZone: "UTC+08:00",
      hasExplicitOffset: true
    });
    expect(offsetWithoutSeconds.atIso).toBe("2026-05-01T00:00:00.000Z");
    expect(utc).toMatchObject({
      atIso: "2026-05-01T00:00:00.000Z",
      timeZone: "UTC",
      hasExplicitOffset: true
    });
    expect(localWithSeconds).toMatchObject({
      atIso: new Date(2026, 4, 1, 8, 0, 30).toISOString(),
      timeZone: localTimeZone(),
      hasExplicitOffset: false
    });
    expect(localWithoutSeconds.atIso).toBe(new Date(2026, 4, 1, 8, 0, 0).toISOString());
    expect(localDateOnly.atIso).toBe(new Date(2026, 4, 1, 0, 0, 0).toISOString());
    expect(() => parseWeeklyCycleAnchorTime("2026-02-31 08:00")).toThrow("Invalid local");
  });

  it("adds, sorts, and removes anchors by account", () => {
    const now = new Date("2026-05-13T01:50:00.000Z");
    const first = addWeeklyCycleAnchor(
      createEmptyWeeklyCycleStore(),
      {
        accountId: "account-a",
        at: "2026-05-02T08:00:00+08:00",
        note: "second"
      },
      now
    );
    const second = addWeeklyCycleAnchor(
      first.store,
      {
        accountId: "account-a",
        at: "2026-05-01 08:00",
        note: "first"
      },
      now
    );
    const anchors = listWeeklyCycleAnchors(second.store, "account-a");

    expect(anchors.map((anchor) => anchor.note)).toEqual(["first", "second"]);
    expect(anchors[0]).toMatchObject({
      id: anchorIdFor(new Date(2026, 4, 1, 8, 0, 0)),
      at: new Date(2026, 4, 1, 8, 0, 0).toISOString(),
      input: "2026-05-01 08:00",
      timeZone: localTimeZone(),
      source: "manual",
      note: "first",
      createdAt: now.toISOString()
    });

    expect(() =>
      addWeeklyCycleAnchor(second.store, {
        accountId: "account-a",
        at: "2026-05-02T00:00:00Z"
      })
    ).toThrow("already exists");

    const removed = removeWeeklyCycleAnchor(second.store, "account-a", anchors[0]?.id ?? "");

    expect(removed.removed.note).toBe("first");
    expect(listWeeklyCycleAnchors(removed.store, "account-a").map((anchor) => anchor.note)).toEqual([
      "second"
    ]);
    expect(() => removeWeeklyCycleAnchor(removed.store, "account-a", "missing")).toThrow(
      "No weekly cycle anchor"
    );
  });

  it("persists anchors and rejects malformed store JSON without overwriting it", async () => {
    const tempDir = await mkdtemp(join(tmpdir(), "codex-helper-cycles-"));
    const cycleFile = join(tempDir, "stat-cycles.json");
    const now = new Date("2026-05-13T01:50:00.000Z");

    try {
      expect(await readWeeklyCycleStore(cycleFile)).toEqual(createEmptyWeeklyCycleStore());

      const added = await addWeeklyCycleAnchorToFile(
        {
          cycleFile,
          accountId: "account-a",
          at: "2026-05-01T08:00:00+08:00",
          note: "initial weekly cycle"
        },
        now
      );
      const stored = JSON.parse(await readFile(cycleFile, "utf8")) as Record<string, unknown>;
      const listed = await listWeeklyCycleAnchorsFromFile({ cycleFile, accountId: "account-a" });

      expect(added).toMatchObject({
        cycleFile,
        accountId: "account-a",
        accountSource: "explicit"
      });
      expect(stored).toMatchObject({
        version: 1,
        accounts: {
          "account-a": {
            weekly: {
              periodHours: WEEKLY_CYCLE_PERIOD_HOURS,
              anchors: [
                {
                  id: "anc_20260501T000000000Z",
                  at: "2026-05-01T00:00:00.000Z",
                  input: "2026-05-01T08:00:00+08:00",
                  timeZone: "UTC+08:00",
                  source: "manual",
                  note: "initial weekly cycle",
                  createdAt: now.toISOString()
                }
              ]
            }
          }
        }
      });
      expect(listed.anchors).toHaveLength(1);

      const removed = await removeWeeklyCycleAnchorFromFile(added.anchor.id, {
        cycleFile,
        accountId: "account-a"
      });

      expect(removed.anchor.id).toBe(added.anchor.id);
      expect((await listWeeklyCycleAnchorsFromFile({ cycleFile, accountId: "account-a" })).anchors).toEqual(
        []
      );
      await expect(
        removeWeeklyCycleAnchorFromFile("missing", { cycleFile, accountId: "account-a" })
      ).rejects.toThrow("No weekly cycle anchor");

      await writeFile(cycleFile, "{not-json");
      await expect(readWeeklyCycleStore(cycleFile)).rejects.toThrow(`Failed to parse ${cycleFile}`);
      await expect(
        addWeeklyCycleAnchorToFile({ cycleFile, accountId: "account-a", at: "2026-05-01" }, now)
      ).rejects.toThrow(`Failed to parse ${cycleFile}`);
      expect(await readFile(cycleFile, "utf8")).toBe("{not-json");
    } finally {
      await rm(tempDir, { force: true, recursive: true });
    }
  });

  it("resolves account ownership from explicit, auth, token, or default sources", () => {
    expect(
      resolveWeeklyCycleAccount({
        accountId: " explicit-account ",
        authStatus: {
          summary: {
            chatgptAccountId: "chatgpt-account",
            tokenAccountId: "token-account"
          }
        }
      })
    ).toEqual({
      accountId: "explicit-account",
      source: "explicit",
      isDefault: false
    });
    expect(
      resolveWeeklyCycleAccount({
        authStatus: {
          summary: {
            chatgptAccountId: "chatgpt-account",
            tokenAccountId: "token-account"
          }
        }
      })
    ).toEqual({
      accountId: "chatgpt-account",
      source: "chatgpt_account_id",
      isDefault: false
    });
    expect(
      resolveWeeklyCycleAccount({
        authStatus: {
          summary: {
            tokenAccountId: "token-account"
          }
        }
      })
    ).toEqual({
      accountId: "token-account",
      source: "token_account_id",
      isDefault: false
    });
    expect(resolveWeeklyCycleAccount()).toEqual({
      accountId: "default",
      source: "default",
      isDefault: true
    });
    expect(() => resolveWeeklyCycleAccount({ accountId: "  " })).toThrow("cannot be empty");
  });

  it("derives delayed weekly cycles from usage after reset", () => {
    const anchors = [
      anchor("anchor-may-01", "2026-05-01T00:00:00.000Z", "2026-05-01T00:00:00Z")
    ];
    const records = [
      record("2026-05-01T01:00:00.000Z", { sessionId: "session-a", inputTokens: 100 }),
      record("2026-05-07T23:59:59.000Z", { sessionId: "session-a", outputTokens: 20 }),
      record("2026-05-09T08:00:00.000Z", { sessionId: "session-b", inputTokens: 50 })
    ];
    const history = buildWeeklyCycleHistoryReport({
      anchors,
      records,
      now: new Date("2026-05-10T00:00:00.000Z")
    });
    const current = buildWeeklyCycleCurrentReport({
      anchors,
      records,
      now: new Date("2026-05-10T00:00:00.000Z")
    });

    expect(history.status).toBe("ok");
    expect(history.rows.map((row) => row.source)).toEqual(["manual", "derived"]);
    expect(history.rows.map((row) => row.id)).toEqual([
      "anchor-may-01",
      "cyc_20260509T080000000Z"
    ]);
    expect(history.rows.map((row) => row.start.toISOString())).toEqual([
      "2026-05-01T00:00:00.000Z",
      "2026-05-09T08:00:00.000Z"
    ]);
    expect(history.rows.map((row) => row.resetAt.toISOString())).toEqual([
      "2026-05-08T00:00:00.000Z",
      "2026-05-16T08:00:00.000Z"
    ]);
    expect(history.rows.map((row) => row.calls)).toEqual([2, 1]);
    expect(history.rows[0]?.usage.totalTokens).toBe(120);
    expect(history.rows[1]?.usage.totalTokens).toBe(50);
    expect(history.totals).toMatchObject({
      sessions: 2,
      calls: 3,
      usage: { totalTokens: 170 },
      pricedCalls: 3,
      unpricedCalls: 0
    });
    expect(history.diagnostics).toMatchObject({
      anchors: 1,
      usageRecords: 3,
      windows: 2,
      derivedWindows: 1,
      includedUsageEvents: 3,
      unanchored: false
    });
    expect(current).toMatchObject({
      status: "active",
      current: {
        source: "derived",
        calls: 1,
        usage: { totalTokens: 50 }
      }
    });
    expect(current.current?.start.toISOString()).toBe("2026-05-09T08:00:00.000Z");
  });

  it("reports waiting_for_usage after reset when no later usage starts the next cycle", () => {
    const anchors = [
      anchor("anchor-may-01", "2026-05-01T00:00:00.000Z", "2026-05-01T00:00:00Z")
    ];
    const current = buildWeeklyCycleCurrentReport({
      anchors,
      records: [record("2026-05-01T01:00:00.000Z", { sessionId: "session-a", inputTokens: 100 })],
      now: new Date("2026-05-08T00:00:01.000Z")
    });

    expect(current.status).toBe("waiting_for_usage");
    expect(current.current?.source).toBe("manual");
    expect(current.current?.start.toISOString()).toBe("2026-05-01T00:00:00.000Z");
    expect(current.current?.resetAt.toISOString()).toBe("2026-05-08T00:00:00.000Z");
    expect(current.current?.calls).toBe(1);
  });

  it("uses later manual anchors as new calibration segments", () => {
    const anchors = [
      anchor("anchor-may-01", "2026-05-01T00:00:00.000Z", "2026-05-01T00:00:00Z"),
      anchor("anchor-may-10", "2026-05-10T00:00:00.000Z", "2026-05-10T00:00:00Z")
    ];
    const history = buildWeeklyCycleHistoryReport({
      anchors,
      records: [
        record("2026-05-01T01:00:00.000Z", { sessionId: "session-a", inputTokens: 10 }),
        record("2026-05-09T08:00:00.000Z", { sessionId: "session-b", inputTokens: 20 }),
        record("2026-05-10T01:00:00.000Z", { sessionId: "session-c", inputTokens: 30 })
      ],
      now: new Date("2026-05-11T00:00:00.000Z")
    });

    expect(history.rows.map((row) => row.source)).toEqual(["manual", "derived", "manual"]);
    expect(history.rows.map((row) => row.start.toISOString())).toEqual([
      "2026-05-01T00:00:00.000Z",
      "2026-05-09T08:00:00.000Z",
      "2026-05-10T00:00:00.000Z"
    ]);
    expect(history.rows.map((row) => row.calls)).toEqual([1, 1, 1]);
    expect(history.rows[1]?.usage.totalTokens).toBe(20);
    expect(history.rows[2]?.usage.totalTokens).toBe(30);
    expect(history.rows[2]?.anchorId).toBe("anchor-may-10");
  });

  it("keeps unpriced model breakdowns in cycle rows and totals", () => {
    const anchors = [
      anchor("anchor-may-01", "2026-05-01T00:00:00.000Z", "2026-05-01T00:00:00Z")
    ];
    const history = buildWeeklyCycleHistoryReport({
      anchors,
      records: [
        record("2026-05-01T01:00:00.000Z", {
          sessionId: "session-custom",
          model: "custom-model",
          inputTokens: 100,
          outputTokens: 20
        })
      ],
      now: new Date("2026-05-02T00:00:00.000Z")
    });

    expect(history.rows[0]).toMatchObject({
      pricedCalls: 0,
      unpricedCalls: 1,
      unpricedModels: [
        { model: "custom-model", pricingKey: "custom-model", calls: 1, totalTokens: 120 }
      ]
    });
    expect(history.rows[0]?.unpricedModels[0]?.pricingStub).toContain('"custom-model": {');
    expect(history.totals.unpricedModels).toHaveLength(1);
  });

  it("only includes pre-anchor estimated cycles when explicitly requested", () => {
    const anchors = [
      anchor("anchor-may-08", "2026-05-08T00:00:00.000Z", "2026-05-08T00:00:00Z")
    ];
    const records = [
      record("2026-04-30T12:00:00.000Z", { sessionId: "session-before", inputTokens: 10 }),
      record("2026-05-08T01:00:00.000Z", { sessionId: "session-after", inputTokens: 20 })
    ];
    const exact = buildWeeklyCycleHistoryReport({
      anchors,
      records,
      now: new Date("2026-05-09T00:00:00.000Z")
    });
    const estimated = buildWeeklyCycleHistoryReport({
      anchors,
      records,
      now: new Date("2026-05-09T00:00:00.000Z"),
      estimateBeforeAnchor: true
    });

    expect(exact.rows.map((row) => row.source)).toEqual(["manual"]);
    expect(exact.totals.calls).toBe(1);
    expect(exact.diagnostics.ignoredBeforeAnchorEvents).toBe(1);
    expect(estimated.rows.map((row) => row.source)).toEqual(["estimated", "manual"]);
    expect(estimated.rows.map((row) => row.id)).toEqual([
      "est_20260424T000000000Z",
      "anchor-may-08"
    ]);
    expect(estimated.rows[0]?.start.toISOString()).toBe("2026-04-24T00:00:00.000Z");
    expect(estimated.rows[0]?.resetAt.toISOString()).toBe("2026-05-01T00:00:00.000Z");
    expect(estimated.rows[0]?.calls).toBe(1);
    expect(estimated.totals.calls).toBe(2);
    expect(estimated.diagnostics).toMatchObject({
      estimateBeforeAnchor: true,
      estimatedWindows: 1,
      ignoredBeforeAnchorEvents: 0
    });
  });

  it("returns unanchored reports when no anchor is available", () => {
    const records = [record("2026-05-01T01:00:00.000Z", { inputTokens: 100 })];

    expect(
      buildWeeklyCycleHistoryReport({
        anchors: [],
        records,
        now: new Date("2026-05-02T00:00:00.000Z")
      })
    ).toMatchObject({
      status: "unanchored",
      rows: [],
      totals: { calls: 0 },
      diagnostics: { unanchored: true, usageRecords: 1 }
    });
    const current = buildWeeklyCycleCurrentReport({
      anchors: [],
      records,
      now: new Date("2026-05-02T00:00:00.000Z")
    });

    expect(current).toMatchObject({
      status: "unanchored",
      totals: { calls: 0 },
      diagnostics: { unanchored: true, usageRecords: 1 }
    });
    expect(current.current).toBeUndefined();
  });

  it("formats anchor lists as table, JSON, CSV, and Markdown", () => {
    const anchorRow = anchor("anchor-may-01", "2026-05-01T00:00:00.000Z", "2026-05-01T00:00:00Z");
    const report = {
      cycleFile: "/tmp/stat-cycles.json",
      accountId: "account-a",
      accountSource: "explicit" as const,
      anchors: [anchorRow],
      store: createEmptyWeeklyCycleStore()
    };

    const table = formatWeeklyCycleAnchorList(report);
    const json = JSON.parse(formatWeeklyCycleAnchorList(report, "json")) as Record<string, unknown>;
    const csv = formatWeeklyCycleAnchorList(report, "csv");
    const markdown = formatWeeklyCycleAnchorList(report, "markdown");

    expect(table).toContain("Codex weekly cycle anchors");
    expect(table).toContain("Account: account-a (explicit)");
    expect(table).toContain("anchor-may-01");
    expect(table).toContain("2026-05-01");
    expect(json).toMatchObject({
      accountId: "account-a",
      accountSource: "explicit",
      cycleFile: "/tmp/stat-cycles.json",
      periodHours: WEEKLY_CYCLE_PERIOD_HOURS,
      anchors: [{ id: "anchor-may-01", at: "2026-05-01T00:00:00.000Z" }]
    });
    expect(csv).toContain("Account,ID,Local time,UTC at,Source,Note,Created at");
    expect(csv).toContain("account-a,anchor-may-01");
    expect(markdown).toContain("| Account | ID | Local time | UTC at | Source | Note | Created at |");
    expect(markdown).toContain("| account-a | anchor-may-01 |");
  });

  it("formats current cycle reports with table text and ISO JSON", () => {
    const anchors = [
      anchor("anchor-may-01", "2026-05-01T00:00:00.000Z", "2026-05-01T00:00:00Z")
    ];
    const report = buildWeeklyCycleCurrentReport({
      anchors,
      records: [
        record("2026-05-09T08:00:00.000Z", {
          sessionId: "session-b",
          model: "gpt-5.5",
          inputTokens: 50
        }),
        record("2026-05-10T09:00:00.000Z", {
          sessionId: "session-c",
          model: "gpt-5.4",
          inputTokens: 30
        })
      ],
      now: new Date("2026-05-10T00:00:00.000Z")
    });
    const context = {
      accountId: "account-a",
      accountSource: "chatgpt_account_id" as const,
      cycleFile: "/tmp/stat-cycles.json"
    };
    const table = formatWeeklyCycleCurrent(report, "table", context);
    const json = toWeeklyCycleCurrentJson(report, context);

    expect(table).toContain("Codex weekly cycle current");
    expect(table).toContain("Status: active");
    expect(table).toContain("Account: account-a (chatgpt_account_id)");
    expect(table).toContain("Summary:");
    expect(table).toContain("By day:");
    expect(table).toContain("By model:");
    expect(table).toContain("derived");
    expect(table).toContain("50");
    expect(table).toContain("2026-05-09");
    expect(table).toContain("gpt-5.5");
    expect(table).not.toContain("gpt-5.4");
    expect(table).toContain("Diagnostics:");
    expect(json).toMatchObject({
      accountId: "account-a",
      accountSource: "chatgpt_account_id",
      cycleFile: "/tmp/stat-cycles.json",
      status: "active",
      periodHours: WEEKLY_CYCLE_PERIOD_HOURS,
      now: "2026-05-10T00:00:00.000Z",
      current: {
        start: "2026-05-09T08:00:00.000Z",
        resetAt: "2026-05-16T08:00:00.000Z",
        source: "derived",
        calls: 1,
        usage: { totalTokens: 50 }
      },
      byDay: [
        {
          key: "2026-05-09",
          calls: 1,
          usage: { totalTokens: 50 }
        }
      ],
      byModel: [
        {
          key: "gpt-5.5",
          calls: 1,
          usage: { totalTokens: 50 }
        }
      ],
      diagnostics: {
        anchors: 1,
        windows: 1
      }
    });
  });

  it("formats a selected history cycle with current-style breakdown details", () => {
    const anchors = [
      anchor("anchor-may-01", "2026-05-01T00:00:00.000Z", "2026-05-01T00:00:00Z")
    ];
    const records = [
      record("2026-05-01T01:00:00.000Z", {
        sessionId: "session-a",
        model: "gpt-5.5",
        inputTokens: 100
      }),
      record("2026-05-09T08:00:00.000Z", {
        sessionId: "session-b",
        model: "gpt-5.5",
        inputTokens: 50
      }),
      record("2026-05-10T09:00:00.000Z", {
        sessionId: "session-c",
        model: "gpt-5.4",
        inputTokens: 30
      })
    ];
    const history = buildWeeklyCycleHistoryReport({
      anchors,
      records,
      now: new Date("2026-05-11T00:00:00.000Z")
    });
    const detail = buildWeeklyCycleDetailReport({
      history,
      cycleId: "cyc_20260509T080000000Z",
      records
    });
    const context = {
      accountId: "account-a",
      accountSource: "explicit" as const,
      cycleFile: "/tmp/stat-cycles.json"
    };
    const table = formatWeeklyCycleDetail(detail, "table", context);
    const json = toWeeklyCycleDetailJson(detail, context);

    expect(table).toContain("Codex weekly cycle detail");
    expect(table).toContain("Cycle ID: cyc_20260509T080000000Z");
    expect(table).toContain("Summary:");
    expect(table).toContain("By day:");
    expect(table).toContain("By model:");
    expect(table).toContain("gpt-5.5");
    expect(table).toContain("gpt-5.4");
    expect(json).toMatchObject({
      accountId: "account-a",
      cycleId: "cyc_20260509T080000000Z",
      cycle: {
        id: "cyc_20260509T080000000Z",
        source: "derived",
        calls: 2,
        usage: { totalTokens: 80 }
      },
      byDay: [
        { key: "2026-05-09", calls: 1, usage: { totalTokens: 50 } },
        { key: "2026-05-10", calls: 1, usage: { totalTokens: 30 } }
      ],
      byModel: [
        { key: "gpt-5.4", calls: 1, usage: { totalTokens: 30 } },
        { key: "gpt-5.5", calls: 1, usage: { totalTokens: 50 } }
      ]
    });
    expect(() =>
      buildWeeklyCycleDetailReport({
        history,
        cycleId: "missing",
        records
      })
    ).toThrow("No weekly cycle found for id: missing");
  });

  it("formats history reports as table, JSON, CSV, and Markdown", () => {
    const anchors = [
      anchor("anchor-may-01", "2026-05-01T00:00:00.000Z", "2026-05-01T00:00:00Z")
    ];
    const report = buildWeeklyCycleHistoryReport({
      anchors,
      records: [
        record("2026-05-01T01:00:00.000Z", { sessionId: "session-a", inputTokens: 100 }),
        record("2026-05-09T08:00:00.000Z", { sessionId: "session-b", inputTokens: 50 })
      ],
      now: new Date("2026-05-10T00:00:00.000Z")
    });
    const context = {
      accountId: "account-a",
      accountSource: "explicit" as const,
      cycleFile: "/tmp/stat-cycles.json"
    };
    const table = formatWeeklyCycleHistory(report, "table", context);
    const json = toWeeklyCycleHistoryJson(report, context);
    const csv = formatWeeklyCycleHistory(report, "csv", context);
    const markdown = formatWeeklyCycleHistory(report, "markdown", context);

    expect(table).toContain("Codex weekly cycle history");
    expect(table).toContain("manual");
    expect(table).toContain("derived");
    expect(table).toContain("Total");
    expect(json).toMatchObject({
      accountId: "account-a",
      accountSource: "explicit",
      cycleFile: "/tmp/stat-cycles.json",
      status: "ok",
      periodHours: WEEKLY_CYCLE_PERIOD_HOURS,
      end: "2026-05-10T00:00:00.000Z",
      rows: [
        { id: "anchor-may-01", start: "2026-05-01T00:00:00.000Z", source: "manual" },
        { id: "cyc_20260509T080000000Z", start: "2026-05-09T08:00:00.000Z", source: "derived" }
      ],
      totals: {
        calls: 2,
        usage: { totalTokens: 150 }
      },
      diagnostics: {
        includedUsageEvents: 2
      }
    });
    expect(csv).toContain("ID,Start,Reset at,Source,Sessions,Calls,Input,Cached,Output,Reasoning,Total,Credits,USD");
    expect(csv).toContain("manual");
    expect(markdown).toContain("| ID | Start | Reset at | Source | Sessions | Calls |");
    expect(markdown).toContain("| Total |");
  });
});

function localTimeZone() {
  return Intl.DateTimeFormat().resolvedOptions().timeZone ?? "local";
}

function anchorIdFor(date: Date) {
  return `anc_${date
    .toISOString()
    .replace(/[-:]/g, "")
    .replace(".", "")}`;
}

function anchor(id: string, at: string, input: string) {
  return {
    id,
    at,
    input,
    timeZone: "UTC",
    source: "manual" as const,
    note: "",
    createdAt: "2026-05-13T01:50:00.000Z"
  };
}

function record(
  timestamp: string,
  options: {
    sessionId?: string;
    model?: string;
    inputTokens?: number;
    cachedInputTokens?: number;
    outputTokens?: number;
    reasoningOutputTokens?: number;
    totalTokens?: number;
  } = {}
): UsageRecord {
  const usage = tokenUsage(options);

  return {
    timestamp: new Date(timestamp),
    sessionId: options.sessionId ?? "session-a",
    model: options.model ?? "gpt-5.5",
    cwd: "/repo",
    filePath: "/tmp/session.jsonl",
    usage
  };
}

function tokenUsage(options: {
  inputTokens?: number;
  cachedInputTokens?: number;
  outputTokens?: number;
  reasoningOutputTokens?: number;
  totalTokens?: number;
}): TokenUsage {
  const inputTokens = options.inputTokens ?? 0;
  const cachedInputTokens = options.cachedInputTokens ?? 0;
  const outputTokens = options.outputTokens ?? 0;
  const reasoningOutputTokens = options.reasoningOutputTokens ?? 0;

  return {
    inputTokens,
    cachedInputTokens,
    outputTokens,
    reasoningOutputTokens,
    totalTokens: options.totalTokens ?? inputTokens + outputTokens
  };
}
