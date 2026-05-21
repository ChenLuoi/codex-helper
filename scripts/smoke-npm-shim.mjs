#!/usr/bin/env node
// npm shim smoke only. Keep assertions limited to shim forwarding, exit-code
// propagation, and install-error behavior; CLI behavior belongs in Rust tests.

import { spawnSync } from "node:child_process";
import { accessSync, closeSync, constants, mkdtempSync, openSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const shimPath = join(repoRoot, "bin", "codex-ops.js");
const binaryName = process.platform === "win32" ? "codex-ops.exe" : "codex-ops";
const shimTestEnvNames = [
  "CODEX_OPS_SHIM_TEST_PLATFORM",
  "CODEX_OPS_SHIM_TEST_ARCH",
  "CODEX_OPS_SHIM_TEST_LIBC"
];
const rustBinary = resolve(
  repoRoot,
  process.env.CODEX_OPS_RUST_BINARY ?? join("target", "release", binaryName)
);

try {
  accessSync(rustBinary, constants.X_OK);
} catch {
  console.error(`Missing release binary: ${rustBinary}`);
  console.error("Run `rtk cargo build --release` before this smoke test.");
  process.exit(1);
}

const cases = [
  {
    name: "root help",
    args: ["--help"],
    expectedStatus: 0,
    stdoutIncludes: "Usage: codex-ops <command> [options]"
  },
  {
    name: "stat help",
    args: ["stat", "--help"],
    expectedStatus: 0,
    stdoutIncludes: "Usage: codex-ops stat [view] [session] [options]"
  },
  {
    name: "nonzero exit code",
    args: ["__codex_ops_unknown__"],
    expectedStatus: 2,
    stderrIncludes: "error: unrecognized subcommand '__codex_ops_unknown__'"
  },
  {
    name: "missing binary",
    args: ["--help"],
    env: {
      CODEX_OPS_RUST_BINARY: join(repoRoot, "target", "release", "__missing_codex_ops__")
    },
    expectedStatus: 127,
    stderrIncludes: "codex-ops: unable to find the Rust binary."
  },
  {
    name: "linux musl uses static package target",
    args: ["--help"],
    skipRustBinaryOverride: true,
    env: {
      CODEX_OPS_SHIM_TEST_PLATFORM: "linux",
      CODEX_OPS_SHIM_TEST_ARCH: "x64",
      CODEX_OPS_SHIM_TEST_LIBC: "musl"
    },
    expectedStatus: 127,
    stderrIncludes: [
      "codex-ops: unable to find the Rust binary.",
      "target: linux-x64",
      "codex-ops-linux-x64-bin"
    ]
  }
];

for (const testCase of cases) {
  const result = runShim(testCase);

  assertEqual(result.status, testCase.expectedStatus, `${testCase.name} exit status`, result);

  if (testCase.stdoutIncludes !== undefined) {
    assertIncludes(result.stdout, testCase.stdoutIncludes, `${testCase.name} stdout`, result);
  }

  if (testCase.stderrIncludes !== undefined) {
    for (const expected of arrayOf(testCase.stderrIncludes)) {
      assertIncludes(result.stderr, expected, `${testCase.name} stderr`, result);
    }
  }
}

console.log("npm shim smoke passed");

function assertEqual(actual, expected, label, result) {
  if (actual === expected) {
    return;
  }

  fail(`${label}: expected ${expected}, got ${actual}`, result);
}

function assertIncludes(actual, expected, label, result) {
  if (actual.includes(expected)) {
    return;
  }

  fail(`${label}: expected to include ${JSON.stringify(expected)}`, result);
}

function arrayOf(value) {
  return Array.isArray(value) ? value : [value];
}

function runShim(testCase) {
  const tempDir = mkdtempSync(join(tmpdir(), "codex-ops-shim-smoke-"));
  const stdoutPath = join(tempDir, "stdout");
  const stderrPath = join(tempDir, "stderr");
  const stdoutFd = openSync(stdoutPath, "w");
  const stderrFd = openSync(stderrPath, "w");

  try {
    const result = spawnSync(process.execPath, [shimPath, ...testCase.args], {
      cwd: repoRoot,
      env: testEnv(testCase),
      stdio: ["ignore", stdoutFd, stderrFd]
    });

    closeSync(stdoutFd);
    closeSync(stderrFd);

    return {
      ...result,
      stdout: readFileSync(stdoutPath, "utf8"),
      stderr: readFileSync(stderrPath, "utf8")
    };
  } finally {
    try {
      closeSync(stdoutFd);
    } catch {
      // Already closed.
    }
    try {
      closeSync(stderrFd);
    } catch {
      // Already closed.
    }
    rmSync(tempDir, { recursive: true, force: true });
  }
}

function testEnv(testCase) {
  const env = { ...process.env };

  for (const name of shimTestEnvNames) {
    delete env[name];
  }

  if (!testCase.skipRustBinaryOverride) {
    env.CODEX_OPS_RUST_BINARY = rustBinary;
  } else {
    delete env.CODEX_OPS_RUST_BINARY;
  }

  return {
    ...env,
    ...testCase.env
  };
}

function fail(message, result) {
  console.error(message);
  console.error("--- stdout ---");
  console.error(result.stdout);
  console.error("--- stderr ---");
  console.error(result.stderr);
  process.exit(1);
}
