#!/usr/bin/env node
// GitHub Release asset assembler. Keep this script limited to collecting the
// already-built release artifacts, npm tarballs, crate archive, manifests, and
// checksums into one upload directory.

import { createHash } from "node:crypto";
import { copyFileSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { basename, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname as pathDirname } from "node:path";
import { npmPackFileName, releaseTargets } from "./release-targets.mjs";

const repoRoot = pathDirname(pathDirname(fileURLToPath(import.meta.url)));
const packageJson = readJson(join(repoRoot, "package.json"));
const args = parseArgs(process.argv.slice(2));
const inputDir = resolve(repoRoot, args.inputDir ?? "dist/downloaded");
const outputDir = resolve(repoRoot, args.outputDir ?? "dist/release-assets");
const version = args.version ?? packageJson.version;
const commit = process.env.GITHUB_SHA ?? gitCommitOrUnknown();

rmSync(outputDir, { recursive: true, force: true });
mkdirSync(outputDir, { recursive: true });

const discovered = walk(inputDir);
const assets = [];

for (const target of releaseTargets) {
  const archiveName = `codex-ops-${version}-${target.target}.tar.gz`;
  const archivePath = copyRequiredFile(discovered, archiveName, outputDir);
  assets.push(assetEntry("binary", archivePath, target));

  const tarballName = npmPackFileName(target.packageName, version);
  const tarballPath = copyRequiredFile(discovered, tarballName, outputDir);
  assets.push(assetEntry("npm-platform", tarballPath, target));
}

const mainTarball = copyRequiredFile(discovered, `codex-ops-${version}.tgz`, outputDir);
assets.push(assetEntry("npm-main", mainTarball));

const crateArchive = copyRequiredFile(discovered, `codex-ops-${version}.crate`, outputDir);
assets.push(assetEntry("crate", crateArchive));

const releaseManifestPath = join(outputDir, "release-manifest.json");
writeFileSync(
  releaseManifestPath,
  `${JSON.stringify(
    {
      package: "codex-ops",
      version,
      tag: `v${version}`,
      commit,
      assets
    },
    null,
    2
  )}\n`
);

const checksumEntries = [
  ...assets.map((asset) => asset.fileName),
  basename(releaseManifestPath)
].sort();

writeFileSync(
  join(outputDir, "SHA256SUMS"),
  `${checksumEntries
    .map((fileName) => `${sha256(join(outputDir, fileName))}  ${fileName}`)
    .join("\n")}\n`
);

console.log(`assembled ${checksumEntries.length} release assets in ${outputDir}`);

function parseArgs(argv) {
  const parsed = {};

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];

    if (arg === "--input-dir") {
      parsed.inputDir = argv[index + 1];
      index += 1;
      continue;
    }

    if (arg === "--output-dir") {
      parsed.outputDir = argv[index + 1];
      index += 1;
      continue;
    }

    if (arg === "--version") {
      parsed.version = argv[index + 1];
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

function walk(root) {
  const entries = [];

  for (const entry of readdirSync(root, { withFileTypes: true })) {
    const path = join(root, entry.name);
    entries.push({
      path,
      name: entry.name,
      isDirectory: entry.isDirectory(),
      isFile: entry.isFile()
    });

    if (entry.isDirectory()) {
      entries.push(...walk(path));
    }
  }

  return entries;
}

function copyRequiredFile(entries, fileName, destinationDir) {
  const matches = entries.filter((entry) => entry.isFile && entry.name === fileName);

  if (matches.length !== 1) {
    throw new Error(`Expected exactly one file named ${fileName}, found ${matches.length}`);
  }

  const destination = join(destinationDir, fileName);
  copyFileSync(matches[0].path, destination);
  return destination;
}

function assetEntry(kind, path, target) {
  const stats = statSync(path);

  return {
    kind,
    fileName: basename(path),
    bytes: stats.size,
    sha256: sha256(path),
    target: target?.target,
    npmPackage: target?.packageName,
    rustTarget: target?.rustTarget
  };
}

function sha256(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
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
    // Keep asset assembly usable in source archives without .git.
  }

  return "unknown";
}
