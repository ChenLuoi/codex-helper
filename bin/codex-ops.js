#!/usr/bin/env node

import { spawn } from "node:child_process";
import { accessSync, constants } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const packageRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const requireFromPackage = createRequire(import.meta.url);
const overrideEnv = "CODEX_OPS_RUST_BINARY";
const binaryName = process.platform === "win32" ? "codex-ops.exe" : "codex-ops";
const forwardedSignals = ["SIGHUP", "SIGINT", "SIGTERM"];

const lookup = findBinary();

if (!lookup.ok) {
  printLookupError(lookup);
  process.exit(lookup.exitCode);
}

const child = spawn(lookup.path, process.argv.slice(2), {
  stdio: "inherit",
  windowsHide: false
});

let settled = false;
const signalHandlers = new Map();

for (const signal of forwardedSignals) {
  const handler = () => {
    if (!child.killed) {
      child.kill(signal);
    }
  };
  signalHandlers.set(signal, handler);
  process.on(signal, handler);
}

child.on("error", (error) => {
  if (settled) {
    return;
  }
  settled = true;
  removeSignalHandlers();
  printSpawnError(error, lookup);
  process.exit(spawnErrorExitCode(error));
});

child.on("close", (code, signal) => {
  if (settled) {
    return;
  }
  settled = true;
  removeSignalHandlers();

  if (signal !== null) {
    process.exit(signalExitCode(signal));
  }

  process.exit(code ?? 1);
});

function findBinary() {
  const override = process.env[overrideEnv];

  if (override !== undefined && override.trim() !== "") {
    const overridePath = resolve(process.cwd(), override);
    const check = checkExecutable(overridePath);

    if (check.ok) {
      return {
        ok: true,
        path: overridePath,
        source: overrideEnv,
        candidates: [formatCandidate(overridePath, overrideEnv)]
      };
    }

    return {
      ok: false,
      kind: check.reason === "not-executable" ? "permission" : "missing",
      exitCode: check.reason === "not-executable" ? 126 : 127,
      target: describeCurrentPlatform(),
      candidates: [formatCandidate(overridePath, overrideEnv)],
      optionalPackages: [],
      override: overridePath
    };
  }

  const target = resolveTarget();

  if (!target.ok) {
    return {
      ok: false,
      kind: "unsupported",
      exitCode: 1,
      target: describeCurrentPlatform(),
      candidates: [],
      optionalPackages: [],
      reason: target.reason
    };
  }

  const candidates = buildCandidates(target.value);
  const notExecutable = [];

  for (const candidate of candidates) {
    const check = checkExecutable(candidate.path);

    if (check.ok) {
      return {
        ok: true,
        path: candidate.path,
        source: candidate.label,
        candidates
      };
    }

    if (check.reason === "not-executable") {
      notExecutable.push(candidate);
    }
  }

  return {
    ok: false,
    kind: notExecutable.length > 0 ? "permission" : "missing",
    exitCode: notExecutable.length > 0 ? 126 : 127,
    target: target.value,
    candidates,
    optionalPackages: optionalPackageNames(target.value)
  };
}

function buildCandidates(target) {
  const candidates = [
    formatCandidate(join(packageRoot, "bin", target, binaryName), `package bin/${target}`),
    formatCandidate(join(packageRoot, "npm", target, "bin", binaryName), `package npm/${target}`)
  ];

  for (const packageName of optionalPackageNames(target)) {
    let packageJsonPath;

    try {
      packageJsonPath = requireFromPackage.resolve(`${packageName}/package.json`, {
        paths: [packageRoot]
      });
    } catch {
      continue;
    }

    const packageDir = dirname(packageJsonPath);
    candidates.push(formatCandidate(join(packageDir, "bin", binaryName), packageName));
    candidates.push(formatCandidate(join(packageDir, binaryName), packageName));
  }

  return candidates;
}

function optionalPackageNames(target) {
  return [`@codex-ops/${target}`, `codex-ops-${target}`];
}

