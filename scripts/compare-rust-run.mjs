#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { cp, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { existsSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const MAX_DIFFS = 40;
const PENDING_RUST_MESSAGE = "Rust candidate command is not implemented yet";
const FIXED_NOW = "2026-05-17T00:00:00.000Z";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const cases = [
  {
    id: "root-help",
    groups: ["help", "foundation"],
    args: ["--help"],
    compare: "contains",
    requiredStdout: ["auth", "doctor", "stat", "cycle"]
  },
  {
    id: "auth-help",
    groups: ["help", "auth"],
    args: ["auth", "--help"],
    compare: "contains",
    requiredStdout: ["status", "save", "list", "select", "remove"]
  },
  {
    id: "auth-status-help",
    groups: ["help", "auth"],
    args: ["auth", "status", "--help"],
    compare: "contains",
    requiredStdout: ["--auth-file", "--codex-home", "--json", "--include-token-claims"]
  },
  {
    id: "doctor-help",
    groups: ["help", "doctor"],
    args: ["doctor", "--help"],
    compare: "contains",
    requiredStdout: ["--auth-file", "--codex-home", "--sessions-dir", "--json"]
  },
  {
    id: "stat-help",
    groups: ["help", "stat"],
    args: ["stat", "--help"],
    compare: "contains",
    requiredStdout: [
      "--group-by",
      "--sort",
      "--limit",
      "--all",
      "--format",
      "--sessions-dir"
    ]
  },
  {
    id: "cycle-help",
    groups: ["help", "cycle"],
    args: ["cycle", "--help"],
    compare: "contains",
    requiredStdout: ["add", "list", "remove", "current", "history"]
  },
  {
    id: "cycle-history-help",
    groups: ["help", "cycle"],
    args: ["cycle", "history", "--help"],
    compare: "contains",
    requiredStdout: ["--select", "--estimate-before-anchor", "--sessions-dir", "--format"]
  },
  {
    id: "auth-status-json",
    groups: ["auth"],
    args: ["auth", "status", "--auth-file", "{authFile}", "--json"],
    compare: "json",
    pendingRustImplementation: true
  },
  {
    id: "auth-status-json-claims",
    groups: ["auth"],
    args: [
      "auth",
      "status",
      "--auth-file",
      "{authFile}",
      "--include-token-claims",
      "--json"
    ],
    compare: "json",
    pendingRustImplementation: true
  },
  {
    id: "auth-status-table",
    groups: ["auth"],
    args: ["auth", "status", "--auth-file", "{authFile}"],
    compare: "text",
    pendingRustImplementation: true
  },
  {
    id: "auth-save",
    groups: ["auth"],
    args: ["auth", "save", "--auth-file", "{authFile}", "--store-dir", "{storeDir}"],
    compare: "text",
    pendingRustImplementation: true
  },
  {
    id: "auth-list",
    groups: ["auth"],
    args: ["auth", "list", "--auth-file", "{authFile}", "--store-dir", "{storeDir}"],
    compare: "text",
    pendingRustImplementation: true
  },
  {
    id: "auth-select",
    groups: ["auth"],
    args: [
      "auth",
      "select",
      "--auth-file",
      "{authFile}",
      "--store-dir",
      "{storeDir}",
      "--codex-home",
      "{codexHome}",
      "--account-id",
      "account-other"
    ],
    compare: "text",
    sideEffectFiles: ["{cycleFile}"]
  },
  {
    id: "auth-remove",
    groups: ["auth"],
    args: [
      "auth",
      "remove",
      "--auth-file",
      "{authFile}",
      "--store-dir",
      "{storeDir}",
      "--account-id",
      "account-other",
      "--yes"
    ],
    compare: "text",
    pendingRustImplementation: true
  },
  {
    id: "doctor-json",
    groups: ["doctor"],
    args: [
      "doctor",
      "--auth-file",
      "{authFile}",
      "--codex-home",
      "{codexHome}",
      "--sessions-dir",
      "{sessionsDir}",
      "--cycle-file",
      "{cycleFile}",
      "--json"
    ],
    compare: "json"
  },
  {
    id: "stat-json",
    groups: ["stat", "stat-real"],
    args: ["stat", "--all", "--format", "json", "--sessions-dir", "{sessionsDir}"],
    compare: "json"
  },
  {
    id: "stat-table",
    groups: ["stat", "stat-real"],
    args: ["stat", "--all", "--format", "table", "--sessions-dir", "{sessionsDir}"],
    compare: "table"
  },
  {
    id: "stat-csv",
    groups: ["stat", "stat-real"],
    args: ["stat", "--all", "--format", "csv", "--sessions-dir", "{sessionsDir}"],
    compare: "csv"
  },
  {
    id: "stat-markdown",
    groups: ["stat", "stat-real"],
    args: ["stat", "--all", "--format", "markdown", "--sessions-dir", "{sessionsDir}"],
    compare: "markdown"
  },
  {
    id: "stat-account-json",
    groups: ["stat"],
    args: [
      "stat",
      "--all",
      "--group-by",
      "account",
      "--format",
      "json",
      "--auth-file",
      "{authFile}",
      "--account-history-file",
      "{accountHistoryFile}",
      "--sessions-dir",
      "{sessionsDir}"
    ],
    compare: "json"
  },
  {
    id: "stat-account-filter-json",
    groups: ["stat"],
    args: [
      "stat",
      "--all",
      "--account-id",
      "account-fixture",
      "--format",
      "json",
      "--auth-file",
      "{authFile}",
      "--account-history-file",
      "{accountHistoryFile}",
      "--sessions-dir",
      "{sessionsDir}"
    ],
    compare: "json"
  },
  {
    id: "stat-full-scan-json",
    groups: ["stat"],
    args: [
      "stat",
      "--start",
      "2026-05-10",
      "--end",
      "2026-05-11",
      "--full-scan",
      "--format",
      "json",
      "--sessions-dir",
      "{sessionsDir}"
    ],
    compare: "json"
  },
  {
    id: "stat-sessions-json",
    groups: ["stat", "stat-real"],
    args: ["stat", "sessions", "--all", "--format", "json", "--sessions-dir", "{sessionsDir}"],
    compare: "json"
  },
  {
    id: "stat-session-detail-json",
    groups: ["stat", "stat-real"],
    args: [
      "stat",
      "sessions",
      "rust-run-session-alpha",
      "--all",
      "--format",
      "json",
      "--sessions-dir",
      "{sessionsDir}"
    ],
    compare: "json"
  },
  {
    id: "cycle-add",
    groups: ["cycle"],
    args: [
      "cycle",
      "add",
      "2026-05-17",
      "09:00",
      "--note",
      "compare",
      "--cycle-file",
      "{cycleFile}",
      "--account-id",
      "account-fixture"
    ],
    compare: "text",
    sideEffectFiles: ["{cycleFile}"]
  },
  {
    id: "cycle-list-json",
    groups: ["cycle"],
    args: [
      "cycle",
      "list",
      "--cycle-file",
      "{cycleFile}",
      "--account-id",
      "account-fixture",
      "--format",
      "json"
    ],
    compare: "json"
  },
  {
    id: "cycle-remove",
    groups: ["cycle"],
    args: [
      "cycle",
      "remove",
      "anc_20260510T090000000Z",
      "--cycle-file",
      "{cycleFile}",
      "--account-id",
      "account-fixture"
    ],
    compare: "text",
    pendingRustImplementation: true
  },
  {
    id: "cycle-current-json",
    groups: ["cycle"],
    args: [
      "cycle",
      "current",
      "--cycle-file",
      "{cycleFile}",
      "--account-id",
      "account-fixture",
      "--sessions-dir",
      "{sessionsDir}",
      "--format",
      "json"
    ],
    compare: "json"
  },
  {
    id: "cycle-history-json",
    groups: ["cycle"],
    args: [
      "cycle",
      "history",
      "--cycle-file",
      "{cycleFile}",
      "--account-id",
      "account-fixture",
      "--sessions-dir",
      "{sessionsDir}",
      "--all",
      "--format",
      "json"
    ],
    compare: "json",
    pendingRustImplementation: true
  },
  {
    id: "cycle-history-detail-json",
    groups: ["cycle"],
    args: [
      "cycle",
      "history",
      "anc_20260510T090000000Z",
      "--cycle-file",
      "{cycleFile}",
      "--account-id",
      "account-fixture",
      "--sessions-dir",
      "{sessionsDir}",
      "--all",
      "--format",
      "json"
    ],
    compare: "json"
  },
  {
    id: "cycle-history-select-non-tty",
    groups: ["cycle"],
    args: [
      "cycle",
      "history",
      "--select",
      "--cycle-file",
      "{cycleFile}",
      "--account-id",
      "account-fixture",
      "--sessions-dir",
      "{sessionsDir}",
      "--all"
    ],
    compare: "text"
  }
];

