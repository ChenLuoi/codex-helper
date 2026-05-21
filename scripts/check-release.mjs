#!/usr/bin/env node
// npm/Cargo release metadata guard. Keep this script limited to package,
// platform manifest, optional dependency, and binary version checks.

import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { dirname } from "node:path";
import {
  expectedReleaseTargetNames,
  optionalDependencyMap,
  releaseTargets,
  unsupportedOptionalDependencyNames
} from "./release-targets.mjs";

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const args = parseArgs(process.argv.slice(2));
const packageJson = readJson(join(repoRoot, "package.json"));
const packageLock = readJson(join(repoRoot, "package-lock.json"));
const cargoToml = readFileSync(join(repoRoot, "Cargo.toml"), "utf8");
const cargoVersion = matchTomlString(cargoToml, "version");
const cargoName = matchTomlString(cargoToml, "name");
const cargoPublish = matchTomlValue(cargoToml, "publish");
const cargoPackageFiles = readCargoPackageList();
const npmProjectMetadata = {
  homepage: "https://github.com/ChenLuoi/codex-ops#readme",
  bugs: {
    url: "https://github.com/ChenLuoi/codex-ops/issues"
  },
  repository: {
    type: "git",
    url: "git+https://github.com/ChenLuoi/codex-ops.git"
  }
};

assertEqual(packageJson.name, "codex-ops", "package name");
assertEqual(cargoName, "codex-ops", "Cargo package name");
assertEqual(packageJson.version, cargoVersion, "package/Cargo version");
assertNpmProjectMetadata(packageJson, "package");
assertEqual(packageJson.bin?.["codex-ops"], "bin/codex-ops.js", "npm bin");
assertEqual(
  packageJson.files,
  [
    "bin/**/*.js",
    "bin/*/codex-ops",
    "bin/*/codex-ops.exe",
    "npm/**/bin/codex-ops",
    "npm/**/bin/codex-ops.exe",
    "README.md"
  ],
  "npm main package files"
);
assertAbsent(packageJson.main, "package main");
assertAbsent(packageJson.types, "package types");
assertAbsent(packageJson.exports, "package exports");

if (cargoPublish === "false") {
  throw new Error("Cargo.toml still has publish = false; release workflow cannot publish the crate.");
}

assertCargoPackageIncludes(cargoPackageFiles, [
  "Cargo.lock",
  "Cargo.toml",
  "data/codex-rate-card.json",
  "README.md",
  "src/main.rs",
  "src/lib.rs"
]);
assertCargoPackageExcludes(cargoPackageFiles, [
  ".github/",
  ".gitignore",
  ".ignore",
  "AGENTS.md",
  "bin/",
  "justfile",
  "npm/",
  "package-lock.json",
  "package.json",
  "scripts/",
  "src/bin/",
  "task/",
  "test/",
  "tests/"
]);

assertJsonEqual(
  packageJson.optionalDependencies,
  optionalDependencyMap(packageJson.version),
  "package optionalDependencies"
);
assertEqual(
  releaseTargets.map((target) => target.target),
  expectedReleaseTargetNames,
  "release targets"
);
assertNoUnsupportedOptionalDependencies(packageJson.optionalDependencies, unsupportedOptionalDependencyNames);
assertEqual(packageLock.name, packageJson.name, "package-lock name");
assertEqual(packageLock.version, packageJson.version, "package-lock version");
assertJsonEqual(
  packageLock.packages?.[""]?.optionalDependencies,
  packageJson.optionalDependencies,
  "package-lock optionalDependencies"
);

for (const target of releaseTargets) {
  const manifest = readJson(join(repoRoot, "npm", target.target, "package.json"));
  assertEqual(manifest.name, target.packageName, `${target.target} package name`);
  assertEqual(manifest.version, packageJson.version, `${target.target} package version`);
  assertNpmProjectMetadata(manifest, `${target.target} package`);
  assertEqual(manifest.publishConfig, { access: "public" }, `${target.target} publishConfig`);
  assertEqual(manifest.os, target.os, `${target.target} os`);
  assertEqual(manifest.cpu, target.cpu, `${target.target} cpu`);

  if (target.libc === undefined) {
    assertAbsent(manifest.libc, `${target.target} libc`);
  } else {
    assertEqual(manifest.libc, target.libc, `${target.target} libc`);
  }

  assertEqual(manifest.files, ["bin/**", "manifest.json", "SHA256SUMS", "README.md"], `${target.target} files`);
}