function resolveTarget() {
  const { arch, platform } = process;

  if (platform === "darwin") {
    if (arch === "x64" || arch === "arm64") {
      return { ok: true, value: `darwin-${arch}` };
    }
    return { ok: false, reason: `unsupported macOS architecture: ${arch}` };
  }

  if (platform === "linux") {
    if (arch === "x64" || arch === "arm64") {
      return { ok: true, value: `linux-${arch}-${detectLinuxLibc()}` };
    }
    return { ok: false, reason: `unsupported Linux architecture: ${arch}` };
  }

  if (platform === "win32") {
    if (arch === "x64" || arch === "arm64") {
      return { ok: true, value: `win32-${arch}-msvc` };
    }
    return { ok: false, reason: `unsupported Windows architecture: ${arch}` };
  }

  return { ok: false, reason: `unsupported platform: ${platform}` };
}

function detectLinuxLibc() {
  try {
    const report = process.report?.getReport?.();
    if (report?.header?.glibcVersionRuntime) {
      return "gnu";
    }
  } catch {
    return "gnu";
  }

  return "musl";
}

function checkExecutable(path) {
  try {
    accessSync(path, constants.X_OK);
    return { ok: true };
  } catch {
    try {
      accessSync(path, constants.F_OK);
      return { ok: false, reason: "not-executable" };
    } catch {
      return { ok: false, reason: "missing" };
    }
  }
}

function formatCandidate(path, label) {
  return { path, label };
}

function printLookupError(result) {
  const headline =
    result.kind === "unsupported"
      ? "codex-ops: unsupported platform for the bundled Rust binary."
      : result.kind === "permission"
        ? "codex-ops: found the Rust binary, but it is not executable."
        : "codex-ops: unable to find the Rust binary.";

  const lines = [
    headline,
    `platform: ${process.platform}`,
    `arch: ${process.arch}`,
    `target: ${result.target}`,
    `${overrideEnv}: ${result.override ?? process.env[overrideEnv] ?? "<unset>"}`
  ];

  if (result.reason !== undefined) {
    lines.push(`reason: ${result.reason}`);
  }

  if (result.candidates.length > 0) {
    lines.push("searched paths:");
    for (const candidate of result.candidates) {
      lines.push(`  - ${candidate.path} (${candidate.label})`);
    }
  }

  if (result.optionalPackages.length > 0) {
    lines.push("optional package candidates:");
    for (const packageName of result.optionalPackages) {
      lines.push(`  - ${packageName}`);
    }
  }

  lines.push(`set ${overrideEnv}=/path/to/codex-ops to run a local Rust build.`);
  console.error(lines.join("\n"));
}

function printSpawnError(error, lookupResult) {
  const code = error && typeof error === "object" && "code" in error ? error.code : "unknown";

  console.error(
    [
      "codex-ops: failed to execute the Rust binary.",
      `binary: ${lookupResult.path}`,
      `source: ${lookupResult.source}`,
      `error: ${code}`,
      `set ${overrideEnv}=/path/to/codex-ops to run a local Rust build.`
    ].join("\n")
  );
}

function spawnErrorExitCode(error) {
  if (error && typeof error === "object" && "code" in error) {
    if (error.code === "EACCES") {
      return 126;
    }

    if (error.code === "ENOENT") {
      return 127;
    }
  }

  return 1;
}

function signalExitCode(signal) {
  const signalNumbers = {
    SIGHUP: 1,
    SIGINT: 2,
    SIGTERM: 15
  };

  return 128 + (signalNumbers[signal] ?? 1);
}

function removeSignalHandlers() {
  for (const [signal, handler] of signalHandlers) {
    process.off(signal, handler);
  }
}

function describeCurrentPlatform() {
  if (process.platform === "linux") {
    return `linux-${process.arch}-${detectLinuxLibc()}`;
  }

  return `${process.platform}-${process.arch}`;
}