main().catch((error) => {
  console.error(`error: ${error.message}`);
  process.exit(1);
});

async function main() {
  const options = parseArgs(process.argv.slice(2));
  if (!options.legacyJs) {
    throw new Error("Legacy JS/Rust comparison requires --legacy-js.");
  }
  const paths = resolvePaths(options);
  validateInputs(paths);

  const selectedCases = selectCases(options.matrix);
  const results = [];

  for (const matrixCase of selectedCases) {
    const sandbox = await prepareSandbox(paths.fixture, options);
    try {
      const args = expandArgs(matrixCase.args, sandbox.context);
      const js = runCommand({
        label: "JS",
        command: process.execPath,
        args: [paths.jsEntry, ...args],
        env: sandbox.env
      });
      const jsSideEffects = await captureSideEffects(matrixCase, sandbox.context);
      await resetSandboxFixture(sandbox, paths.fixture, options);
      const rust = runCommand({
        label: "Rust",
        command: paths.rustBinary,
        args,
        env: sandbox.env
      });
      const rustSideEffects = await captureSideEffects(matrixCase, sandbox.context);

      results.push(compareCase(matrixCase, args, js, rust, jsSideEffects, rustSideEffects));
    } finally {
      await rm(sandbox.root, { force: true, recursive: true });
    }
  }

  const report = buildReport(options, paths, results);
  printReport(report);

  if (options.output !== undefined) {
    const outputPath = resolve(repoRoot, options.output);
    await mkdir(dirname(outputPath), { recursive: true });
    await writeFile(outputPath, `${JSON.stringify(report, null, 2)}\n`);
  }

  if (report.summary.failed > 0) {
    process.exit(1);
  }
}

