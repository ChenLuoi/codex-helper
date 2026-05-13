import { constants } from "node:fs";
import { access, readdir, stat } from "node:fs/promises";
import { join } from "node:path";
import {
  CODEX_RATE_CARD_SOURCE,
  listKnownUnpricedModels,
  listModelPricing
} from "./pricing.js";
import { readCodexUsageStats } from "./stats.js";
import { readCodexAuthStatus, resolveCodexAuthFile } from "./index.js";
import { readWeeklyCycleStore, resolveWeeklyCycleStoreFile } from "./cycles.js";
import { defaultCodexHome, resolveCodexHelperDir } from "./storage.js";

export type DoctorStatus = "ok" | "warn" | "error";

export type DoctorCheck = {
  name: string;
  status: DoctorStatus;
  message: string;
  details: string[];
};

export type DoctorReport = {
  now: Date;
  codexHome: string;
  authFile: string;
  sessionsDir: string;
  helperDir: string;
  cycleFile: string;
  checks: DoctorCheck[];
};

export type DoctorOptions = {
  authFile?: string;
  codexHome?: string;
  sessionsDir?: string;
  cycleFile?: string;
};

export type DoctorFormat = "table" | "json";

export async function readDoctorReport(
  options: DoctorOptions = {},
  now = new Date()
): Promise<DoctorReport> {
  const codexHome = options.codexHome ?? defaultCodexHome();
  const authFile = resolveCodexAuthFile({ authFile: options.authFile, codexHome });
  const sessionsDir = options.sessionsDir ?? join(codexHome, "sessions");
  const helperDir = resolveCodexHelperDir({ codexHome });
  const cycleFile = resolveWeeklyCycleStoreFile({ cycleFile: options.cycleFile, codexHome });
  const checks: DoctorCheck[] = [];

  checks.push(checkNodeVersion());
  checks.push(await checkDirectory("Codex home", codexHome, { writable: false }));
  checks.push(await checkAuthFile(authFile, options, now));
  checks.push(await checkDirectory("Sessions directory", sessionsDir, { writable: false }));
  checks.push(await checkHelperDirectory(helperDir));
  checks.push(await checkCycleStore(cycleFile));
  checks.push(await checkRecentUsage(sessionsDir, now));
  checks.push(checkPricing());

  return {
    now,
    codexHome,
    authFile,
    sessionsDir,
    helperDir,
    cycleFile,
    checks
  };
}

export function formatDoctorReport(report: DoctorReport, format: DoctorFormat = "table") {
  if (format === "json") {
    return `${JSON.stringify(toDoctorJson(report), null, 2)}\n`;
  }

  const lines = [
    "Codex helper doctor",
    `Codex home: ${report.codexHome}`,
    `Auth file: ${report.authFile}`,
    `Sessions dir: ${report.sessionsDir}`,
    `Helper dir: ${report.helperDir}`,
    `Cycle file: ${report.cycleFile}`,
    ""
  ];

  for (const check of report.checks) {
    lines.push(`${statusLabel(check.status)} ${check.name}: ${check.message}`);
    for (const detail of check.details) {
      lines.push(`  ${detail}`);
    }
  }

  const errors = report.checks.filter((check) => check.status === "error").length;
  const warnings = report.checks.filter((check) => check.status === "warn").length;
  lines.push("", `Result: ${errors} error(s), ${warnings} warning(s)`);

  return `${lines.join("\n")}\n`;
}

function toDoctorJson(report: DoctorReport) {
  return {
    now: report.now.toISOString(),
    codexHome: report.codexHome,
    authFile: report.authFile,
    sessionsDir: report.sessionsDir,
    helperDir: report.helperDir,
    cycleFile: report.cycleFile,
    checks: report.checks,
    summary: {
      errors: report.checks.filter((check) => check.status === "error").length,
      warnings: report.checks.filter((check) => check.status === "warn").length
    }
  };
}

function checkNodeVersion(): DoctorCheck {
  const major = Number(/^v(\d+)/.exec(process.version)?.[1] ?? "0");

  if (major >= 20) {
    return ok("Node.js", `${process.version} satisfies >=20.0.0`);
  }

  return error("Node.js", `${process.version} is below the required >=20.0.0`);
}

async function checkAuthFile(
  authFile: string,
  options: DoctorOptions,
  now: Date
): Promise<DoctorCheck> {
  try {
    const report = await readCodexAuthStatus(
      {
        authFile,
        codexHome: options.codexHome
      },
      now
    );
    const summary = report.summary;
    const label = summary.email ?? summary.name ?? summary.userId ?? "authenticated";
    const details = [
      `Account: ${summary.chatgptAccountId ?? summary.tokenAccountId ?? "unknown"}`,
      `Plan: ${summary.planType ?? "unknown"}`
    ];

    if (summary.expiresAt !== undefined) {
      details.push(`Token expires: ${summary.expiresAt.toISOString()}`);
    }

    if (summary.isExpired === true) {
      return warn("Auth file", `Decoded ${authFile}, but the ID token is expired`, details);
    }

    return ok("Auth file", `Decoded ${authFile} for ${label}`, details);
  } catch (authError) {
    if (isNotFoundError(authError)) {
      return warn("Auth file", `Missing auth.json at ${authFile}`);
    }

    return error("Auth file", errorMessage(authError));
  }
}

