import { readdir, readFile, unlink } from "node:fs/promises";
import { join, resolve } from "node:path";
import { defaultCodexHome, resolveCodexHelperDir, writeSensitiveFile } from "./storage.js";

export { resolveCodexHelperDir, writeSensitiveFile } from "./storage.js";

export type JsonValue =
  | string
  | number
  | boolean
  | null
  | JsonValue[]
  | { [key: string]: JsonValue };
export type JsonObject = { [key: string]: JsonValue };

export type AuthStatusFormat = "table" | "json";

export type AuthStatusJsonOptions = {
  includeTokenClaims?: boolean;
};

export type RawAuthStatusOptions = {
  authFile?: string;
  codexHome?: string;
};

export type RawAuthProfileOptions = RawAuthStatusOptions & {
  storeDir?: string;
};

export type CodexAuthJson = JsonObject & {
  auth_mode?: JsonValue;
  OPENAI_API_KEY?: JsonValue;
  tokens?: JsonValue;
  last_refresh?: JsonValue;
};

export type JwtParts = {
  header: JsonObject;
  claims: JsonObject;
};

export type AuthOrganization = {
  id?: string;
  title?: string;
  role?: string;
  isDefault?: boolean;
};

export type AuthStatusSummary = {
  authMode?: string;
  tokenAccountId?: string;
  lastRefresh?: Date;
  tokenType?: string;
  algorithm?: string;
  keyId?: string;
  issuer?: string;
  subject?: string;
  audience: string[];
  jwtId?: string;
  issuedAt?: Date;
  expiresAt?: Date;
  notBefore?: Date;
  authTime?: Date;
  requestedAuthTime?: Date;
  isExpired?: boolean;
  secondsUntilExpiry?: number;
  name?: string;
  email?: string;
  emailVerified?: boolean;
  authProvider?: string;
  authMethods: string[];
  chatgptAccountId?: string;
  chatgptUserId?: string;
  userId?: string;
  planType?: string;
  subscriptionActiveStart?: Date;
  subscriptionActiveUntil?: Date;
  subscriptionLastChecked?: Date;
  organizations: AuthOrganization[];
  scopes: string[];
};

export type AuthStatusReport = {
  authFile: string;
  tokenName: "id_token";
  header: JsonObject;
  claims: JsonObject;
  summary: AuthStatusSummary;
};

export type AuthProfileSource = "current" | "stored";

export type AuthProfileEntry = {
  source: AuthProfileSource;
  accountId: string;
  profileFile?: string;
  authFile?: string;
  summary: AuthStatusSummary;
};

export type AuthProfileReadError = {
  profileFile: string;
  reason: string;
};

export type AuthProfileListReport = {
  authFile: string;
  storeDir: string;
  current?: AuthProfileEntry;
  stored: AuthProfileEntry[];
  skippedStored: AuthProfileReadError[];
};

export type AuthProfileSaveReport = {
  authFile: string;
  storeDir: string;
  profile: AuthProfileEntry;
};

export type AuthProfileSwitchReport = {
  authFile: string;
  storeDir: string;
  savedCurrent: AuthProfileEntry;
  activated: AuthProfileEntry;
};

export type AuthProfileRemoveReport = {
  storeDir: string;
  removed: AuthProfileEntry;
};

type ParsedAuthFile = {
  filePath: string;
  content: string;
  report: AuthStatusReport;
  accountId: string;
};

const OPENAI_AUTH_CLAIM = "https://api.openai.com/auth";

export async function readCodexAuthStatus(
  options: RawAuthStatusOptions = {},
  now = new Date()
): Promise<AuthStatusReport> {
  const authFile = resolveCodexAuthFile(options);
  const parsed = await readCodexAuthFile(authFile, now);

  return parsed.report;
}

export function buildCodexAuthStatus(
  authJson: CodexAuthJson,
  authFile: string,
  now = new Date()
): AuthStatusReport {
  const tokens = authJson.tokens;

  if (!isJsonObject(tokens)) {
    throw new Error("No id_token found in auth.json. Expected auth.json tokens.id_token.");
  }

  const idToken = tokens.id_token;

  if (typeof idToken !== "string" || idToken.length === 0) {
    throw new Error("No id_token found in auth.json. Expected auth.json tokens.id_token.");
  }

  const jwt = decodeJwt(idToken, "id_token");

  return {
    authFile,
    tokenName: "id_token",
    header: jwt.header,
    claims: jwt.claims,
    summary: summarizeAuthJwt(authJson, jwt, now)
  };
}