function parseArgs(args) {
  const options = {
    fixture: "test/fixtures/rust-run",
    matrix: "all",
    output: undefined,
    jsEntry: "dist/cli.mjs",
    rustBinary: "target/release/codex-ops",
    sessionsDir: undefined,
    runs: undefined,
    snapshotSessions: true,
    legacyJs: false
  };

  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];

    if (arg === "--fixture") {
      options.fixture = readArgValue(args, index, "--fixture");
      index += 1;
      continue;
    }

    if (arg.startsWith("--fixture=")) {
      options.fixture = arg.slice("--fixture=".length);
      continue;
    }

    if (arg === "--matrix") {
      options.matrix = readArgValue(args, index, "--matrix");
      index += 1;
      continue;
    }

    if (arg.startsWith("--matrix=")) {
      options.matrix = arg.slice("--matrix=".length);
      continue;
    }

    if (arg === "--output") {
      options.output = readArgValue(args, index, "--output");
      index += 1;
      continue;
    }

    if (arg.startsWith("--output=")) {
      options.output = arg.slice("--output=".length);
      continue;
    }

    if (arg === "--js-entry") {
      options.jsEntry = readArgValue(args, index, "--js-entry");
      index += 1;
      continue;
    }

    if (arg.startsWith("--js-entry=")) {
      options.jsEntry = arg.slice("--js-entry=".length);
      continue;
    }

    if (arg === "--rust-binary") {
      options.rustBinary = readArgValue(args, index, "--rust-binary");
      index += 1;
      continue;
    }

    if (arg.startsWith("--rust-binary=")) {
      options.rustBinary = arg.slice("--rust-binary=".length);
      continue;
    }

    if (arg === "--sessions-dir") {
      options.sessionsDir = readArgValue(args, index, "--sessions-dir");
      index += 1;
      continue;
    }

    if (arg.startsWith("--sessions-dir=")) {
      options.sessionsDir = arg.slice("--sessions-dir=".length);
      continue;
    }

    if (arg === "--runs") {
      options.runs = readArgValue(args, index, "--runs");
      index += 1;
      continue;
    }

    if (arg.startsWith("--runs=")) {
      options.runs = arg.slice("--runs=".length);
      continue;
    }

    if (arg === "--no-snapshot") {
      options.snapshotSessions = false;
      continue;
    }

    if (arg === "--legacy-js") {
      options.legacyJs = true;
      continue;
    }

    if (arg === "-h" || arg === "--help") {
      printHelp();
      process.exit(0);
    }

    throw new Error(`Unsupported argument: ${arg}`);
  }

  return options;
}

