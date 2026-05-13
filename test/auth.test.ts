import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import {
  buildCodexAuthStatus,
  decodeJwt,
  formatAuthStatus,
  listCodexAuthProfiles,
  removeCodexAuthProfile,
  resolveCodexAuthProfileStoreDir,
  resolveCodexHelperDir,
  saveCurrentCodexAuthProfile,
  switchCodexAuthProfile,
  type CodexAuthJson,
  type JsonObject
} from "../src/index.js";

describe("auth", () => {
  it("decodes the fixed tokens.id_token JWT and formats key status details", () => {
    const now = new Date("2026-05-13T00:00:00.000Z");
    const idToken = createJwt({
      iss: "https://auth.example.test",
      sub: "auth0|user_123",
      aud: ["codex-helper", "api"],
      iat: timestamp("2026-05-12T23:00:00.000Z"),
      auth_time: timestamp("2026-05-12T22:50:00.000Z"),
      exp: timestamp("2026-05-13T02:30:00.000Z"),
      jti: "jwt-id-1",
      name: "Example User",
      email: "user@example.test",
      email_verified: true,
      auth_provider: "password",
      amr: ["pwd", "otp", "mfa"],
      "https://api.openai.com/auth": {
        chatgpt_account_id: "account_123",
        chatgpt_plan_type: "pro",
        chatgpt_subscription_active_start: "2026-05-01T00:00:00.000Z",
        chatgpt_subscription_active_until: "2026-06-01T00:00:00.000Z",
        chatgpt_subscription_last_checked: "2026-05-12T00:00:00.000Z",
        chatgpt_user_id: "user_123",
        user_id: "user_123",
        organizations: [
          {
            id: "org_123",
            is_default: true,
            role: "owner",
            title: "Personal"
          }
        ]
      }
    });
    const authJson = {
      auth_mode: "chatgpt",
      tokens: {
        id_token: idToken,
        access_token: "not-used",
        refresh_token: "not-a-jwt",
        account_id: "account_123"
      },
      last_refresh: "2026-05-12T05:32:41.917677755Z"
    } satisfies CodexAuthJson;

    const report = buildCodexAuthStatus(authJson, "/tmp/auth.json", now);

    expect(report.tokenName).toBe("id_token");
    expect(report.summary).toMatchObject({
      authMode: "chatgpt",
      tokenAccountId: "account_123",
      lastRefresh: new Date("2026-05-12T05:32:41.917Z"),
      algorithm: "RS256",
      keyId: "key-1",
      issuer: "https://auth.example.test",
      subject: "auth0|user_123",
      audience: ["codex-helper", "api"],
      isExpired: false,
      secondsUntilExpiry: 9000,
      name: "Example User",
      email: "user@example.test",
      emailVerified: true,
      authProvider: "password",
      authMethods: ["pwd", "otp", "mfa"],
      chatgptAccountId: "account_123",
      chatgptUserId: "user_123",
      userId: "user_123",
      planType: "pro",
      organizations: [
        {
          id: "org_123",
          title: "Personal",
          role: "owner",
          isDefault: true
        }
      ]
    });

    const text = formatAuthStatus(report);

    expect(text).toContain("Codex auth");
    expect(text).toContain("Account ID: account_123");
    expect(text).toContain("Key ID: key-1");
    expect(text).toContain("Name: Example User");
    expect(text).toContain("Email: user@example.test");
    expect(text).toContain("User ID: user_123");
    expect(text).toContain("Plan: pro");
    expect(text).toContain("Personal, org_123, role=owner, default");
    expect(text).not.toContain("Token: id_token");
    expect(text).not.toContain("Auth mode: chatgpt");
    expect(text).not.toContain("Issuer: https://auth.example.test");
    expect(text).not.toContain("Subject: auth0|user_123");
    expect(text).not.toContain("Expires at: 2026-05-13T02:30:00.000Z");
    expect(text).not.toContain(idToken);
  });

  it("omits decoded header and claims from JSON output by default", () => {
    const idToken = createJwt({ sub: "user_123", exp: timestamp("2026-05-13T00:00:00.000Z") });
    const report = buildCodexAuthStatus(
      {
        tokens: {
          id_token: idToken,
          refresh_token: "not-a-jwt"
        }
      } satisfies CodexAuthJson,
      "/tmp/auth.json",
      new Date("2026-05-12T00:00:00.000Z")
    );

    expect(JSON.parse(formatAuthStatus(report, "json"))).toMatchObject({
      tokenName: "id_token",
      tokenClaimsIncluded: false,
      summary: {
        subject: "user_123",
        expiresAt: "2026-05-13T00:00:00.000Z"
      }
    });
    expect(JSON.parse(formatAuthStatus(report, "json"))).not.toHaveProperty("header");
    expect(JSON.parse(formatAuthStatus(report, "json"))).not.toHaveProperty("claims");
  });

  it("prints decoded header and claims in JSON output only when explicitly requested", () => {
    const idToken = createJwt({ sub: "user_123", exp: timestamp("2026-05-13T00:00:00.000Z") });
    const report = buildCodexAuthStatus(
      {
        tokens: {
          id_token: idToken,
          refresh_token: "not-a-jwt"
        }
      } satisfies CodexAuthJson,
      "/tmp/auth.json",
      new Date("2026-05-12T00:00:00.000Z")
    );

    expect(JSON.parse(formatAuthStatus(report, "json", { includeTokenClaims: true }))).toMatchObject({
      tokenName: "id_token",
      tokenClaimsIncluded: true,
      header: {
        alg: "RS256",
        kid: "key-1"
      },
      claims: {
        sub: "user_123"
      }
    });
  });

  it("throws a clear error when the id_token is missing or malformed", () => {
    expect(() =>
      buildCodexAuthStatus({ tokens: {} } satisfies CodexAuthJson, "/tmp/auth.json")
    ).toThrow("No id_token");
    expect(() =>
      buildCodexAuthStatus(
        { tokens: { id_token: "not-a-jwt" } } satisfies CodexAuthJson,
        "/tmp/auth.json"
      )
    ).toThrow("id_token is not a JWT");
    expect(() => decodeJwt("not-a-jwt", "id_token")).toThrow("id_token is not a JWT");
  });

  it("persists the current auth.json by account id without rewriting its content", async () => {
    const tempDir = await mkdtemp(join(tmpdir(), "codex-helper-auth-store-"));
    const authFile = join(tempDir, "auth.json");
    const storeDir = join(tempDir, "auth-profiles");
    const content = createAuthContent("account-a", "User A", "a@example.test", "plus");

    try {
      await writeFile(authFile, content);

      const report = await saveCurrentCodexAuthProfile({ authFile, storeDir });
      const storedContent = await readFile(join(storeDir, "account-a.json"), "utf8");

      expect(report.profile.accountId).toBe("account-a");
      expect(storedContent).toBe(content);
    } finally {
      await rm(tempDir, { force: true, recursive: true });
    }
  });

  it("derives the default profile store from the helper directory under --codex-home", async () => {
    const tempDir = await mkdtemp(join(tmpdir(), "codex-helper-auth-path-"));

    try {
      const authFile = join(tempDir, "fixture-home", "auth.json");
      const codexHome = join(tempDir, "codex-home");
      const storeDir = join(codexHome, "codex-helper", "auth-profiles");

      expect(resolveCodexHelperDir({ codexHome })).toBe(join(codexHome, "codex-helper"));
      expect(resolveCodexAuthProfileStoreDir({ authFile, codexHome })).toBe(storeDir);
      expect(resolveCodexAuthProfileStoreDir({ storeDir: join(tempDir, "custom"), codexHome })).toBe(
        join(tempDir, "custom")
      );
    } finally {
      await rm(tempDir, { force: true, recursive: true });
    }
  });

  it("saves profiles to the helper store by default even when --auth-file is supplied", async () => {
    const tempDir = await mkdtemp(join(tmpdir(), "codex-helper-auth-default-store-"));
    const authFile = join(tempDir, "fixture-home", "auth.json");
    const codexHome = join(tempDir, "codex-home");
    const content = createAuthContent("account-a", "User A", "a@example.test", "plus");

    try {
      await mkdir(join(tempDir, "fixture-home"), { recursive: true });
      await writeFile(authFile, content);

      const report = await saveCurrentCodexAuthProfile({ authFile, codexHome });
      const expectedStoreDir = join(codexHome, "codex-helper", "auth-profiles");

      expect(report.storeDir).toBe(expectedStoreDir);
      expect(await readFile(join(expectedStoreDir, "account-a.json"), "utf8")).toBe(content);
    } finally {
      await rm(tempDir, { force: true, recursive: true });
    }
  });

  it("switches to a persisted profile after saving the current auth.json", async () => {
    const tempDir = await mkdtemp(join(tmpdir(), "codex-helper-auth-switch-"));
    const authFile = join(tempDir, "auth.json");
    const storeDir = join(tempDir, "auth-profiles");
    const currentContent = createAuthContent("account-a", "User A", "a@example.test", "plus");
    const selectedContent = createAuthContent("account-b", "User B", "b@example.test", "pro");

    try {
      await writeFile(authFile, selectedContent);
      await saveCurrentCodexAuthProfile({ authFile, storeDir });
      await writeFile(authFile, currentContent);

      const report = await switchCodexAuthProfile("account-b", { authFile, storeDir });

      expect(report.savedCurrent.accountId).toBe("account-a");
      expect(report.activated.accountId).toBe("account-b");
      expect(await readFile(authFile, "utf8")).toBe(selectedContent);
      expect(await readFile(join(storeDir, "account-a.json"), "utf8")).toBe(currentContent);
    } finally {
      await rm(tempDir, { force: true, recursive: true });
    }
  });

  it("lists current and persisted profiles and removes a persisted profile", async () => {
    const tempDir = await mkdtemp(join(tmpdir(), "codex-helper-auth-remove-"));
    const authFile = join(tempDir, "auth.json");
    const storeDir = join(tempDir, "auth-profiles");

    try {
      await writeFile(authFile, createAuthContent("account-a", "User A", "a@example.test", "plus"));
      await saveCurrentCodexAuthProfile({ authFile, storeDir });
      await writeFile(authFile, createAuthContent("account-b", "User B", "b@example.test", "pro"));
      await saveCurrentCodexAuthProfile({ authFile, storeDir });

      const before = await listCodexAuthProfiles({ authFile, storeDir });
      expect(before.current?.accountId).toBe("account-b");
      expect(before.stored.map((entry) => entry.accountId)).toEqual(["account-a", "account-b"]);

      const removed = await removeCodexAuthProfile("account-a", { authFile, storeDir });
      const after = await listCodexAuthProfiles({ authFile, storeDir });

      expect(removed.removed.accountId).toBe("account-a");
      expect(after.stored.map((entry) => entry.accountId)).toEqual(["account-b"]);
    } finally {
      await rm(tempDir, { force: true, recursive: true });
    }
  });

  it("skips malformed persisted profile files while listing valid profiles", async () => {
    const tempDir = await mkdtemp(join(tmpdir(), "codex-helper-auth-malformed-"));
    const authFile = join(tempDir, "auth.json");
    const storeDir = join(tempDir, "auth-profiles");

    try {
      await mkdir(storeDir, { recursive: true });
      await writeFile(authFile, createAuthContent("account-a", "User A", "a@example.test", "plus"));
      await writeFile(
        join(storeDir, "account-a.json"),
        createAuthContent("account-a", "User A", "a@example.test", "plus")
      );
      await writeFile(join(storeDir, "broken.json"), "{not-json");

      const report = await listCodexAuthProfiles({ authFile, storeDir });

      expect(report.stored.map((entry) => entry.accountId)).toEqual(["account-a"]);
      expect(report.skippedStored).toHaveLength(1);
      expect(report.skippedStored[0]?.profileFile).toBe(join(storeDir, "broken.json"));
      expect(report.skippedStored[0]?.reason).toContain("Failed to parse");
    } finally {
      await rm(tempDir, { force: true, recursive: true });
    }
  });
});