export function resolveCodexAuthFile(options: RawAuthStatusOptions = {}) {
  if (options.authFile !== undefined) {
    return resolve(options.authFile);
  }

  return join(options.codexHome ?? defaultCodexHome(), "auth.json");
}

export function resolveCodexAuthProfileStoreDir(options: RawAuthProfileOptions = {}) {
  if (options.storeDir !== undefined) {
    return resolve(options.storeDir);
  }

  return join(resolveCodexHelperDir({ codexHome: options.codexHome }), "auth-profiles");
}

export async function saveCurrentCodexAuthProfile(
  options: RawAuthProfileOptions = {},
  now = new Date()
): Promise<AuthProfileSaveReport> {
  const authFile = resolveCodexAuthFile(options);
  const storeDir = resolveCodexAuthProfileStoreDir(options);
  const current = await readCodexAuthFile(authFile, now);
  const profileFile = resolveCodexAuthProfileFile(storeDir, current.accountId);

  await writeSensitiveFile(profileFile, current.content);

  return {
    authFile,
    storeDir,
    profile: toAuthProfileEntry(current, "stored", profileFile)
  };
}

export async function listCodexAuthProfiles(
  options: RawAuthProfileOptions = {},
  now = new Date()
): Promise<AuthProfileListReport> {
  const authFile = resolveCodexAuthFile(options);
  const storeDir = resolveCodexAuthProfileStoreDir(options);
  const current = await readOptionalCodexAuthFile(authFile, now);
  const stored = await readStoredCodexAuthProfiles(storeDir, now);

  return {
    authFile,
    storeDir,
    current: current === undefined ? undefined : toAuthProfileEntry(current, "current", undefined, authFile),
    stored: stored.entries,
    skippedStored: stored.skipped
  };
}

export async function switchCodexAuthProfile(
  accountId: string,
  options: RawAuthProfileOptions = {},
  now = new Date()
): Promise<AuthProfileSwitchReport> {
  const authFile = resolveCodexAuthFile(options);
  const storeDir = resolveCodexAuthProfileStoreDir(options);
  const profileFile = resolveCodexAuthProfileFile(storeDir, accountId);
  const selected = await readCodexAuthFile(profileFile, now);

  if (selected.accountId !== accountId) {
    throw new Error(
      `Stored auth profile ${profileFile} contains account id ${selected.accountId}, expected ${accountId}.`
    );
  }

  const saved = await saveCurrentCodexAuthProfile(options, now);
  await writeSensitiveFile(authFile, selected.content);

  return {
    authFile,
    storeDir,
    savedCurrent: saved.profile,
    activated: toAuthProfileEntry(selected, "current", undefined, authFile)
  };
}

export async function removeCodexAuthProfile(
  accountId: string,
  options: RawAuthProfileOptions = {},
  now = new Date()
): Promise<AuthProfileRemoveReport> {
  const storeDir = resolveCodexAuthProfileStoreDir(options);
  const profileFile = resolveCodexAuthProfileFile(storeDir, accountId);
  const selected = await readCodexAuthFile(profileFile, now);

  if (selected.accountId !== accountId) {
    throw new Error(
      `Stored auth profile ${profileFile} contains account id ${selected.accountId}, expected ${accountId}.`
    );
  }

  await unlink(profileFile);

  return {
    storeDir,
    removed: toAuthProfileEntry(selected, "stored", profileFile)
  };
}

export function formatAuthProfileList(report: AuthProfileListReport) {
  const lines = ["Codex auth profiles", `Store: ${report.storeDir}`, ""];

  if (report.current === undefined) {
    lines.push("Current: (missing auth.json)");
  } else {
    lines.push(`Current: ${formatAuthProfileEntry(report.current)}`);
  }

  lines.push("");

  if (report.stored.length === 0) {
    lines.push("Persisted: none");
  } else {
    lines.push("Persisted:");
    report.stored.forEach((entry, index) => {
      const marker = entry.accountId === report.current?.accountId ? " (current)" : "";
      lines.push(`  ${index + 1}. ${formatAuthProfileEntry(entry)}${marker}`);
    });
  }

  if (report.skippedStored.length > 0) {
    lines.push("", "Skipped persisted profiles:");
    report.skippedStored.forEach((entry, index) => {
      lines.push(`  ${index + 1}. ${entry.profileFile} - ${entry.reason}`);
    });
  }

  return `${lines.join("\n")}\n`;
}