function readArgValue(args, index, name) {
  const value = args[index + 1];
  if (value === undefined || value.startsWith("--")) {
    throw new Error(`Missing value for ${name}`);
  }
  return value;
}

function printHelp() {
  console.log(`Usage: node scripts/compare-rust-run.mjs [options]

Legacy JS/Rust comparison harness. This is not part of the production
quality gate; pass --legacy-js to run it explicitly.

Options:
  --legacy-js            Enable historical JS baseline comparison
  --fixture <path>       Fixture root, default test/fixtures/rust-run
  --matrix <name>        Matrix group: all, help, foundation, auth, doctor,
                         stat, stat-real, cycle
                         Multiple groups can be comma-separated.
  --output <path>        Write a machine-readable JSON report
  --js-entry <path>      JS baseline entry, default dist/cli.mjs
  --rust-binary <path>   Rust candidate binary, default target/release/codex-ops
  --sessions-dir <path>  Override fixture sessions directory
  --runs <n>             Accepted for stat-real compatibility; ignored here
  --no-snapshot          Use --sessions-dir directly instead of copying it
  -h, --help             Print help`);
}

function resolvePaths(options) {
  return {
    fixture: resolve(repoRoot, options.fixture),
    jsEntry: resolve(repoRoot, options.jsEntry),
    rustBinary: resolve(repoRoot, options.rustBinary),
    sessionsDir:
      options.sessionsDir === undefined ? undefined : resolve(repoRoot, options.sessionsDir)
  };
}

function validateInputs(paths) {
  assertFile(paths.jsEntry, "JS baseline is missing. Run `rtk npm run build` first.");
  assertFile(paths.rustBinary, "Rust candidate binary is missing. Run `rtk cargo build --release` first.");
  if (!existsSync(paths.fixture) || !statSync(paths.fixture).isDirectory()) {
    throw new Error(`Fixture directory not found: ${paths.fixture}`);
  }
  if (paths.sessionsDir !== undefined) {
    if (!existsSync(paths.sessionsDir) || !statSync(paths.sessionsDir).isDirectory()) {
      throw new Error(`Sessions directory not found: ${paths.sessionsDir}`);
    }
  }
}

function assertFile(path, message) {
  if (!existsSync(path) || !statSync(path).isFile()) {
    throw new Error(message);
  }
}

function selectCases(matrix) {
  const requested = new Set(
    matrix
      .split(",")
      .map((group) => group.trim())
      .filter(Boolean)
  );

  if (requested.size === 0 || requested.has("all")) {
    return cases;
  }

  const known = new Set(cases.flatMap((matrixCase) => matrixCase.groups));
  for (const group of requested) {
    if (!known.has(group)) {
      throw new Error(`Unknown matrix group: ${group}`);
    }
  }

  return cases.filter((matrixCase) => matrixCase.groups.some((group) => requested.has(group)));
}

