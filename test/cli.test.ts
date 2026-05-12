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
    const storeDir = join(tempDir, "auth-profiles");
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
    expect(helpText).toContain("--verbose");
    expect(helpText).toContain("--top");
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

function createJwt(payload: Record<string, unknown>, header = { alg: "RS256", typ: "JWT", kid: "key-1" }) {
  return `${encodeJson(header)}.${encodeJson(payload)}.signature`;
}

function encodeJson(value: Record<string, unknown>) {
  return Buffer.from(JSON.stringify(value)).toString("base64url");
}

function timestamp(value: string) {
  return Math.floor(new Date(value).getTime() / 1000);
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