function createJwt(
  payload: JsonObject,
  header: JsonObject = { alg: "RS256", typ: "JWT", kid: "key-1" }
) {
  return `${encodeJson(header)}.${encodeJson(payload)}.signature`;
}

function encodeJson(value: JsonObject) {
  return Buffer.from(JSON.stringify(value)).toString("base64url");
}

function timestamp(value: string) {
  return Math.floor(new Date(value).getTime() / 1000);
}

function createAuthContent(accountId: string, name: string, email: string, plan: string) {
  return JSON.stringify(
    {
      auth_mode: "chatgpt",
      tokens: {
        id_token: createJwt({
          iss: "https://auth.example.test",
          sub: `auth0|${accountId}`,
          name,
          email,
          "https://api.openai.com/auth": {
            chatgpt_account_id: accountId,
            chatgpt_plan_type: plan,
            chatgpt_user_id: `user-${accountId}`,
            user_id: `user-${accountId}`,
            organizations: [
              {
                id: `org-${accountId}`,
                is_default: true,
                role: "owner",
                title: "Personal"
              }
            ]
          }
        }),
        access_token: "not-used",
        refresh_token: "not-a-jwt",
        account_id: accountId
      },
      last_refresh: "2026-05-12T05:32:41.917677755Z"
    },
    null,
    2
  );
}
