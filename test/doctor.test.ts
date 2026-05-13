import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { formatDoctorReport, readDoctorReport } from "../src/doctor.js";

describe("doctor", () => {
  it("checks auth, sessions, helper storage, cycle store, recent usage, and pricing", async () => {
    const tempDir = await mkdtemp(join(tmpdir(), "codex-helper-doctor-"));
    const codexHome = join(tempDir, "codex-home");
    const sessionsDir = join(codexHome, "sessions");
    const dayDir = join(sessionsDir, "2026", "05", "13");
    const helperDir = join(codexHome, "codex-helper");
    const now = new Date("2026-05-13T01:00:00.000Z");

    try {
      await mkdir(dayDir, { recursive: true });
      await mkdir(helperDir, { recursive: true });
      await writeFile(
        join(codexHome, "auth.json"),
        JSON.stringify({
          auth_mode: "chatgpt",
          tokens: {
            id_token: createJwt({
              sub: "auth0|account-a",
              email: "user@example.test",
              exp: timestamp("2026-05-13T03:00:00.000Z"),
              "https://api.openai.com/auth": {
                chatgpt_account_id: "account-a",
                chatgpt_plan_type: "pro"
              }
            }),
            account_id: "account-a"
          }
        })
      );
      await writeFile(
        join(dayDir, "rollout-2026-05-13T00-00-00-doctor-session.jsonl"),
        [
          JSON.stringify({
            timestamp: "2026-05-13T00:00:00.000Z",
            type: "session_meta",
            payload: { id: "doctor-session", model: "gpt-5.3-codex-spark", cwd: "/repo/doctor" }
          }),
          JSON.stringify({
            timestamp: "2026-05-13T00:00:01.000Z",
            type: "event_msg",
            payload: {
              type: "token_count",
              info: {
                last_token_usage: usage(100, 0, 20, 0, 120),
                total_token_usage: usage(100, 0, 20, 0, 120)
              }
            }
          })
        ].join("\n")
      );

      const report = await readDoctorReport({ codexHome }, now);
      const output = formatDoctorReport(report);

      expect(report.checks.map((check) => [check.name, check.status])).toEqual([
        ["Node.js", "ok"],
        ["Codex home", "ok"],
        ["Auth file", "ok"],
        ["Sessions directory", "ok"],
        ["Helper directory", "ok"],
        ["Cycle store", "ok"],
        ["Recent usage", "ok"],
        ["Pricing", "ok"]
      ]);
      expect(output).toContain("Codex helper doctor");
      expect(output).toContain("GPT-5.3-Codex-Spark");
      expect(output).toContain("8 priced model(s), 0 known unpriced model(s)");
      expect(formatDoctorReport(report, "json")).toContain("\"summary\"");
    } finally {
      await rm(tempDir, { force: true, recursive: true });
    }
  });
});

function createJwt(
  payload: Record<string, unknown>,
  header = { alg: "RS256", typ: "JWT", kid: "key-1" }
) {
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