async function checkDirectory(
  name: string,
  path: string,
  options: { writable: boolean }
): Promise<DoctorCheck> {
  try {
    const info = await stat(path);

    if (!info.isDirectory()) {
      return error(name, `${path} exists but is not a directory`);
    }

    await access(path, constants.R_OK | (options.writable ? constants.W_OK : 0));
    return ok(name, `${path} is accessible`);
  } catch (directoryError) {
    if (isNotFoundError(directoryError)) {
      return warn(name, `${path} does not exist`);
    }

    return error(name, errorMessage(directoryError));
  }
}

async function checkHelperDirectory(helperDir: string): Promise<DoctorCheck> {
  try {
    const info = await stat(helperDir);

    if (!info.isDirectory()) {
      return error("Helper directory", `${helperDir} exists but is not a directory`);
    }

    await access(helperDir, constants.R_OK | constants.W_OK);
    return ok("Helper directory", `${helperDir} is readable and writable`);
  } catch (helperError) {
    if (isNotFoundError(helperError)) {
      return ok("Helper directory", `${helperDir} does not exist yet; helper commands will create it`);
    }

    return error("Helper directory", errorMessage(helperError));
  }
}

async function checkCycleStore(cycleFile: string): Promise<DoctorCheck> {
  try {
    await access(cycleFile, constants.R_OK);
  } catch (cycleError) {
    if (isNotFoundError(cycleError)) {
      return ok("Cycle store", `${cycleFile} does not exist yet`);
    }

    return error("Cycle store", errorMessage(cycleError));
  }

  try {
    const store = await readWeeklyCycleStore(cycleFile);
    const accountCount = Object.keys(store.accounts).length;
    const anchorCount = Object.values(store.accounts).reduce(
      (sum, entry) => sum + entry.weekly.anchors.length,
      0
    );

    return ok("Cycle store", `Read ${cycleFile}`, [
      `Accounts: ${accountCount}`,
      `Weekly anchors: ${anchorCount}`
    ]);
  } catch (cycleError) {
    return error("Cycle store", errorMessage(cycleError));
  }
}

async function checkRecentUsage(sessionsDir: string, now: Date): Promise<DoctorCheck> {
  try {
    const entries = await readdir(sessionsDir).catch((usageError) => {
      if (isNotFoundError(usageError)) {
        return undefined;
      }

      throw usageError;
    });

    if (entries === undefined) {
      return warn("Recent usage", `Cannot scan usage because ${sessionsDir} does not exist`);
    }

    const report = await readCodexUsageStats({
      sessionsDir,
      start: new Date(now.getTime() - 7 * 24 * 60 * 60 * 1000),
      end: now,
      groupBy: "model"
    });

    const details = [
      `Files read: ${report.diagnostics?.readFiles ?? 0}`,
      `Token events: ${report.diagnostics?.tokenCountEvents ?? 0}`,
      `Included usage events: ${report.totals.calls}`
    ];

    if (report.unpricedModels.length > 0) {
      return warn(
        "Recent usage",
        `${report.totals.calls} usage event(s), with unpriced model usage found`,
        [
          ...details,
          ...report.unpricedModels.map((model) =>
            `${model.model}: ${model.calls} call(s), ${model.totalTokens} token(s)${
              model.note === undefined ? "" : ` (${model.note})`
            }`
          )
        ]
      );
    }

    if (report.totals.calls === 0) {
      return warn("Recent usage", "No token_count usage events found in the last 7 days", details);
    }

    return ok("Recent usage", `${report.totals.calls} usage event(s) found in the last 7 days`, details);
  } catch (usageError) {
    return error("Recent usage", errorMessage(usageError));
  }
}

function checkPricing(): DoctorCheck {
  const priced = listModelPricing();
  const unpriced = listKnownUnpricedModels();

  return ok(
    "Pricing",
    `${priced.length} priced model(s), ${unpriced.length} known unpriced model(s)`,
    [
      `Source: ${CODEX_RATE_CARD_SOURCE.name}`,
      `Checked: ${CODEX_RATE_CARD_SOURCE.checkedAt}`,
      `Credits: ${CODEX_RATE_CARD_SOURCE.creditToUsd}`,
      ...priced
        .filter((model) => model.note !== undefined)
        .map((model) => `${model.label}: ${model.note}`),
      ...unpriced.map((model) => `${model.label}: ${model.note}`)
    ]
  );
}

function ok(name: string, message: string, details: string[] = []): DoctorCheck {
  return { name, status: "ok", message, details };
}

function warn(name: string, message: string, details: string[] = []): DoctorCheck {
  return { name, status: "warn", message, details };
}

function error(name: string, message: string, details: string[] = []): DoctorCheck {
  return { name, status: "error", message, details };
}

function statusLabel(status: DoctorStatus) {
  switch (status) {
    case "ok":
      return "[ok]";
    case "warn":
      return "[warn]";
    case "error":
      return "[error]";
    default:
      return `[${status}]`;
  }
}

function isNotFoundError(error: unknown) {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    (error as { code?: unknown }).code === "ENOENT"
  );
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
