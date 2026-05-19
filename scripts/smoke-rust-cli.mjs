#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { accessSync, constants, existsSync } from "node:fs";
import { cp, mkdir, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const FIXED_NOW = "2026-05-17T00:00:00.000Z";
const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const fixtureRoot = resolve(repoRoot, "test/fixtures/rust-run");
const shimPath = resolve(repoRoot, "bin/codex-ops.js");
const binaryName = process.platform === "win32" ? "codex-ops.exe" : "codex-ops";
const rustBinary = resolve(
  repoRoot,
  process.env.CODEX_OPS_RUST_BINARY ?? join("target", "release", binaryName)
);

const rawSecrets = [
  "fixture-signature",
  "synthetic-refresh-token",
  "synthetic-refresh-token-other"
];

main().catch((error) => {
  console.error(`error: ${error.message}`);
  process.exit(1);
});

async function main() {
  assertExecutable(rustBinary, "Rust binary is missing. Run `rtk cargo build --release` first.");
  assertFile(shimPath, `npm shim not found: ${shimPath}`);

  const sandbox = await prepareSandbox();
  try {
    await runSmoke(sandbox);
    console.log("rust CLI smoke passed");
  } finally {
    await rm(sandbox.root, { force: true, recursive: true });
  }
}

async function prepareSandbox() {
  const root = await mkdtemp(join(tmpdir(), "codex-ops-rust-smoke-"));
  const fixtureCopy = join(root, "fixture");
  const home = join(root, "home");
  await cp(fixtureRoot, fixtureCopy, { recursive: true });
  await mkdir(home, { recursive: true });

  const codexHome = join(fixtureCopy, "codex-home");
  const helperDir = join(codexHome, "codex-ops");
  return {
    root,
    fixtureCopy,
    context: {
      codexHome,
      authFile: join(codexHome, "auth.json"),
      sessionsDir: join(codexHome, "sessions"),
      storeDir: join(helperDir, "auth-profiles"),
      accountHistoryFile: join(helperDir, "auth-account-history.json"),
      cycleFile: join(helperDir, "stat-cycles.json")
    },
    env: {
      ...process.env,
      CODEX_HOME: codexHome,
      CODEX_OPS_FIXED_NOW: FIXED_NOW,
      HOME: home
    }
  };
}

async function runSmoke(sandbox) {
  const ctx = sandbox.context;

  const shimHelp = runShim(["--help"], sandbox.env);
  assertStatus(shimHelp, 0, "shim root help");
  assertIncludes(shimHelp.stdout, "Usage: codex-ops <command> [options]", "shim root help");

  const authStatus = runRust(["auth", "status", "--auth-file", ctx.authFile, "--json"], sandbox.env);
  assertStatus(authStatus, 0, "auth status json");
  assertNoSecrets(authStatus.stdout, "auth status json");
  const auth = parseJson(authStatus.stdout, "auth status json");
  assertEqual(auth.summary.chatgptAccountId, "account-fixture", "auth account id");
  assertEqual(auth.summary.email, "fixture@example.test", "auth email");
  assertEqual(auth.tokenClaimsIncluded, false, "auth claims default");

  const authSave = runRust(["auth", "save", "--auth-file", ctx.authFile, "--store-dir", ctx.storeDir], sandbox.env);
  assertStatus(authSave, 0, "auth save");
  assertIncludes(authSave.stdout, "Saved auth profile: fixture@example.test(account-fixture) - pro", "auth save");
  assertFile(join(ctx.storeDir, "account-fixture.json"), "auth save did not write current profile");

  const authList = runRust(["auth", "list", "--auth-file", ctx.authFile, "--store-dir", ctx.storeDir], sandbox.env);
  assertStatus(authList, 0, "auth list");
  assertNoSecrets(authList.stdout, "auth list");
  assertIncludes(authList.stdout, "Current: fixture@example.test(account-fixture) - pro", "auth list current");
  assertIncludes(authList.stdout, "other@example.test(account-other) - plus", "auth list persisted");

  const doctor = runRust(
    [
      "doctor",
      "--auth-file",
      ctx.authFile,
      "--codex-home",
      ctx.codexHome,
      "--sessions-dir",
      ctx.sessionsDir,
      "--cycle-file",
      ctx.cycleFile,
      "--json"
    ],
    sandbox.env
  );
  assertStatus(doctor, 0, "doctor json");
  assertNoSecrets(doctor.stdout, "doctor json");
  const doctorJson = parseJson(doctor.stdout, "doctor json");
  assertEqual(doctorJson.summary.errors, 0, "doctor errors");
  assertEqual(doctorJson.summary.warnings, 0, "doctor warnings");
  assertCheckStatus(doctorJson, "Auth file", "ok");
  assertCheckStatus(doctorJson, "Recent usage", "ok");
  assertCheckStatus(doctorJson, "Cycle store", "ok");

  const statJson = runRust(
    ["stat", "--all", "--format", "json", "--sessions-dir", ctx.sessionsDir],
    sandbox.env
  );
  assertStatus(statJson, 0, "stat json");
  const stat = parseJson(statJson.stdout, "stat json");
  assertEqual(stat.totals.sessions, 2, "stat total sessions");
  assertEqual(stat.totals.calls, 3, "stat total calls");
  assertEqual(stat.totals.usage.totalTokens, 3600, "stat total tokens");
  assertEqual(stat.diagnostics.includedUsageEvents, 3, "stat included events");

  const statAccount = runRust(
    [
      "stat",
      "--all",
      "--group-by",
      "account",
      "--format",
      "json",
      "--auth-file",
      ctx.authFile,
      "--account-history-file",
      ctx.accountHistoryFile,
      "--codex-home",
      ctx.codexHome,
      "--sessions-dir",
      ctx.sessionsDir
    ],
    sandbox.env
  );
  assertStatus(statAccount, 0, "stat account json");
  const statAccountJson = parseJson(statAccount.stdout, "stat account json");
  assertEqual(statAccountJson.rows[0].key, "account-fixture", "stat account row");

  const statTable = runRust(
    ["stat", "--all", "--format", "table", "--sessions-dir", ctx.sessionsDir],
    sandbox.env
  );
  assertStatus(statTable, 0, "stat table");
  assertIncludes(statTable.stdout, "Codex usage", "stat table title");
  assertIncludes(statTable.stdout, "Total", "stat table total");
  assertIncludes(statTable.stdout, "3,600", "stat table total tokens");

  const statCsv = runRust(
    ["stat", "--all", "--format", "csv", "--sessions-dir", ctx.sessionsDir],
    sandbox.env
  );
  assertStatus(statCsv, 0, "stat csv");
  const csvRows = parseCsv(statCsv.stdout);
  assertEqual(csvRows[0].join(","), "Group,Sessions,Calls,Input,Cached,Output,Reasoning,Total,Credits,USD", "stat csv header");
  assertEqual(csvRows.at(-1)[0], "Total", "stat csv total row");

  const statMarkdown = runRust(
    ["stat", "--all", "--format", "markdown", "--sessions-dir", ctx.sessionsDir],
    sandbox.env
  );
  assertStatus(statMarkdown, 0, "stat markdown");
  assertIncludes(statMarkdown.stdout, "| Group | Sessions | Calls |", "stat markdown header");
  assertIncludes(statMarkdown.stdout, "| Total | 2 | 3 |", "stat markdown total");

  const statSessions = runRust(
    ["stat", "sessions", "--all", "--top", "10", "--format", "json", "--sessions-dir", ctx.sessionsDir],
    sandbox.env
  );
  assertStatus(statSessions, 0, "stat sessions json");
  const sessions = parseJson(statSessions.stdout, "stat sessions json");
  assertEqual(sessions.rows.length, 2, "stat sessions rows");
  assertEqual(sessions.rows[0].sessionId, "rust-run-session-alpha", "stat sessions first id");
  assertEqual(sessions.totals.usage.totalTokens, 3600, "stat sessions total tokens");

  const cycleList = runRust(
    ["cycle", "list", "--cycle-file", ctx.cycleFile, "--account-id", "account-fixture", "--format", "json"],
    sandbox.env
  );
  assertStatus(cycleList, 0, "cycle list json");
  let cycleListJson = parseJson(cycleList.stdout, "cycle list json");
  assertEqual(cycleListJson.anchors.length, 1, "cycle list initial anchors");
  assertEqual(cycleListJson.anchors[0].id, "anc_20260510T090000000Z", "cycle list anchor id");

  const cycleCurrent = runRust(
    [
      "cycle",
      "current",
      "--cycle-file",
      ctx.cycleFile,
      "--account-id",
      "account-fixture",
      "--sessions-dir",
      ctx.sessionsDir,
      "--format",
      "json"
    ],
    sandbox.env
  );
  assertStatus(cycleCurrent, 0, "cycle current json");
  const current = parseJson(cycleCurrent.stdout, "cycle current json");
  assertEqual(current.status, "active", "cycle current status");
  assertEqual(current.totals.calls, 3, "cycle current calls");
  assertEqual(current.current.id, "anc_20260510T090000000Z", "cycle current id");

  const cycleHistory = runRust(
    [
      "cycle",
      "history",
      "--cycle-file",
      ctx.cycleFile,
      "--account-id",
      "account-fixture",
      "--sessions-dir",
      ctx.sessionsDir,
      "--all",
      "--format",
      "json"
    ],
    sandbox.env
  );
  assertStatus(cycleHistory, 0, "cycle history json");
  const history = parseJson(cycleHistory.stdout, "cycle history json");
  assertEqual(history.status, "ok", "cycle history status");
  assertEqual(history.rows.length, 1, "cycle history rows");

  const cycleDetail = runRust(
    [
      "cycle",
      "history",
      "anc_20260510T090000000Z",
      "--cycle-file",
      ctx.cycleFile,
      "--account-id",
      "account-fixture",
      "--sessions-dir",
      ctx.sessionsDir,
      "--all",
      "--format",
      "json"
    ],
    sandbox.env
  );
  assertStatus(cycleDetail, 0, "cycle history detail json");
  const detail = parseJson(cycleDetail.stdout, "cycle history detail json");
  assertEqual(detail.status, "ok", "cycle detail status");
  assertEqual(detail.cycle.id, "anc_20260510T090000000Z", "cycle detail id");
  assertEqual(detail.cycle.usage.totalTokens, 3600, "cycle detail tokens");

  const cycleAdd = runRust(
    [
      "cycle",
      "add",
      "2026-05-17",
      "09:00",
      "--note",
      "smoke",
      "--cycle-file",
      ctx.cycleFile,
      "--account-id",
      "account-fixture"
    ],
    sandbox.env
  );
  assertStatus(cycleAdd, 0, "cycle add");
  assertIncludes(cycleAdd.stdout, "Added weekly cycle anchor:", "cycle add");
  const addedAnchorId = match(cycleAdd.stdout, /Added weekly cycle anchor: (\S+)/, "cycle add id");

  cycleListJson = parseJson(
    runRust(
      ["cycle", "list", "--cycle-file", ctx.cycleFile, "--account-id", "account-fixture", "--format", "json"],
      sandbox.env
    ).stdout,
    "cycle list after add"
  );
  assertEqual(cycleListJson.anchors.length, 2, "cycle list anchors after add");
  assertEqual(cycleListJson.anchors.at(-1).id, addedAnchorId, "cycle added anchor id");

  const cycleRemove = runRust(
    [
      "cycle",
      "remove",
      addedAnchorId,
      "--cycle-file",
      ctx.cycleFile,
      "--account-id",
      "account-fixture"
    ],
    sandbox.env
  );
  assertStatus(cycleRemove, 0, "cycle remove");
  assertIncludes(cycleRemove.stdout, `Removed weekly cycle anchor: ${addedAnchorId}`, "cycle remove");

  cycleListJson = parseJson(
    runRust(
      ["cycle", "list", "--cycle-file", ctx.cycleFile, "--account-id", "account-fixture", "--format", "json"],
      sandbox.env
    ).stdout,
    "cycle list after remove"
  );
  assertEqual(cycleListJson.anchors.length, 1, "cycle list anchors after remove");
}

function runRust(args, env) {
  return runCommand(rustBinary, args, env);
}

function runShim(args, env) {
  return runCommand(process.execPath, [shimPath, ...args], {
    ...env,
    CODEX_OPS_RUST_BINARY: rustBinary
  });
}

function runCommand(command, args, env) {
  const result = spawnSync(command, args, {
    cwd: repoRoot,
    encoding: "utf8",
    env,
    maxBuffer: 64 * 1024 * 1024
  });

  if (result.error !== undefined) {
    throw new Error(`Failed to start ${command}: ${result.error.message}`);
  }

  return {
    status: result.status,
    signal: result.signal,
    stdout: result.stdout ?? "",
    stderr: result.stderr ?? ""
  };
}

function assertExecutable(path, message) {
  try {
    accessSync(path, constants.X_OK);
  } catch {
    throw new Error(message);
  }
}

function assertFile(path, message) {
  if (!existsSync(path)) {
    throw new Error(message);
  }
}

function assertStatus(result, expected, label) {
  if (result.status === expected) {
    return;
  }
  throw new Error(
    [
      `${label}: expected exit ${expected}, got ${result.status}`,
      "--- stdout ---",
      result.stdout,
      "--- stderr ---",
      result.stderr
    ].join("\n")
  );
}

function parseJson(stdout, label) {
  try {
    return JSON.parse(stdout);
  } catch (error) {
    throw new Error(`${label}: expected JSON output: ${error.message}`);
  }
}

function assertCheckStatus(report, name, expected) {
  const check = report.checks.find((item) => item.name === name);
  if (check === undefined) {
    throw new Error(`doctor check not found: ${name}`);
  }
  assertEqual(check.status, expected, `doctor check ${name}`);
}

function assertNoSecrets(text, label) {
  for (const secret of rawSecrets) {
    if (text.includes(secret)) {
      throw new Error(`${label}: output contains raw secret marker ${secret}`);
    }
  }
}

function assertIncludes(actual, expected, label) {
  if (!actual.includes(expected)) {
    throw new Error(`${label}: expected output to include ${JSON.stringify(expected)}`);
  }
}

function assertEqual(actual, expected, label) {
  if (Object.is(actual, expected)) {
    return;
  }
  throw new Error(`${label}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
}

function match(text, pattern, label) {
  const result = pattern.exec(text);
  if (result?.[1] === undefined) {
    throw new Error(`${label}: pattern not found`);
  }
  return result[1];
}

function parseCsv(stdout) {
  return stdout
    .trim()
    .split(/\r?\n/)
    .filter(Boolean)
    .map(parseCsvLine);
}

function parseCsvLine(line) {
  const cells = [];
  let current = "";
  let quoted = false;

  for (let index = 0; index < line.length; index += 1) {
    const char = line[index];
    const next = line[index + 1];
    if (char === "\"" && quoted && next === "\"") {
      current += "\"";
      index += 1;
      continue;
    }
    if (char === "\"") {
      quoted = !quoted;
      continue;
    }
    if (char === "," && !quoted) {
      cells.push(current);
      current = "";
      continue;
    }
    current += char;
  }
  cells.push(current);
  return cells.map((cell) => cell.trim());
}
