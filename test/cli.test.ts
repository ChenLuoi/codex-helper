import { mkdir, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Writable } from "node:stream";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { createProgram, isMainModule } from "../src/cli.js";

class MemoryStream extends Writable {
  chunks: string[] = [];

  _write(
    chunk: Buffer | string,
    _encoding: BufferEncoding,
    callback: (error?: Error | null) => void
  ) {
    this.chunks.push(chunk.toString());
    callback();
  }

  toString() {
    return this.chunks.join("");
  }
}

describe("cli", () => {
  it("prints help text", async () => {
    const output = new MemoryStream();
    const program = createProgram({ output });

    program.exitOverride();
    program.configureOutput({
      writeOut: (text) => output.write(text),
      writeErr: (text) => output.write(text)
    });

    try {
      await program.parseAsync(["node", "codex-helper", "--help"]);
    } catch {
      // Commander throws after printing help when exitOverride is enabled.
    }

    expect(output.toString()).toContain("codex-helper");
    expect(output.toString()).toContain("auth");
    expect(output.toString()).toContain("doctor");
    expect(output.toString()).not.toContain("init");
    expect(output.toString()).toContain("stat");
    expect(output.toString()).toContain("cycle");
  });

  it("prints auth status help text", async () => {
    const program = createProgram();
    const authCommand = program.commands.find((command) => command.name() === "auth");
    const authHelpText = authCommand?.helpInformation() ?? "";
    const statusCommand = authCommand?.commands.find((command) => command.name() === "status");
    const helpText = statusCommand?.helpInformation() ?? "";

    expect(authHelpText).toContain("status");
    expect(authHelpText).toContain("save");
    expect(authHelpText).toContain("list");
    expect(authHelpText).toContain("select");
    expect(authHelpText).toContain("remove");
    expect(helpText).toContain("auth status");
    expect(helpText).toContain("--auth-file");
    expect(helpText).toContain("--codex-home");
    expect(helpText).toContain("--json");
    expect(helpText).toContain("--include-token-claims");
  });

  it("prints auth status from an auth.json ID token", async () => {
    const tempDir = await mkdtemp(join(tmpdir(), "codex-helper-auth-"));
    const authFile = join(tempDir, "auth.json");
    const idToken = createJwt({
      sub: "user_123",
      iss: "https://auth.example.test",
      name: "Example User",
      email: "user@example.test",
      exp: timestamp("2026-05-13T00:00:00.000Z"),
      "https://api.openai.com/auth": {
        chatgpt_account_id: "account_123",
        user_id: "user_123",
        chatgpt_plan_type: "pro"
      }
    });
    const output = new MemoryStream();
    const program = createProgram({ output });

    try {
      await writeFile(
        authFile,
        JSON.stringify({
          auth_mode: "chatgpt",
          tokens: {
            id_token: idToken,
            refresh_token: "not-a-jwt"
          }
        })
      );

      await program.parseAsync(["node", "codex-helper", "auth", "status", "--auth-file", authFile]);

      expect(output.toString()).toContain("Codex auth");
      expect(output.toString()).toContain("Account ID: account_123");
      expect(output.toString()).toContain("Name: Example User");
      expect(output.toString()).toContain("Email: user@example.test");
      expect(output.toString()).toContain("User ID: user_123");
      expect(output.toString()).toContain("Plan: pro");
      expect(output.toString()).not.toContain("Token: id_token");
      expect(output.toString()).not.toContain("Issuer: https://auth.example.test");
      expect(output.toString()).not.toContain(idToken);
    } finally {
      await rm(tempDir, { force: true, recursive: true });
    }
  });

  it("selects a persisted auth profile by account id", async () => {
    const tempDir = await mkdtemp(join(tmpdir(), "codex-helper-auth-select-"));
    const authFile = join(tempDir, "auth.json");
    const codexHome = join(tempDir, "codex-home");
    const storeDir = join(tempDir, "auth-profiles");
    const accountHistoryFile = join(codexHome, "codex-helper", "auth-account-history.json");
    const currentContent = createAuthContent("account-a", "User A", "a@example.test", "plus");
    const selectedContent = createAuthContent("account-b", "User B", "b@example.test", "pro");
    const output = new MemoryStream();
    const program = createProgram({ output });

    try {
      await mkdir(storeDir, { recursive: true });
      await writeFile(authFile, currentContent);
      await writeFile(join(storeDir, "account-b.json"), selectedContent);

      await program.parseAsync([
        "node",
        "codex-helper",
        "auth",
        "select",
        "--auth-file",
        authFile,
        "--store-dir",
        storeDir,
        "--codex-home",
        codexHome,
        "--account-id",
        "account-b"
      ]);

      expect(output.toString()).toContain(
        "Saved current auth profile: a@example.test(account-a) - plus"
      );
      expect(output.toString()).toContain(
        "Activated auth profile: b@example.test(account-b) - pro"
      );
      expect(await readFile(authFile, "utf8")).toBe(selectedContent);
      expect(await readFile(join(storeDir, "account-a.json"), "utf8")).toBe(currentContent);
      expect(await readFile(accountHistoryFile, "utf8")).toContain(
        "\"toAccountId\": \"account-b\""
      );
    } finally {
      await rm(tempDir, { force: true, recursive: true });
    }
  });

  it("prints stat help text", async () => {
    const program = createProgram();
    const statCommand = program.commands.find((command) => command.name() === "stat");
    const helpText = statCommand?.helpInformation() ?? "";

    expect(helpText).toContain("stat [options] [view] [session]");
    expect(helpText).toContain("--format");
    expect(helpText).toContain("--today");
    expect(helpText).toContain("hour, day, week, month");
    expect(helpText).toContain("--sort");
    expect(helpText).toContain("--limit");
    expect(helpText).toContain("--detail");
    expect(helpText).toContain("--full-scan");
    expect(helpText).toContain("--all");
    expect(helpText).toContain("--reasoning-effort");
    expect(helpText).toContain("--account-id");
    expect(helpText).toContain("--verbose");
    expect(helpText).toContain("--top");
    expect(helpText).not.toContain("cycle");
  });

  it("prints cycle help text", async () => {
    const program = createProgram();
    const cycleCommand = program.commands.find((command) => command.name() === "cycle");
    const cycleHelpText = cycleCommand?.helpInformation() ?? "";
    const addCommand = cycleCommand?.commands.find((command) => command.name() === "add");
    const addHelpText = addCommand?.helpInformation() ?? "";

    expect(cycleHelpText).toContain("add");
    expect(cycleHelpText).toContain("list");
    expect(cycleHelpText).toContain("remove");
    expect(cycleHelpText).toContain("current");
    expect(cycleHelpText).toContain("history");
    expect(addHelpText).toContain("cycle add");
    expect(addHelpText).toContain("<time...>");
    expect(addHelpText).not.toContain("--at");

    const historyCommand = cycleCommand?.commands.find((command) => command.name() === "history");
    const historyHelpText = historyCommand?.helpInformation() ?? "";
    expect(historyHelpText).toContain("history [options] [cycle-id]");
    expect(historyHelpText).toContain("--select");
  });

  it("adds and lists weekly cycle anchors through the cycle CLI", async () => {
    const tempDir = await mkdtemp(join(tmpdir(), "codex-helper-cycle-cli-"));
    const cycleFile = join(tempDir, "stat-cycles.json");

    try {
      const added = await runCli([
        "cycle",
        "add",
        "2026-05-01T08:00:00+08:00",
        "2026-05-08T08:00:00+08:00",
        "--cycle-file",
        cycleFile,
        "--account-id",
        "account-a",
        "--note",
        "initial weekly cycle"
      ]);

      expect(added).toContain("Added 2 weekly cycle anchors:");
      expect(added).toContain("anc_20260501T000000000Z");
      expect(added).toContain("anc_20260508T000000000Z");
      expect(added).toContain("Account: account-a");
      expect(added).toContain(`Cycle file: ${cycleFile}`);

      const listed = await runCli([
        "cycle",
        "list",
        "--cycle-file",
        cycleFile,
        "--account-id",
        "account-a"
      ]);

      expect(listed).toContain("Codex weekly cycle anchors");
      expect(listed).toContain("account-a");
      expect(listed).toContain("anc_20260501T000000000Z");
      expect(listed).toContain("anc_20260508T000000000Z");
      expect(listed).toContain("initial weekly cycle");
    } finally {
      await rm(tempDir, { force: true, recursive: true });
    }
  });

  it("adds multiple weekly cycle anchors from unquoted date and time pairs", async () => {
    const tempDir = await mkdtemp(join(tmpdir(), "codex-helper-cycle-cli-pairs-"));
    const cycleFile = join(tempDir, "stat-cycles.json");

    try {
      const added = await runCli([
        "cycle",
        "add",
        "2026-05-01",
        "08:00",
        "2026-05-08",
        "08:00",
        "--cycle-file",
        cycleFile,
        "--account-id",
        "account-a"
      ]);
      const listed = JSON.parse(
        await runCli([
          "cycle",
          "list",
          "--cycle-file",
          cycleFile,
          "--account-id",
          "account-a",
          "--json"
        ])
      ) as { anchors: Array<{ input: string }> };

      expect(added).toContain("Added 2 weekly cycle anchors:");
      expect(listed.anchors.map((anchor) => anchor.input)).toEqual([
        "2026-05-01 08:00",
        "2026-05-08 08:00"
      ]);
    } finally {
      await rm(tempDir, { force: true, recursive: true });
    }
  });

  it("prints a clear current-cycle message when no anchor is configured", async () => {
    const tempDir = await mkdtemp(join(tmpdir(), "codex-helper-cycle-current-"));
    const cycleFile = join(tempDir, "stat-cycles.json");
    const authFile = join(tempDir, "auth.json");

    try {
      await writeFile(authFile, createAuthContent("account-a", "User A", "a@example.test", "plus"));

      const output = await runCli([
        "cycle",
        "current",
        "--cycle-file",
        cycleFile,
        "--auth-file",
        authFile
      ]);

      expect(output).toContain("Codex weekly cycle current");
      expect(output).toContain("Status: unanchored");
      expect(output).toContain("Account: a@example.test(account-a)");
      expect(output).toContain("No weekly cycle anchors configured.");
    } finally {
      await rm(tempDir, { force: true, recursive: true });
    }
  });

  it("prints weekly cycle history from local sessions with scan diagnostics", async () => {
    const tempDir = await mkdtemp(join(tmpdir(), "codex-helper-cycle-history-"));
    const cycleFile = join(tempDir, "stat-cycles.json");
    const sessionsDir = join(tempDir, "sessions");
    const authFile = join(tempDir, "auth.json");

    try {
      await writeFile(authFile, createAuthContent("account-a", "User A", "a@example.test", "plus"));
      await writeCycleSession(sessionsDir, "rollout-2026-05-01T00-00-00-cycle-session.jsonl", [
        { timestamp: "2026-05-01T01:00:00.000Z", inputTokens: 100 },
        { timestamp: "2026-05-09T08:00:00.000Z", inputTokens: 50 }
      ]);
      await runCli([
        "cycle",
        "add",
        "2026-05-01T00:00:00Z",
        "--cycle-file",
        cycleFile,
        "--account-id",
        "account-a"
      ]);

      const output = await runCli([
        "cycle",
        "history",
        "--cycle-file",
        cycleFile,
        "--account-id",
        "account-a",
        "--auth-file",
        authFile,
        "--sessions-dir",
        sessionsDir,
        "--start",
        "2026-05-01T00:00:00Z",
        "--end",
        "2026-05-10T00:00:00Z",
        "--json"
      ]);
      const json = JSON.parse(output) as {
        accountLabel?: string;
        rows: Array<{ id: string; start: string; source: string; calls: number }>;
        totals: { calls: number; usage: { totalTokens: number } };
        diagnostics: { usageDiagnostics?: { scanAllFiles: boolean; includedUsageEvents: number } };
      };

      expect(json.accountLabel).toBe("a@example.test(account-a)");
      expect(json.rows.map((row) => row.id)).toEqual([
        "anc_20260501T000000000Z",
        "cyc_20260509T080000000Z"
      ]);
      expect(json.rows.map((row) => row.source)).toEqual(["manual", "derived"]);
      expect(json.rows.map((row) => row.calls)).toEqual([1, 1]);
      expect(json.rows[1]?.start).toBe("2026-05-09T08:00:00.000Z");
      expect(json.totals).toMatchObject({
        calls: 2,
        usage: { totalTokens: 150 }
      });
      expect(json.diagnostics.usageDiagnostics).toMatchObject({
        scanAllFiles: true,
        includedUsageEvents: 2
      });

      const detail = await runCli([
        "cycle",
        "history",
        "cyc_20260509T080000000Z",
        "--cycle-file",
        cycleFile,
        "--account-id",
        "account-a",
        "--auth-file",
        authFile,
        "--sessions-dir",
        sessionsDir,
        "--start",
        "2026-05-01T00:00:00Z",
        "--end",
        "2026-05-10T00:00:00Z"
      ]);

      expect(detail).toContain("Codex weekly cycle detail");
      expect(detail).toContain("Account: a@example.test(account-a)");
      expect(detail).toContain("Cycle ID: cyc_20260509T080000000Z");
      expect(detail).toContain("By day:");
      expect(detail).toContain("By model:");
    } finally {
      await rm(tempDir, { force: true, recursive: true });
    }
  });

  it("uses full scan automatically for a specific session detail", async () => {
    const tempDir = await mkdtemp(join(tmpdir(), "codex-helper-stat-detail-full-scan-"));
    const sessionsDir = join(tempDir, "sessions");

    try {
      await writeCycleSession(sessionsDir, "rollout-2026-05-01T00-00-00-long-session.jsonl", [
        { timestamp: "2026-05-10T12:00:00.000Z", inputTokens: 100 }
      ]);

      const output = await runCli([
        "stat",
        "sessions",
        "cycle-session",
        "--sessions-dir",
        sessionsDir,
        "--start",
        "2026-05-10T00:00:00Z",
        "--end",
        "2026-05-10T23:59:59Z",
        "--json"
      ]);
      const json = JSON.parse(output) as {
        totals: { calls: number };
        diagnostics: { scanAllFiles: boolean; readFiles: number };
      };

      expect(json.totals.calls).toBe(1);
      expect(json.diagnostics).toMatchObject({
        scanAllFiles: true,
        readFiles: 1
      });
    } finally {
      await rm(tempDir, { force: true, recursive: true });
    }
  });

  it("defaults weekly cycle history to all usage instead of the last seven days", async () => {
    const tempDir = await mkdtemp(join(tmpdir(), "codex-helper-cycle-history-default-all-"));
    const cycleFile = join(tempDir, "stat-cycles.json");
    const sessionsDir = join(tempDir, "sessions");

    try {
      await writeCycleSession(sessionsDir, "rollout-2026-05-01T00-00-00-cycle-session.jsonl", [
        { timestamp: "2026-05-01T01:00:00.000Z", inputTokens: 100 },
        { timestamp: "2026-05-20T08:00:00.000Z", inputTokens: 50 }
      ]);
      await runCli([
        "cycle",
        "add",
        "2026-05-01T00:00:00Z",
        "--cycle-file",
        cycleFile,
        "--account-id",
        "account-a"
      ]);

      const output = await runCli([
        "cycle",
        "history",
        "--cycle-file",
        cycleFile,
        "--account-id",
        "account-a",
        "--sessions-dir",
        sessionsDir,
        "--json"
      ]);
      const json = JSON.parse(output) as {
        start: string;
        end: string;
        totals: { calls: number; usage: { totalTokens: number } };
        rows: Array<{ start: string; calls: number }>;
      };

      expect(new Date(json.start).getFullYear()).toBeLessThanOrEqual(1900);
      expect(new Date(json.end).getFullYear()).toBe(9999);
      expect(json.totals).toMatchObject({
        calls: 2,
        usage: { totalTokens: 150 }
      });
      expect(json.rows.map((row) => row.calls)).toEqual([1, 1]);
    } finally {
      await rm(tempDir, { force: true, recursive: true });
    }
  });

  it("prints estimated pre-anchor history only when requested", async () => {
    const tempDir = await mkdtemp(join(tmpdir(), "codex-helper-cycle-estimate-"));
    const cycleFile = join(tempDir, "stat-cycles.json");
    const sessionsDir = join(tempDir, "sessions");

    try {
      await writeCycleSession(sessionsDir, "rollout-2026-04-30T00-00-00-cycle-session.jsonl", [
        { timestamp: "2026-04-30T12:00:00.000Z", inputTokens: 10 },
        { timestamp: "2026-05-08T01:00:00.000Z", inputTokens: 20 }
      ]);
      await runCli([
        "cycle",
        "add",
        "2026-05-08T00:00:00Z",
        "--cycle-file",
        cycleFile,
        "--account-id",
        "account-a"
      ]);

      const exact = JSON.parse(
        await runCli([
          "cycle",
          "history",
          "--cycle-file",
          cycleFile,
          "--account-id",
          "account-a",
          "--sessions-dir",
          sessionsDir,
          "--start",
          "2026-04-24T00:00:00Z",
          "--end",
          "2026-05-09T00:00:00Z",
          "--json"
        ])
      ) as { rows: Array<{ source: string }>; totals: { calls: number } };
      const estimated = JSON.parse(
        await runCli([
          "cycle",
          "history",
          "--cycle-file",
          cycleFile,
          "--account-id",
          "account-a",
          "--sessions-dir",
          sessionsDir,
          "--start",
          "2026-04-24T00:00:00Z",
          "--end",
          "2026-05-09T00:00:00Z",
          "--estimate-before-anchor",
          "--json"
        ])
      ) as { rows: Array<{ source: string }>; totals: { calls: number } };

      expect(exact.rows.map((row) => row.source)).toEqual(["manual"]);
      expect(exact.totals.calls).toBe(1);
      expect(estimated.rows.map((row) => row.source)).toEqual(["estimated", "manual"]);
      expect(estimated.totals.calls).toBe(2);
    } finally {
      await rm(tempDir, { force: true, recursive: true });
    }
  });

  it("recognizes npm bin symlink execution as the main module", async () => {
    const tempDir = await mkdtemp(join(tmpdir(), "codex-helper-cli-"));
    const cliPath = fileURLToPath(new URL("../src/cli.ts", import.meta.url));
    const binPath = join(tempDir, "codex-helper");

    try {
      await symlink(cliPath, binPath);

      expect(isMainModule(binPath)).toBe(true);
    } finally {
      await rm(tempDir, { force: true, recursive: true });
    }
  });
});