export function formatAuthProfileEntry(entry: AuthProfileEntry) {
  const label =
    entry.summary.email ??
    entry.summary.name ??
    entry.summary.userId ??
    entry.summary.chatgptUserId ??
    "unknown";
  const plan = entry.summary.planType ?? "unknown";

  return `${label}(${entry.accountId}) - ${plan}`;
}

export function decodeJwt(token: string, tokenName = "JWT"): JwtParts {
  const parts = token.split(".");

  if (parts.length !== 3 || parts.some((part) => part.length === 0)) {
    throw new Error(`${tokenName} is not a JWT with header, payload, and signature parts.`);
  }

  const header = decodeJwtJsonPart(parts[0] ?? "", tokenName, "header");
  const claims = decodeJwtJsonPart(parts[1] ?? "", tokenName, "payload");

  return { header, claims };
}

export function formatAuthStatus(
  report: AuthStatusReport,
  format: AuthStatusFormat = "table",
  options: AuthStatusJsonOptions = {}
): string {
  if (format === "json") {
    return `${JSON.stringify(toAuthStatusJson(report, options), null, 2)}\n`;
  }

  const lines = ["Codex auth"];
  const summary = report.summary;

  appendOptionalLine(lines, "Account ID", summary.chatgptAccountId ?? summary.tokenAccountId);
  appendOptionalLine(lines, "Key ID", summary.keyId);
  appendOptionalLine(lines, "Name", summary.name);
  appendOptionalLine(lines, "Email", summary.email);
  appendOptionalLine(lines, "User ID", summary.userId ?? summary.chatgptUserId);
  appendOptionalLine(lines, "Plan", summary.planType);

  if (summary.organizations.length > 0) {
    lines.push("Organizations:");
    for (const organization of summary.organizations) {
      lines.push(`  ${formatOrganization(organization)}`);
    }
  }

  return `${lines.join("\n")}\n`;
}

export function toAuthStatusJson(
  report: AuthStatusReport,
  options: AuthStatusJsonOptions = {}
) {
  const json: Record<string, unknown> = {
    authFile: report.authFile,
    tokenName: report.tokenName,
    tokenClaimsIncluded: options.includeTokenClaims === true,
    summary: {
      ...report.summary,
      lastRefresh: report.summary.lastRefresh?.toISOString(),
      issuedAt: report.summary.issuedAt?.toISOString(),
      expiresAt: report.summary.expiresAt?.toISOString(),
      notBefore: report.summary.notBefore?.toISOString(),
      authTime: report.summary.authTime?.toISOString(),
      requestedAuthTime: report.summary.requestedAuthTime?.toISOString(),
      subscriptionActiveStart: report.summary.subscriptionActiveStart?.toISOString(),
      subscriptionActiveUntil: report.summary.subscriptionActiveUntil?.toISOString(),
      subscriptionLastChecked: report.summary.subscriptionLastChecked?.toISOString()
    }
  };

  if (options.includeTokenClaims === true) {
    json.header = report.header;
    json.claims = report.claims;
  }

  return json;
}

async function readCodexAuthFile(filePath: string, now: Date): Promise<ParsedAuthFile> {
  const content = await readFile(filePath, "utf8");
  const authJson = parseCodexAuthJson(content, filePath);
  const report = buildCodexAuthStatus(authJson, filePath, now);
  const accountId = getAuthAccountId(report);

  return {
    filePath,
    content,
    report,
    accountId
  };
}

async function readOptionalCodexAuthFile(filePath: string, now: Date) {
  try {
    return await readCodexAuthFile(filePath, now);
  } catch (error) {
    if (isNodeError(error) && error.code === "ENOENT") {
      return undefined;
    }

    throw error;
  }
}

async function readStoredCodexAuthProfiles(storeDir: string, now: Date) {
  let filenames: string[];

  try {
    filenames = await readdir(storeDir);
  } catch (error) {
    if (isNodeError(error) && error.code === "ENOENT") {
      return { entries: [], skipped: [] };
    }

    throw error;
  }

  const entries: AuthProfileEntry[] = [];
  const skipped: AuthProfileReadError[] = [];

  for (const filename of filenames.filter((name) => name.endsWith(".json")).sort()) {
    const profileFile = join(storeDir, filename);

    try {
      const parsed = await readCodexAuthFile(profileFile, now);
      entries.push(toAuthProfileEntry(parsed, "stored", parsed.filePath));
    } catch (error) {
      skipped.push({
        profileFile,
        reason: error instanceof Error ? error.message : String(error)
      });
    }
  }

  return {
    entries: entries.sort((left, right) => left.accountId.localeCompare(right.accountId)),
    skipped
  };
}

