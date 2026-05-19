#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { cp, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { existsSync, statSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { performance } from "node:perf_hooks";

const MAX_DIFFS = 20;
const FIXED_NOW = "2026-05-17T00:00:00.000Z";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const benchmarkCases = [
  {
    id: "auth-status-json",
    groups: ["auth"],
    compare: "json",
    args: ["auth", "status", "--auth-file", "{authFile}", "--json"]
  },
  {
    id: "doctor-json",
    groups: ["doctor"],
    compare: "json",
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
    ]
  },
  {
    id: "stat-json-poc",
    groups: ["stat", "poc"],
    compare: "json",
    includeLegacyPoc: true,
    args: ["stat", "--all", "--format", "json", "--sessions-dir", "{sessionsDir}"]
  },
  {
    id: "stat-table",
    groups: ["stat"],
    compare: "table",
    args: ["stat", "--all", "--format", "table", "--sessions-dir", "{sessionsDir}"]
  },
  {
    id: "stat-model-json",
    groups: ["stat"],
    compare: "json",
    args: [
      "stat",
      "--all",
      "--group-by",
      "model",
      "--reasoning-effort",
      "--sort",
      "tokens",
      "--limit",
      "10",
      "--format",
      "json",
      "--sessions-dir",
      "{sessionsDir}"
    ]
  },
  {
    id: "stat-account-json",
    groups: ["stat"],
    compare: "json",
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
    ]
  },
  {
    id: "stat-sessions-json",
    groups: ["stat"],
    compare: "json",
    args: [
      "stat",
      "sessions",
      "--all",
      "--top",
      "10",
      "--format",
      "json",
      "--sessions-dir",
      "{sessionsDir}"
    ]
  },
  {
    id: "cycle-current-json",
    groups: ["cycle"],
    compare: "json",
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
    ]
  },
  {
    id: "cycle-history-json",
    groups: ["cycle"],
    compare: "json",
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
    ]
  }
];

main().catch((error) => {
  console.error(`error: ${error.message}`);
  process.exit(1);
});

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const paths = resolvePaths(options);
  validateInputs(paths, options);

  const selectedCases = selectCases(options.matrix);
  const sandbox = await prepareSandbox(paths, options);

  try {
    const scale = await summarizeSessions(sandbox.context.sessionsDir);
    const comparison = !options.legacyJs
      ? {
          status: "not-run",
          reason: "Rust-only benchmark mode; enable --legacy-js to run the historical JS comparison harness."
        }
      : options.skipHarness
      ? {
          status: "skipped",
          reason: "Skipped by --skip-harness; each benchmark command still performs cold JS/Rust output comparison before timing."
        }
      : runComparisonHarness(options, paths);
    const results = [];

    for (const benchCase of selectedCases) {
      results.push(runBenchmarkCase(benchCase, sandbox, paths, options));
    }

    const report = buildReport(options, paths, sandbox, scale, comparison, results);
    printSummary(report);
    await writeReport(report, options.output);
  } finally {
    await rm(sandbox.root, { force: true, recursive: true });
  }
}

