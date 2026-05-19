#!/usr/bin/env node
// npm shim smoke only. Keep assertions limited to shim forwarding, exit-code
// propagation, and install-error behavior; CLI behavior belongs in Rust tests.

import { spawnSync } from "node:child_process";
import { accessSync, constants } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const shimPath = join(repoRoot, "bin", "codex-ops.js");
const binaryName = process.platform === "win32" ? "codex-ops.exe" : "codex-ops";
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
    stderrIncludes: "error: Unknown command: __codex_ops_unknown__"
  },
  {
    name: "missing binary",
    args: ["--help"],
    env: {
      CODEX_OPS_RUST_BINARY: join(repoRoot, "target", "release", "__missing_codex_ops__")
    },
    expectedStatus: 127,
    stderrIncludes: "codex-ops: unable to find the Rust binary."
  }
];

for (const testCase of cases) {
  const result = spawnSync(process.execPath, [shimPath, ...testCase.args], {
    cwd: repoRoot,
    env: {
      ...process.env,
      CODEX_OPS_RUST_BINARY: rustBinary,
      ...testCase.env
    },
    encoding: "utf8"
  });

  assertEqual(result.status, testCase.expectedStatus, `${testCase.name} exit status`, result);

  if (testCase.stdoutIncludes !== undefined) {
    assertIncludes(result.stdout, testCase.stdoutIncludes, `${testCase.name} stdout`, result);
  }

  if (testCase.stderrIncludes !== undefined) {
    assertIncludes(result.stderr, testCase.stderrIncludes, `${testCase.name} stderr`, result);
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

function fail(message, result) {
  console.error(message);
  console.error("--- stdout ---");
  console.error(result.stdout);
  console.error("--- stderr ---");
  console.error(result.stderr);
  process.exit(1);
}