function toAuthProfileEntry(
  parsed: ParsedAuthFile,
  source: AuthProfileSource,
  profileFile?: string,
  authFile?: string
): AuthProfileEntry {
  return {
    source,
    accountId: parsed.accountId,
    profileFile,
    authFile,
    summary: parsed.report.summary
  };
}

function getAuthAccountId(report: AuthStatusReport) {
  const accountId = report.summary.chatgptAccountId ?? report.summary.tokenAccountId;

  if (accountId === undefined || accountId.length === 0) {
    throw new Error("No account id found in auth.json.");
  }

  return accountId;
}

function resolveCodexAuthProfileFile(storeDir: string, accountId: string) {
  return join(storeDir, `${encodeURIComponent(accountId)}.json`);
}

function isNodeError(error: unknown): error is NodeJS.ErrnoException {
  return error instanceof Error && "code" in error;
}

function parseCodexAuthJson(content: string, filePath: string): CodexAuthJson {
  let parsed: unknown;

  try {
    parsed = JSON.parse(content);
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error);
    throw new Error(`Failed to parse ${filePath}: ${detail}`);
  }

  if (!isJsonObject(parsed)) {
    throw new Error(`Expected ${filePath} to contain a JSON object.`);
  }

  return parsed as CodexAuthJson;
}

function decodeJwtJsonPart(segment: string, tokenName: string, partName: string): JsonObject {
  let parsed: unknown;

  try {
    parsed = JSON.parse(Buffer.from(segment, "base64url").toString("utf8"));
  } catch {
    throw new Error(`${tokenName} ${partName} is not valid base64url JSON.`);
  }

  if (!isJsonObject(parsed)) {
    throw new Error(`${tokenName} ${partName} must be a JSON object.`);
  }

  return parsed;
}

function summarizeAuthJwt(authJson: CodexAuthJson, jwt: JwtParts, now: Date): AuthStatusSummary {
  const expiresAt = readNumericDateClaim(jwt.claims, "exp");
  const openaiAuth = getObjectClaim(jwt.claims, OPENAI_AUTH_CLAIM);
  const secondsUntilExpiry =
    expiresAt === undefined ? undefined : Math.floor((expiresAt.getTime() - now.getTime()) / 1000);
  const tokens = isJsonObject(authJson.tokens) ? authJson.tokens : undefined;

  return {
    authMode: getStringValue(authJson.auth_mode),
    tokenAccountId: getStringValue(tokens?.account_id),
    lastRefresh: readDateValue(authJson.last_refresh),
    tokenType: getStringClaim(jwt.header, "typ"),
    algorithm: getStringClaim(jwt.header, "alg"),
    keyId: getStringClaim(jwt.header, "kid"),
    issuer: getStringClaim(jwt.claims, "iss"),
    subject: getStringClaim(jwt.claims, "sub"),
    audience: getStringArrayClaim(jwt.claims, "aud"),
    jwtId: getStringClaim(jwt.claims, "jti"),
    issuedAt: readNumericDateClaim(jwt.claims, "iat"),
    expiresAt,
    notBefore: readNumericDateClaim(jwt.claims, "nbf"),
    authTime: readNumericDateClaim(jwt.claims, "auth_time"),
    requestedAuthTime: readNumericDateClaim(jwt.claims, "rat"),
    isExpired: expiresAt === undefined ? undefined : expiresAt.getTime() <= now.getTime(),
    secondsUntilExpiry,
    name: getStringClaim(jwt.claims, "name"),
    email: getStringClaim(jwt.claims, "email"),
    emailVerified: getBooleanClaim(jwt.claims, "email_verified"),
    authProvider: getStringClaim(jwt.claims, "auth_provider"),
    authMethods: getStringArrayClaim(jwt.claims, "amr"),
    chatgptAccountId: getStringClaim(openaiAuth, "chatgpt_account_id"),
    chatgptUserId: getStringClaim(openaiAuth, "chatgpt_user_id"),
    userId: getStringClaim(openaiAuth, "user_id"),
    planType: getStringClaim(openaiAuth, "chatgpt_plan_type"),
    subscriptionActiveStart: readDateValue(openaiAuth?.chatgpt_subscription_active_start),
    subscriptionActiveUntil: readDateValue(openaiAuth?.chatgpt_subscription_active_until),
    subscriptionLastChecked: readDateValue(openaiAuth?.chatgpt_subscription_last_checked),
    organizations: getOrganizations(openaiAuth),
    scopes: getScopeClaims(jwt.claims)
  };
}

