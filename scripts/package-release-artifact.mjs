#!/usr/bin/env node
// Release artifact staging helper. Keep this script limited to staging the
// already-built Rust binary, platform package files, manifests, and sums.

import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { chmodSync, copyFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { releaseBinaryFileName, targetByName } from "./release-targets.mjs";

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)));

if (process.argv[1] !== undefined && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const args = parseArgs(process.argv.slice(2));
  const result = stageReleaseArtifact(args);
  console.log(JSON.stringify(result, null, 2));
}

export function stageReleaseArtifact(options) {
  const target = targetByName(required(options.target, "--target"));
  const rustTarget = options.rustTarget ?? target.rustTarget;
  const packageJson = readJson(join(repoRoot, "package.json"));
  const binaryPath = resolve(
    repoRoot,
    options.binary ?? join("target", rustTarget, "release", target.binaryName)
  );
  const outputDir = resolve(repoRoot, options.outputDir ?? join("dist", "release"));
  const binaryAssetDir = join(outputDir, "binary-assets");
  const binaryAsset = join(binaryAssetDir, releaseBinaryFileName(target, packageJson.version));
  const npmPackageDir = join(outputDir, "npm", target.packageName);
  const relativeBinary = ["bin", target.binaryName].join("/");
  const commit = process.env.GITHUB_SHA ?? gitCommitOrUnknown();
  const manifest = {
    package: "codex-ops",
    version: packageJson.version,
    platformTarget: target.target,
    rustTarget,
    npmPackage: target.packageName,
    binary: relativeBinary,
    commit
  };

  mkdirSync(binaryAssetDir, { recursive: true });
  mkdirSync(join(outputDir, "npm-tarballs"), { recursive: true });
  copyFileSync(binaryPath, binaryAsset);

  mkdirSync(join(npmPackageDir, "bin"), { recursive: true });
  copyFileSync(join(repoRoot, "npm", target.target, "package.json"), join(npmPackageDir, "package.json"));
  copyFileSync(binaryPath, join(npmPackageDir, relativeBinary));
  writeFileSync(join(npmPackageDir, "README.md"), platformReadme(target, packageJson.version));
  writeFileSync(join(npmPackageDir, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  writeFileSync(join(npmPackageDir, "SHA256SUMS"), checksumFile(npmPackageDir, [relativeBinary, "manifest.json"]));

  if (!target.binaryName.endsWith(".exe")) {
    chmodSync(binaryAsset, 0o755);
    chmodSync(join(npmPackageDir, relativeBinary), 0o755);
  }

  return {
    binaryAsset,
    npmPackageDir,
    target: target.target,
    rustTarget,
    packageName: target.packageName,
    version: packageJson.version
  };
}

function parseArgs(argv) {
  const parsed = {};

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];

    if (arg === "--target") {
      parsed.target = argv[index + 1];
      index += 1;
      continue;
    }

    if (arg === "--rust-target") {
      parsed.rustTarget = argv[index + 1];
      index += 1;
      continue;
    }

    if (arg === "--binary") {
      parsed.binary = argv[index + 1];
      index += 1;
      continue;
    }

    if (arg === "--output-dir") {
      parsed.outputDir = argv[index + 1];
      index += 1;
      continue;
    }

    throw new Error(`Unknown argument: ${arg}`);
  }

  return parsed;
}

function required(value, label) {
  if (value === undefined || value === "") {
    throw new Error(`Missing ${label}`);
  }

  return value;
}

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function checksumFile(root, paths) {
  return `${paths
    .map((path) => `${sha256(join(root, path))}  ${path.split("\\").join("/")}`)
    .join("\n")}\n`;
}

function sha256(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function platformReadme(target, version) {
  return [
    `# ${target.packageName}`,
    "",
    `Prebuilt codex-ops ${version} Rust binary for ${target.target}.`,
    "",
    "This package is installed as an optional dependency by the main `codex-ops` npm package.",
    "It contains no JavaScript business logic."
  ].join("\n");
}

function gitCommitOrUnknown() {
  try {
    const result = spawnSync("git", ["rev-parse", "HEAD"], {
      cwd: repoRoot,
      encoding: "utf8"
    });

    if (result.status === 0) {
      return result.stdout.trim();
    }
  } catch {
    // Keep release packaging usable in source archives without .git.
  }

  return "unknown";
}