async function runCli(args: string[]) {
  const output = new MemoryStream();
  const program = createProgram({ output });

  await program.parseAsync(["node", "codex-helper", ...args]);

  return output.toString();
}

async function writeCycleSession(
  sessionsDir: string,
  fileName: string,
  events: Array<{ timestamp: string; inputTokens: number }>
) {
  const dir = join(sessionsDir, "uncategorized");
  await mkdir(dir, { recursive: true });
  await writeFile(
    join(dir, fileName),
    [
      JSON.stringify({
        timestamp: "2026-05-01T00:00:00.000Z",
        type: "session_meta",
        payload: { id: "cycle-session", model: "gpt-5.5", cwd: "/repo/cycle" }
      }),
      ...events.map((event) =>
        JSON.stringify({
          timestamp: event.timestamp,
          type: "event_msg",
          payload: {
            type: "token_count",
            info: {
              last_token_usage: usage(event.inputTokens, 0, 0, 0, event.inputTokens),
              total_token_usage: usage(event.inputTokens, 0, 0, 0, event.inputTokens)
            }
          }
        })
      )
    ].join("\n")
  );
}

function createJwt(payload: Record<string, unknown>, header = { alg: "RS256", typ: "JWT", kid: "key-1" }) {
  return `${encodeJson(header)}.${encodeJson(payload)}.signature`;
}

function encodeJson(value: Record<string, unknown>) {
  return Buffer.from(JSON.stringify(value)).toString("base64url");
}

function timestamp(value: string) {
  return Math.floor(new Date(value).getTime() / 1000);
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

function createAuthContent(accountId: string, name: string, email: string, plan: string) {
  return JSON.stringify({
    auth_mode: "chatgpt",
    tokens: {
      id_token: createJwt({
        sub: `auth0|${accountId}`,
        name,
        email,
        "https://api.openai.com/auth": {
          chatgpt_account_id: accountId,
          user_id: `user-${accountId}`,
          chatgpt_plan_type: plan
        }
      }),
      refresh_token: "not-a-jwt",
      account_id: accountId
    }
  });
}