async function prepareSandbox(fixture, options) {
  const root = await mkdtemp(join(tmpdir(), "codex-ops-compare-"));
  const fixtureCopy = join(root, "fixture");
  const home = join(root, "home");
  await cp(fixture, fixtureCopy, { recursive: true });
  await mkdir(home, { recursive: true });

  const codexHome = join(fixtureCopy, "codex-home");
  const helperDir = join(codexHome, "codex-ops");
  const sessionsDir =
    options.sessionsDir === undefined
      ? join(codexHome, "sessions")
      : options.snapshotSessions
        ? join(root, "sessions-snapshot")
        : options.sessionsDir;
  if (options.sessionsDir !== undefined && options.snapshotSessions) {
    await cp(options.sessionsDir, sessionsDir, { recursive: true });
  }
  const context = {
    fixtureRoot: fixtureCopy,
    codexHome,
    authFile: join(codexHome, "auth.json"),
    sessionsDir,
    storeDir: join(helperDir, "auth-profiles"),
    accountHistoryFile: join(helperDir, "auth-account-history.json"),
    cycleFile: join(helperDir, "stat-cycles.json")
  };

  return {
    root,
    fixtureCopy,
    context,
    env: {
      ...process.env,
      CODEX_HOME: codexHome,
      CODEX_OPS_FIXED_NOW: FIXED_NOW,
      HOME: home
    }
  };
}

async function resetSandboxFixture(sandbox, fixture, options) {
  await rm(sandbox.fixtureCopy, { force: true, recursive: true });
  await cp(fixture, sandbox.fixtureCopy, { recursive: true });
}

function expandArgs(args, context) {
  return args.map((arg) =>
    arg.replace(/\{([a-zA-Z]+)\}/g, (_match, key) => {
      if (context[key] === undefined) {
        throw new Error(`Unknown argument placeholder: {${key}}`);
      }
      return context[key];
    })
  );
}

function runCommand(command) {
  const result = spawnSync(command.command, command.args, {
    cwd: repoRoot,
    encoding: "utf8",
    env: command.env,
    maxBuffer: 256 * 1024 * 1024
  });

  return {
    label: command.label,
    command: [command.command, ...command.args].join(" "),
    status: result.status,
    signal: result.signal,
    error: result.error?.message,
    stdout: result.stdout ?? "",
    stderr: result.stderr ?? ""
  };
}

async function captureSideEffects(matrixCase, context) {
  const files = matrixCase.sideEffectFiles ?? [];
  const output = {};
  for (const file of files) {
    const path = expandArgs([file], context)[0];
    try {
      output[file] = await readFile(path, "utf8");
    } catch (error) {
      if (error?.code === "ENOENT") {
        output[file] = null;
        continue;
      }
      throw error;
    }
  }
  return output;
}

function compareCase(matrixCase, args, js, rust, jsSideEffects = {}, rustSideEffects = {}) {
  if (matrixCase.pendingRustImplementation && isRustPending(rust)) {
    return {
      id: matrixCase.id,
      status: "pending",
      compare: matrixCase.compare,
      args,
      pendingReason: PENDING_RUST_MESSAGE,
      js: summarizeRun(js),
      rust: summarizeRun(rust),
      diffs: []
    };
  }

  const diffs = [];
  if (js.error !== undefined) {
    diffs.push(diff("process.error.js", js.error, undefined));
  }
  if (rust.error !== undefined) {
    diffs.push(diff("process.error.rust", undefined, rust.error));
  }
  if (js.status !== rust.status) {
    diffs.push(diff("exitCode", js.status, rust.status));
  }

  if (diffs.length === 0) {
    diffs.push(...compareOutput(matrixCase, js, rust));
  }
  if (diffs.length === 0) {
    diffs.push(...compareSideEffects(jsSideEffects, rustSideEffects));
  }

  return {
    id: matrixCase.id,
    status: diffs.length === 0 ? "passed" : "failed",
    compare: matrixCase.compare,
    args,
    js: summarizeRun(js),
    rust: summarizeRun(rust),
    diffs: diffs.slice(0, MAX_DIFFS)
  };
}