if (args.binary !== undefined) {
  const versionResult = spawnSync(args.binary, ["--version"], {
    cwd: repoRoot,
    encoding: "utf8"
  });

  if (versionResult.status !== 0) {
    throw new Error(
      `Failed to run ${args.binary} --version\nstdout:\n${versionResult.stdout}\nstderr:\n${versionResult.stderr}`
    );
  }

  assertEqual(versionResult.stdout.trim(), packageJson.version, "binary --version");
}

console.log(`release metadata check passed for codex-ops ${packageJson.version}`);

function parseArgs(argv) {
  const parsed = {};

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];

    if (arg === "--binary") {
      parsed.binary = argv[index + 1];
      index += 1;
      continue;
    }

    throw new Error(`Unknown argument: ${arg}`);
  }

  return parsed;
}

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function readCargoPackageList() {
  const result = spawnSync("cargo", ["package", "--list", "--allow-dirty", "--quiet"], {
    cwd: repoRoot,
    encoding: "utf8"
  });

  if (result.status !== 0) {
    throw new Error(
      `Failed to list Cargo package contents\nstdout:\n${result.stdout}\nstderr:\n${result.stderr}`
    );
  }

  if (result.stderr.trim() !== "") {
    throw new Error(`Cargo package list wrote stderr:\n${result.stderr}`);
  }

  return result.stdout
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
}

function matchTomlString(toml, key) {
  const match = toml.match(new RegExp(`^${key}\\s*=\\s*"([^"]+)"`, "m"));

  if (match === null) {
    throw new Error(`Missing Cargo.toml ${key}`);
  }

  return match[1];
}

function matchTomlValue(toml, key) {
  const match = toml.match(new RegExp(`^${key}\\s*=\\s*(.+)$`, "m"));
  return match?.[1]?.trim();
}

function assertAbsent(value, label) {
  if (value !== undefined) {
    throw new Error(`${label}: expected to be absent`);
  }
}

function assertEqual(actual, expected, label) {
  if (stableStringify(actual) !== stableStringify(expected)) {
    throw new Error(`${label}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
  }
}

function assertJsonEqual(actual, expected, label) {
  assertEqual(actual, expected, label);
}

function assertNpmProjectMetadata(manifest, label) {
  assertEqual(manifest.homepage, npmProjectMetadata.homepage, `${label} homepage`);
  assertEqual(manifest.bugs, npmProjectMetadata.bugs, `${label} bugs`);
  assertEqual(manifest.repository, npmProjectMetadata.repository, `${label} repository`);
}

function assertCargoPackageIncludes(files, expectedPaths) {
  for (const expectedPath of expectedPaths) {
    if (!files.includes(expectedPath)) {
      throw new Error(`Cargo package contents: expected ${expectedPath} to be included`);
    }
  }
}

function assertCargoPackageExcludes(files, excludedPaths) {
  for (const excludedPath of excludedPaths) {
    const found = files.find((file) => file === excludedPath || file.startsWith(excludedPath));
    if (found !== undefined) {
      throw new Error(`Cargo package contents: expected ${excludedPath} to be excluded, found ${found}`);
    }
  }
}

function assertNoUnsupportedOptionalDependencies(optionalDependencies, unsupportedPackageNames) {
  const packageNames = Object.keys(optionalDependencies ?? {});

  for (const packageName of unsupportedPackageNames) {
    if (packageNames.includes(packageName)) {
      throw new Error(`${packageName}: unsupported runtime target must not be an optionalDependency`);
    }
  }
}

function stableStringify(value) {
  if (Array.isArray(value)) {
    return `[${value.map((entry) => stableStringify(entry)).join(",")}]`;
  }

  if (value !== null && typeof value === "object") {
    return `{${Object.keys(value)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`)
      .join(",")}}`;
  }

  return JSON.stringify(value);
}