function getObjectClaim(object: JsonObject | undefined, key: string) {
  if (object === undefined) {
    return undefined;
  }

  const value = object[key];
  return isJsonObject(value) ? value : undefined;
}

function getOrganizations(openaiAuth: JsonObject | undefined) {
  const organizations = openaiAuth?.organizations;

  if (!Array.isArray(organizations)) {
    return [];
  }

  return organizations.filter(isJsonObject).map((organization) => ({
    id: getStringClaim(organization, "id"),
    title: getStringClaim(organization, "title"),
    role: getStringClaim(organization, "role"),
    isDefault: getBooleanClaim(organization, "is_default")
  }));
}

function getStringClaim(object: JsonObject | undefined, key: string) {
  if (object === undefined) {
    return undefined;
  }

  return getStringValue(object[key]);
}

function getStringValue(value: JsonValue | undefined) {
  if (typeof value === "string") {
    return value;
  }

  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }

  return undefined;
}

function getBooleanClaim(object: JsonObject | undefined, key: string) {
  if (object === undefined) {
    return undefined;
  }

  const value = object[key];

  if (typeof value === "boolean") {
    return value;
  }

  if (typeof value === "string") {
    const normalized = value.toLowerCase();

    if (normalized === "true") {
      return true;
    }

    if (normalized === "false") {
      return false;
    }
  }

  return undefined;
}

function getStringArrayClaim(object: JsonObject | undefined, key: string) {
  if (object === undefined) {
    return [];
  }

  const value = object[key];

  if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
    return [String(value)];
  }

  if (Array.isArray(value)) {
    return value
      .filter(
        (item): item is string | number | boolean =>
          typeof item === "string" || typeof item === "number" || typeof item === "boolean"
      )
      .map(String);
  }

  return [];
}

function getScopeClaims(object: JsonObject) {
  const values = [
    ...getSpaceSeparatedClaim(object, "scope"),
    ...getSpaceSeparatedClaim(object, "scp"),
    ...getStringArrayClaim(object, "scopes")
  ];

  return [...new Set(values)].sort();
}

function getSpaceSeparatedClaim(object: JsonObject, key: string) {
  const value = object[key];

  if (typeof value === "string") {
    return value.split(/\s+/).filter((part) => part.length > 0);
  }

  return getStringArrayClaim(object, key);
}

function readNumericDateClaim(object: JsonObject, key: string) {
  const value = object[key];
  const timestamp =
    typeof value === "number" ? value : typeof value === "string" ? Number(value) : Number.NaN;

  if (!Number.isFinite(timestamp)) {
    return undefined;
  }

  return new Date(timestamp * 1000);
}

function readDateValue(value: JsonValue | undefined) {
  if (typeof value !== "string" || value.length === 0) {
    return undefined;
  }

  const date = new Date(value);

  if (!Number.isNaN(date.getTime())) {
    return date;
  }

  const millisecondPrecision = value.replace(/\.(\d{3})\d+(Z|[+-]\d{2}:\d{2})$/, ".$1$2");

  if (millisecondPrecision === value) {
    return undefined;
  }

  const normalizedDate = new Date(millisecondPrecision);
  return Number.isNaN(normalizedDate.getTime()) ? undefined : normalizedDate;
}

function appendOptionalLine(lines: string[], label: string, value: string | undefined) {
  if (value !== undefined && value.length > 0) {
    lines.push(`${label}: ${value}`);
  }
}

function formatOrganization(organization: AuthOrganization) {
  const parts = [
    organization.title,
    organization.id,
    organization.role === undefined ? undefined : `role=${organization.role}`,
    organization.isDefault === true ? "default" : undefined
  ].filter((part): part is string => part !== undefined && part.length > 0);

  return parts.length > 0 ? parts.join(", ") : "(unknown organization)";
}

function isJsonObject(value: unknown): value is JsonObject {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}