function parseArgs(args) {
  const options = {
    fixture: "test/fixtures/rust-run",
    sessionsDir: undefined,
    runs: 7,
    matrix: "all",
    output: undefined,
    jsEntry: "dist/cli.mjs",
    rustBinary: "target/release/codex-ops",
    legacyPocBinary: "target/release/codex-ops-stat-poc",
    snapshotSessions: true,
    skipHarness: false,
    legacyJs: false,
    legacyPoc: false
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
      options.runs = parseRuns(readArgValue(args, index, "--runs"));
      index += 1;
      continue;
    }
    if (arg.startsWith("--runs=")) {
      options.runs = parseRuns(arg.slice("--runs=".length));
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

    if (arg === "--legacy-poc-binary") {
      options.legacyPocBinary = readArgValue(args, index, "--legacy-poc-binary");
      index += 1;
      continue;
    }
    if (arg.startsWith("--legacy-poc-binary=")) {
      options.legacyPocBinary = arg.slice("--legacy-poc-binary=".length);
      continue;
    }

    if (arg === "--no-snapshot") {
      options.snapshotSessions = false;
      continue;
    }

    if (arg === "--skip-harness") {
      options.skipHarness = true;
      continue;
    }

    if (arg === "--legacy-js") {
      options.legacyJs = true;
      continue;
    }

    if (arg === "--legacy-poc") {
      options.legacyPoc = true;
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

function parseRuns(value) {
  const runs = Number(value);
  if (!Number.isSafeInteger(runs) || runs < 0) {
    throw new Error("Invalid --runs value. Expected a non-negative integer.");
  }
  return runs;
}

function printHelp() {
  console.log(`Usage: node scripts/bench-rust-run.mjs [options]

Runs the production Rust binary against the same fixture or sessions snapshot.
Historical JS and stat POC comparisons are disabled unless explicitly requested.

Options:
  --fixture <path>             Fixture root, default test/fixtures/rust-run
  --sessions-dir <path>        Real sessions directory to snapshot for timing
  --runs <n>                   Warm runs per command, default 7; use 0 for cold-only
  --matrix <groups>            all, auth, doctor, stat, cycle, poc; comma-separated
  --output <path>              JSON report path; default task/rust-run-bench-<mode>-report.json
  --rust-binary <path>         Production Rust binary, default target/release/codex-ops
  --legacy-js                  Also run the historical JS baseline and JS/Rust comparison harness
  --js-entry <path>            Legacy JS entry, default dist/cli.mjs
  --legacy-poc                 Also run the historical stat POC for cases that support it
  --legacy-poc-binary <path>   Legacy stat POC binary, default target/release/codex-ops-stat-poc
  --no-snapshot                Use --sessions-dir directly instead of copying it
  --skip-harness               With --legacy-js, skip the extra comparison harness preflight
  -h, --help                   Print help`);
}

function resolvePaths(options) {
  const sessionsDir =
    options.sessionsDir === undefined ? undefined : resolveInputPath(options.sessionsDir);
  return {
    fixture: resolveInputPath(options.fixture),
    sessionsDir,
    jsEntry: resolveInputPath(options.jsEntry),
    rustBinary: resolveInputPath(options.rustBinary),
    legacyPocBinary: resolveInputPath(options.legacyPocBinary),
    compareScript: resolve(repoRoot, "scripts/compare-rust-run.mjs")
  };
}

function resolveInputPath(value) {
  if (value === "~") {
    return homedir();
  }
  if (value.startsWith("~/")) {
    return resolve(homedir(), value.slice(2));
  }
  return resolve(repoRoot, value);
}

function validateInputs(paths, options) {
  assertDirectory(paths.fixture, `Fixture directory not found: ${paths.fixture}`);
  assertFile(paths.rustBinary, "Rust binary is missing. Run `rtk cargo build --release` first.");
  if (options.legacyJs) {
    assertFile(paths.jsEntry, "Legacy JS baseline is missing. Run `rtk npm run build` first.");
    if (!options.skipHarness) {
      assertFile(paths.compareScript, `Comparison harness not found: ${paths.compareScript}`);
    }
  }
  if (options.legacyPoc) {
    assertFile(
      paths.legacyPocBinary,
      "Legacy stat POC binary is missing. Run `rtk cargo build --release` first."
    );
  }
  if (paths.sessionsDir !== undefined) {
    assertDirectory(paths.sessionsDir, `Sessions directory not found: ${paths.sessionsDir}`);
  }
}

function assertFile(path, message) {
  if (!existsSync(path) || !statSync(path).isFile()) {
    throw new Error(message);
  }
}

function assertDirectory(path, message) {
  if (!existsSync(path) || !statSync(path).isDirectory()) {
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
    return benchmarkCases;
  }

  const known = new Set(benchmarkCases.flatMap((benchCase) => benchCase.groups));
  for (const group of requested) {
    if (!known.has(group)) {
      throw new Error(`Unknown matrix group: ${group}`);
    }
  }

  return benchmarkCases.filter((benchCase) =>
    benchCase.groups.some((group) => requested.has(group))
  );
}

async function prepareSandbox(paths, options) {
  const root = await mkdtemp(join(tmpdir(), "codex-ops-bench-"));
  const fixtureCopy = join(root, "fixture");
  const home = join(root, "home");
  await cp(paths.fixture, fixtureCopy, { recursive: true });
  await mkdir(home, { recursive: true });

  const codexHome = join(fixtureCopy, "codex-home");
  const helperDir = join(codexHome, "codex-ops");
  const sessionsDir =
    paths.sessionsDir === undefined
      ? join(codexHome, "sessions")
      : options.snapshotSessions
        ? join(root, "sessions-snapshot")
        : paths.sessionsDir;

  if (paths.sessionsDir !== undefined && options.snapshotSessions) {
    await cp(paths.sessionsDir, sessionsDir, { recursive: true });
  }

  return {
    root,
    fixtureCopy,
    mode: options.sessionsDir === undefined ? "fixture" : "real-sessions",
    context: {
      codexHome,
      authFile: join(codexHome, "auth.json"),
      sessionsDir,
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

function runComparisonHarness(options, paths) {
  const matrix = paths.sessionsDir === undefined ? "all" : "stat-real";
  const args = [
    paths.compareScript,
    "--legacy-js",
    "--fixture",
    paths.fixture,
    "--matrix",
    matrix,
    "--js-entry",
    paths.jsEntry,
    "--rust-binary",
    paths.rustBinary
  ];

  if (paths.sessionsDir !== undefined) {
    args.push("--sessions-dir", paths.sessionsDir);
    if (!options.snapshotSessions) {
      args.push("--no-snapshot");
    }
  }

  const result = spawnSync(process.execPath, args, {
    cwd: repoRoot,
    encoding: "utf8",
    env: process.env,
    maxBuffer: 256 * 1024 * 1024
  });

  if (result.error !== undefined) {
    throw new Error(`Correctness comparison failed to start: ${result.error.message}`);
  }
  if (result.status !== 0) {
    throw new Error(
      [
        "Correctness comparison failed before timing; no performance report was produced.",
        `Command: node ${args.map(shellToken).join(" ")}`,
        `Exit code: ${result.status}`,
        summarizeProcessOutput(result)
      ]
        .filter(Boolean)
        .join("\n")
    );
  }

  return {
    status: "passed",
    matrix,
    command: ["node", ...args.map(reportPath)].join(" "),
    summary: parseComparisonSummary(result.stdout)
  };
}

function parseComparisonSummary(stdout) {
  const line = stdout
    .split(/\r?\n/)
    .find((item) => item.startsWith("Cases: "));
  if (line === undefined) {
    return undefined;
  }
  const match = /Cases: (?<total>\d+) total, (?<passed>\d+) passed, (?<pending>\d+) pending, (?<failed>\d+) failed/.exec(line);
  if (match?.groups === undefined) {
    return undefined;
  }
  return {
    total: Number(match.groups.total),
    passed: Number(match.groups.passed),
    pending: Number(match.groups.pending),
    failed: Number(match.groups.failed)
  };
}

function runBenchmarkCase(benchCase, sandbox, paths, options) {
  const args = expandArgs(benchCase.args, sandbox.context);
  const rustCommand = {
    label: "Rust",
    command: paths.rustBinary,
    args,
    env: sandbox.env
  };

  const rustCold = runCommand(rustCommand);

  if (options.legacyJs) {
    const jsCommand = {
      label: "Legacy JS",
      command: process.execPath,
      args: [paths.jsEntry, ...args],
      env: sandbox.env
    };
    const jsCold = runCommand(jsCommand);
    const diffs = compareRuns(benchCase, jsCold, rustCold);
    if (diffs.length > 0) {
      throw new Error(
        [
          `Correctness failed for benchmark case ${benchCase.id}; no performance report was produced.`,
          ...diffs.slice(0, MAX_DIFFS).map((item) => formatDiff(item))
        ].join("\n")
      );
    }

    const jsWarm = runWarmCommands(jsCommand, options.runs);
    const rustWarm = runWarmCommands(rustCommand, options.runs);
    const result = {
      id: benchCase.id,
      groups: benchCase.groups,
      compare: benchCase.compare,
      argsTemplate: benchCase.args,
      correctness: "passed",
      mode: "legacy-js-comparison",
      js: summarizeTiming(jsCold, jsWarm),
      rust: summarizeTiming(rustCold, rustWarm)
    };

    result.ratio = buildRatio(result.js, result.rust);

    if (benchCase.includeLegacyPoc && options.legacyPoc) {
      result.legacyPoc = runLegacyPoc(benchCase, args, rustCold, result.rust, sandbox, paths, options);
    }

    return result;
  }

  const diffs = validateRustRun(benchCase, rustCold);
  if (diffs.length > 0) {
    throw new Error(
      [
        `Correctness failed for benchmark case ${benchCase.id}; no performance report was produced.`,
        ...diffs.slice(0, MAX_DIFFS).map((item) => formatDiff(item))
      ].join("\n")
    );
  }

  const rustWarm = runWarmCommands(rustCommand, options.runs);
  const result = {
    id: benchCase.id,
    groups: benchCase.groups,
    compare: benchCase.compare,
    argsTemplate: benchCase.args,
    correctness: "passed",
    mode: "rust-only",
    rust: summarizeTiming(rustCold, rustWarm)
  };

  if (benchCase.includeLegacyPoc && options.legacyPoc) {
    result.legacyPoc = runLegacyPoc(benchCase, args, rustCold, result.rust, sandbox, paths, options);
  }

  return result;
}

function runLegacyPoc(benchCase, args, rustCold, rustTiming, sandbox, paths, options) {
  const command = {
    label: "Legacy POC",
    command: paths.legacyPocBinary,
    args,
    env: sandbox.env
  };
  const cold = runCommand(command);
  const diffs = compareRuns(benchCase, rustCold, cold);
  if (diffs.length > 0) {
    return {
      status: "failed",
      diffs: diffs.slice(0, MAX_DIFFS).map(redactDiff)
    };
  }

  const warm = runWarmCommands(command, options.runs);
  const timing = summarizeTiming(cold, warm);
  return {
    status: "passed",
    ...timing,
    ratioVsRust: buildRatio(rustTiming, timing)
  };
}

function validateRustRun(benchCase, rust) {
  const diffs = [];
  if (rust.status !== 0) {
    diffs.push(diff("exitCode", 0, rust.status));
  }
  if (rust.signal !== null) {
    diffs.push(diff("signal", null, rust.signal));
  }
  if (diffs.length > 0) {
    return diffs;
  }

  switch (benchCase.compare) {
    case "json":
      return validateJsonOutput(rust.stdout);
    case "table":
      return parsePlainTable(rust.stdout).length > 0
        ? []
        : [diff("stdout.table.rows", "non-empty table", 0)];
    case "text":
      return normalizeText(rust.stdout) !== ""
        ? []
        : [diff("stdout.text", "non-empty text", "")];
    default:
      return [diff("compare", benchCase.compare, "known compare mode")];
  }
}

function validateJsonOutput(stdout) {
  try {
    JSON.parse(stdout);
    return [];
  } catch (error) {
    return [diff("json.parse.rust", "parseable JSON", error.message)];
  }
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

function runWarmCommands(command, runs) {
  const durations = [];
  for (let index = 0; index < runs; index += 1) {
    durations.push(runCommand(command).durationMs);
  }
  return durations;
}

function runCommand(command) {
  const started = performance.now();
  const result = spawnSync(command.command, command.args, {
    cwd: repoRoot,
    encoding: "utf8",
    env: command.env,
    maxBuffer: 256 * 1024 * 1024
  });
  const durationMs = performance.now() - started;

  if (result.error !== undefined) {
    throw new Error(`${command.label} command failed to start: ${result.error.message}`);
  }

  return {
    label: command.label,
    status: result.status,
    signal: result.signal,
    stdout: result.stdout ?? "",
    stderr: result.stderr ?? "",
    durationMs
  };
}

function compareRuns(benchCase, js, rust) {
  const diffs = [];
  if (js.status !== rust.status) {
    diffs.push(diff("exitCode", js.status, rust.status));
  }
  if (js.signal !== rust.signal) {
    diffs.push(diff("signal", js.signal, rust.signal));
  }
  if (diffs.length > 0) {
    return diffs;
  }

  switch (benchCase.compare) {
    case "json":
      return compareJson(js.stdout, rust.stdout);
    case "table":
      return compareRows(parsePlainTable(js.stdout), parsePlainTable(rust.stdout), "table");
    case "text":
      return normalizeText(js.stdout) === normalizeText(rust.stdout)
        ? []
        : [diff("stdout.text", js.stdout, rust.stdout)];
    default:
      return [diff("compare", benchCase.compare, "known compare mode")];
  }
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

  if (jsType === "number" && isEquivalentJsonNumber(path, jsValue, rustValue)) {
    return [];
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

function isEquivalentJsonNumber(path, jsValue, rustValue) {
  if (!path.endsWith(".credits") && !path.endsWith(".usd")) {
    return false;
  }
  return Math.abs(jsValue - rustValue) <= 1e-5;
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
      if (jsRow[column] !== rustRow[column] && !isEquivalentFormattedNumber(jsRow[column], rustRow[column])) {
        diffs.push(diff(`${label}.row[${row}].column[${column}]`, jsRow[column], rustRow[column]));
      }
      if (diffs.length >= MAX_DIFFS) {
        return diffs;
      }
    }
  }
  return diffs;
}

function isEquivalentFormattedNumber(left, right) {
  const leftNumber = parseFormattedNumber(left);
  const rightNumber = parseFormattedNumber(right);
  if (leftNumber === undefined || rightNumber === undefined) {
    return false;
  }
  return Math.abs(leftNumber - rightNumber) <= 0.01;
}

function parseFormattedNumber(value) {
  if (typeof value !== "string" || value.trim() === "") {
    return undefined;
  }
  const normalized = value.replaceAll(",", "").replace(/^\$/, "");
  if (!/^-?\d+(?:\.\d+)?$/.test(normalized)) {
    return undefined;
  }
  const number = Number(normalized);
  return Number.isFinite(number) ? number : undefined;
}

function normalizeText(value) {
  return value
    .split(/\r?\n/)
    .map((line) => line.trim().replace(/\s+/g, " "))
    .filter(Boolean)
    .join("\n");
}

function diff(path, js, rust) {
  return { path, js, rust };
}

function redactDiff(item) {
  return {
    path: item.path,
    js: summarizeValue(item.js),
    rust: summarizeValue(item.rust)
  };
}

function summarizeValue(value) {
  if (typeof value === "string") {
    return `<string length=${value.length}>`;
  }
  if (Array.isArray(value)) {
    return `<array length=${value.length}>`;
  }
  if (value !== null && typeof value === "object") {
    return `<object keys=${Object.keys(value).length}>`;
  }
  return value;
}

function formatDiff(item) {
  const redacted = redactDiff(item);
  return `- ${redacted.path}: JS=${JSON.stringify(redacted.js)} Rust=${JSON.stringify(redacted.rust)}`;
}

function summarizeTiming(cold, warmDurations) {
  const warm = warmDurations.length > 0 ? timingStats(warmDurations) : undefined;
  return {
    coldMs: roundMs(cold.durationMs),
    warmRunsMs: warmDurations.map(roundMs),
    warm: warm === undefined ? undefined : mapStats(warm),
    stdoutBytes: Buffer.byteLength(cold.stdout),
    stderrBytes: Buffer.byteLength(cold.stderr),
    exitCode: cold.status
  };
}

function buildRatio(js, rust) {
  const jsMedian = js.warm?.medianMs ?? js.coldMs;
  const rustMedian = rust.warm?.medianMs ?? rust.coldMs;
  return {
    medianSpeedup: roundRatio(jsMedian / rustMedian),
    coldSpeedup: roundRatio(js.coldMs / rust.coldMs)
  };
}

function timingStats(values) {
  const sorted = [...values].sort((left, right) => left - right);
  const sum = values.reduce((total, value) => total + value, 0);
  const middle = Math.floor(sorted.length / 2);
  const median =
    sorted.length % 2 === 0 ? (sorted[middle - 1] + sorted[middle]) / 2 : sorted[middle];

  return {
    min: sorted[0],
    median,
    mean: sum / values.length,
    max: sorted.at(-1)
  };
}

function mapStats(stats) {
  return {
    minMs: roundMs(stats.min),
    medianMs: roundMs(stats.median),
    meanMs: roundMs(stats.mean),
    maxMs: roundMs(stats.max)
  };
}

function roundMs(value) {
  return Number(value.toFixed(2));
}

function roundRatio(value) {
  return Number(value.toFixed(2));
}

async function summarizeSessions(sessionsDir) {
  const files = await listJsonlFiles(sessionsDir);
  let lines = 0;
  let invalidJsonLines = 0;
  let tokenCountEvents = 0;
  let includedUsageEvents = 0;

  for (const file of files) {
    const content = await readFile(file, "utf8");
    let previousTotal;
    for (const line of content.split(/\r?\n/)) {
      if (line.trim() === "") {
        continue;
      }
      lines += 1;
      if (!line.includes("\"token_count\"")) {
        continue;
      }
      let event;
      try {
        event = JSON.parse(line);
      } catch {
        invalidJsonLines += 1;
        continue;
      }
      const payload = event?.payload;
      if (event?.type !== "event_msg" || payload?.type !== "token_count") {
        continue;
      }
      tokenCountEvents += 1;
      const info = payload.info ?? {};
      const total = readUsage(info.total_token_usage);
      const usage = readUsage(info.last_token_usage) ?? diffUsage(total, previousTotal);
      if (total !== undefined) {
        previousTotal = total;
      }
      if (usage !== undefined && !isEmptyUsage(usage)) {
        includedUsageEvents += 1;
      }
    }
  }

  return {
    files: files.length,
    lines,
    invalidJsonLines,
    tokenCountEvents,
    includedUsageEvents
  };
}

async function listJsonlFiles(root) {
  const entries = await readDirectoryRecursive(root);
  return entries.filter((path) => path.endsWith(".jsonl")).sort();
}

async function readDirectoryRecursive(root) {
  const { readdir } = await import("node:fs/promises");
  const entries = [];
  const names = await readdir(root, { withFileTypes: true });
  for (const entry of names) {
    const path = join(root, entry.name);
    if (entry.isDirectory()) {
      entries.push(...(await readDirectoryRecursive(path)));
    } else if (entry.isFile()) {
      entries.push(path);
    }
  }
  return entries;
}

function readUsage(value) {
  if (value === undefined || value === null || typeof value !== "object") {
    return undefined;
  }
  return {
    inputTokens: readNumber(value.input_tokens),
    cachedInputTokens: readNumber(value.cached_input_tokens),
    outputTokens: readNumber(value.output_tokens),
    reasoningOutputTokens: readNumber(value.reasoning_output_tokens),
    totalTokens: readNumber(value.total_tokens)
  };
}

function readNumber(value) {
  return typeof value === "number" && Number.isFinite(value) ? value : 0;
}

function diffUsage(current, previous) {
  if (current === undefined || previous === undefined) {
    return undefined;
  }
  return {
    inputTokens: Math.max(0, current.inputTokens - previous.inputTokens),
    cachedInputTokens: Math.max(0, current.cachedInputTokens - previous.cachedInputTokens),
    outputTokens: Math.max(0, current.outputTokens - previous.outputTokens),
    reasoningOutputTokens: Math.max(0, current.reasoningOutputTokens - previous.reasoningOutputTokens),
    totalTokens: Math.max(0, current.totalTokens - previous.totalTokens)
  };
}

function isEmptyUsage(usage) {
  return (
    usage.inputTokens === 0 &&
    usage.cachedInputTokens === 0 &&
    usage.outputTokens === 0 &&
    usage.reasoningOutputTokens === 0 &&
    usage.totalTokens === 0
  );
}

function buildReport(options, paths, sandbox, scale, comparison, results) {
  return {
    tool: "bench-rust-run",
    generatedAt: new Date().toISOString(),
    dataset: {
      mode: sandbox.mode,
      fixture: reportPath(paths.fixture),
      sessionsDir: paths.sessionsDir === undefined ? "fixture codex-home/sessions" : reportPath(paths.sessionsDir),
      privacy: options.snapshotSessions
        ? "Real sessions are copied to a temporary snapshot. Raw JSONL content, stdout, and stderr are not stored in this report."
        : "Sessions are read directly from the provided directory. Raw JSONL content, stdout, and stderr are not stored in this report.",
      scale
    },
    environment: {
      platform: process.platform,
      arch: process.arch,
      node: process.version,
      rustc: commandVersion("rustc", ["--version"]),
      cargo: commandVersion("cargo", ["--version"]),
      cwd: repoRoot
    },
    binaries: {
      jsEntry: options.legacyJs ? reportPath(paths.jsEntry) : undefined,
      rustBinary: reportPath(paths.rustBinary),
      legacyPocBinary: options.legacyPoc ? reportPath(paths.legacyPocBinary) : undefined,
      buildType: "release"
    },
    correctness: {
      comparisonHarness: comparison,
      benchmarkCommands: options.legacyJs
        ? "passed JS/Rust cold output comparison"
        : "passed Rust cold output validation"
    },
    config: {
      warmRuns: options.runs,
      fixedNow: FIXED_NOW,
      matrix: options.matrix,
      snapshotSessions: options.snapshotSessions,
      skipHarness: options.skipHarness,
      legacyJs: options.legacyJs,
      legacyPoc: options.legacyPoc
    },
    results
  };
}

function commandVersion(command, args) {
  const result = spawnSync(command, args, { encoding: "utf8" });
  if (result.error !== undefined || result.status !== 0) {
    return undefined;
  }
  return result.stdout.trim();
}

function printSummary(report) {
  console.log("Rust run benchmark: PASS");
  console.log(
    [
      `Dataset: ${report.dataset.mode}`,
      `files=${formatInteger(report.dataset.scale.files)}`,
      `lines=${formatInteger(report.dataset.scale.lines)}`,
      `token_count_events=${formatInteger(report.dataset.scale.tokenCountEvents)}`,
      `included_usage_events=${formatInteger(report.dataset.scale.includedUsageEvents)}`
    ].join(" ")
  );
  if (report.correctness.comparisonHarness.summary !== undefined) {
    const summary = report.correctness.comparisonHarness.summary;
    console.log(
      `Correctness: comparison ${summary.total} total, ${summary.passed} passed, ${summary.pending} pending, ${summary.failed} failed; benchmark commands passed`
    );
  } else if (report.correctness.comparisonHarness.status === "skipped") {
    console.log("Correctness: comparison harness skipped; benchmark commands passed cold output comparison");
  } else if (report.correctness.comparisonHarness.status === "not-run") {
    console.log("Correctness: Rust-only cold output validation passed; legacy JS comparison not run");
  } else {
    console.log("Correctness: comparison passed; benchmark commands passed");
  }
  console.log(`Warm runs: ${report.config.warmRuns}`);
  console.log("");

  const timingLabel = report.config.warmRuns === 0 ? "cold" : "median";
  const rows = report.config.legacyJs
    ? [
        ["Case", `JS ${timingLabel}`, `Rust ${timingLabel}`, "Speedup", "JS cold", "Rust cold"],
        ...report.results.map((result) => [
          result.id,
          formatMs(result.js.warm?.medianMs ?? result.js.coldMs),
          formatMs(result.rust.warm?.medianMs ?? result.rust.coldMs),
          `${result.ratio.medianSpeedup.toFixed(2)}x`,
          formatMs(result.js.coldMs),
          formatMs(result.rust.coldMs)
        ])
      ]
    : [
        ["Case", `Rust ${timingLabel}`, "Rust cold", "stdout", "stderr"],
        ...report.results.map((result) => [
          result.id,
          formatMs(result.rust.warm?.medianMs ?? result.rust.coldMs),
          formatMs(result.rust.coldMs),
          formatInteger(result.rust.stdoutBytes),
          formatInteger(result.rust.stderrBytes)
        ])
      ];
  console.log(formatTable(rows));

  const legacy = report.results.find((result) => result.legacyPoc !== undefined)?.legacyPoc;
  if (legacy?.status === "passed") {
    console.log("");
    console.log(
      `Legacy POC stat-json-poc: ${timingLabel}=${formatMs(legacy.warm?.medianMs ?? legacy.coldMs)} speedup_vs_rust=${legacy.ratioVsRust.medianSpeedup.toFixed(2)}x`
    );
  }
}

async function writeReport(report, output) {
  const outputPath = resolveOutputPath(output, report.dataset.mode);
  await mkdir(dirname(outputPath), { recursive: true });
  await writeFile(outputPath, `${JSON.stringify(report, null, 2)}\n`);
  console.log("");
  console.log(`JSON report: ${reportPath(outputPath)}`);
}

function resolveOutputPath(output, mode) {
  if (output !== undefined) {
    return resolveInputPath(output);
  }
  const suffix = mode === "real-sessions" ? "real" : "fixture";
  return resolve(repoRoot, `task/rust-run-bench-${suffix}-report.json`);
}

function reportPath(path) {
  const relative = path.startsWith(`${repoRoot}/`) ? path.slice(repoRoot.length + 1) : path;
  return relative;
}

function formatTable(rows) {
  const widths = rows[0].map((_cell, column) =>
    Math.max(...rows.map((row) => String(row[column] ?? "").length))
  );
  return rows
    .map((row, index) => {
      const line = row
        .map((cell, column) => String(cell ?? "").padEnd(widths[column]))
        .join("  ");
      if (index === 0) {
        return `${line}\n${widths.map((width) => "-".repeat(width)).join("  ")}`;
      }
      return line;
    })
    .join("\n");
}

function formatMs(value) {
  return value === undefined ? "n/a" : `${value.toFixed(2)} ms`;
}

function formatInteger(value) {
  return new Intl.NumberFormat("en-US").format(value);
}

function summarizeProcessOutput(result) {
  const stdout = result.stdout?.trim();
  const stderr = result.stderr?.trim();
  const lines = [];
  if (stdout) {
    lines.push(`stdout: ${redactLongText(stdout)}`);
  }
  if (stderr) {
    lines.push(`stderr: ${redactLongText(stderr)}`);
  }
  return lines.join("\n");
}

function redactLongText(value) {
  const normalized = value.replace(/\s+/g, " ").trim();
  return normalized.length > 500 ? `${normalized.slice(0, 497)}...` : normalized;
}

function shellToken(value) {
  return value.includes(" ") ? JSON.stringify(value) : value;
}