function compareSideEffects(jsSideEffects, rustSideEffects) {
  const diffs = [];
  const keys = [...new Set([...Object.keys(jsSideEffects), ...Object.keys(rustSideEffects)])].sort();
  for (const key of keys) {
    if (jsSideEffects[key] !== rustSideEffects[key]) {
      diffs.push(diff(`sideEffect.${key}`, jsSideEffects[key], rustSideEffects[key]));
    }
  }
  return diffs;
}

function isRustPending(rust) {
  return rust.status !== 0 && rust.stderr.includes(PENDING_RUST_MESSAGE);
}

function summarizeRun(run) {
  return {
    status: run.status,
    signal: run.signal,
    stdoutBytes: Buffer.byteLength(run.stdout),
    stderrBytes: Buffer.byteLength(run.stderr)
  };
}

function compareOutput(matrixCase, js, rust) {
  switch (matrixCase.compare) {
    case "contains":
      return compareContains(matrixCase, js, rust);
    case "json":
      return compareJson(js.stdout, rust.stdout);
    case "csv":
      return compareRows(parseCsv(js.stdout), parseCsv(rust.stdout), "csv");
    case "markdown":
      return compareRows(parseMarkdownTable(js.stdout), parseMarkdownTable(rust.stdout), "markdown");
    case "table":
      return compareRows(parsePlainTable(js.stdout), parsePlainTable(rust.stdout), "table");
    case "text":
      return compareText(js.stdout, rust.stdout);
    default:
      return [diff("compare", matrixCase.compare, "known compare mode")];
  }
}

function compareContains(matrixCase, js, rust) {
  const diffs = [];
  for (const token of matrixCase.requiredStdout ?? []) {
    if (!js.stdout.includes(token)) {
      diffs.push(diff(`stdout.contains(${token}).js`, false, true));
    }
    if (!rust.stdout.includes(token)) {
      diffs.push(diff(`stdout.contains(${token}).rust`, false, true));
    }
  }
  return diffs;
}

function compareJson(jsStdout, rustStdout) {
  let jsJson;
  let rustJson;
  try {
    jsJson = JSON.parse(jsStdout);
  } catch (error) {
    return [diff("json.parse.js", error.message, "parseable JSON")];
  }
  try {
    rustJson = JSON.parse(rustStdout);
  } catch (error) {
    return [diff("json.parse.rust", error.message, "parseable JSON")];
  }
  return compareJsonValue(jsJson, rustJson, "$");
}

function compareJsonValue(jsValue, rustValue, path) {
  const jsType = jsonType(jsValue);
  const rustType = jsonType(rustValue);
  if (jsType !== rustType) {
    return [diff(`${path}.type`, jsType, rustType)];
  }

  if (jsType === "array") {
    const diffs = [];
    if (jsValue.length !== rustValue.length) {
      diffs.push(diff(`${path}.length`, jsValue.length, rustValue.length));
    }
    for (let index = 0; index < Math.min(jsValue.length, rustValue.length); index += 1) {
      diffs.push(...compareJsonValue(jsValue[index], rustValue[index], `${path}[${index}]`));
      if (diffs.length >= MAX_DIFFS) {
        return diffs;
      }
    }
    return diffs;
  }

  if (jsType === "object") {
    const diffs = [];
    const jsKeys = Object.keys(jsValue).sort();
    const rustKeys = Object.keys(rustValue).sort();
    if (jsKeys.join("\u0000") !== rustKeys.join("\u0000")) {
      diffs.push(diff(`${path}.keys`, jsKeys, rustKeys));
    }
    for (const key of jsKeys.filter((key) => Object.hasOwn(rustValue, key))) {
      diffs.push(...compareJsonValue(jsValue[key], rustValue[key], `${path}.${key}`));
      if (diffs.length >= MAX_DIFFS) {
        return diffs;
      }
    }
    return diffs;
  }

  return Object.is(jsValue, rustValue) ? [] : [diff(path, jsValue, rustValue)];
}

function jsonType(value) {
  if (value === null) {
    return "null";
  }
  if (Array.isArray(value)) {
    return "array";
  }
  return typeof value;
}

function compareText(jsStdout, rustStdout) {
  const jsText = normalizeText(jsStdout);
  const rustText = normalizeText(rustStdout);
  return jsText === rustText ? [] : [diff("stdout.text", jsText, rustText)];
}

function normalizeText(value) {
  return value
    .split(/\r?\n/)
    .map((line) => line.trim().replace(/\s+/g, " "))
    .filter(Boolean)
    .join("\n");
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
    if (char === '"' && quoted && next === '"') {
      current += '"';
      index += 1;
      continue;
    }
    if (char === '"') {
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

function parseMarkdownTable(stdout) {
  return stdout
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.startsWith("|") && line.endsWith("|"))
    .filter((line) => !/^\|\s*:?-{3,}:?\s*(\|\s*:?-{3,}:?\s*)+\|$/.test(line))
    .map((line) =>
      line
        .slice(1, -1)
        .split("|")
        .map((cell) => cell.trim())
    );
}

function parsePlainTable(stdout) {
  return stdout
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => line.split(/\s{2,}/).map((cell) => cell.trim()));
}

function compareRows(jsRows, rustRows, label) {
  const diffs = [];
  if (jsRows.length !== rustRows.length) {
    diffs.push(diff(`${label}.rows.length`, jsRows.length, rustRows.length));
  }
  const rowCount = Math.min(jsRows.length, rustRows.length);
  for (let row = 0; row < rowCount; row += 1) {
    const jsRow = jsRows[row] ?? [];
    const rustRow = rustRows[row] ?? [];
    if (jsRow.length !== rustRow.length) {
      diffs.push(diff(`${label}.row[${row}].length`, jsRow.length, rustRow.length));
      continue;
    }
    for (let column = 0; column < jsRow.length; column += 1) {
      if (jsRow[column] !== rustRow[column]) {
        diffs.push(diff(`${label}.row[${row}].column[${column}]`, jsRow[column], rustRow[column]));
      }
      if (diffs.length >= MAX_DIFFS) {
        return diffs;
      }
    }
  }
  return diffs;
}

function diff(path, js, rust) {
  return { path, js, rust };
}

function buildReport(options, paths, results) {
  const summary = {
    total: results.length,
    passed: results.filter((result) => result.status === "passed").length,
    pending: results.filter((result) => result.status === "pending").length,
    failed: results.filter((result) => result.status === "failed").length
  };

  return {
    tool: "compare-rust-run",
    mode: "legacy-js-comparison",
    matrix: options.matrix,
    fixture: paths.fixture,
    jsEntry: paths.jsEntry,
    rustBinary: paths.rustBinary,
    summary,
    results
  };
}

function printReport(report) {
  const status = report.summary.failed === 0 ? "PASS" : "FAIL";
  console.log(`Cross-language comparison: ${status}`);
  console.log(
    `Cases: ${report.summary.total} total, ${report.summary.passed} passed, ${report.summary.pending} pending, ${report.summary.failed} failed`
  );
  for (const result of report.results) {
    console.log(`- ${result.status.toUpperCase()} ${result.id}`);
    if (result.status === "failed") {
      for (const item of result.diffs.slice(0, 5)) {
        console.log(`  ${item.path}: JS=${formatValue(item.js)} Rust=${formatValue(item.rust)}`);
      }
      if (result.diffs.length > 5) {
        console.log(`  ... ${result.diffs.length - 5} more diff(s)`);
      }
    }
  }
}

function formatValue(value) {
  if (typeof value === "string") {
    return JSON.stringify(value.length > 160 ? `${value.slice(0, 157)}...` : value);
  }
  return JSON.stringify(value);
}
