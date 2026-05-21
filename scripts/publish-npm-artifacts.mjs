#!/usr/bin/env node
// npm release artifact publisher. Keep this script limited to publishing the
// already-packed npm tarballs produced by the release workflow.

import { spawnSync } from "node:child_process";
import { readdirSync, readFileSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { npmPackFileName, releaseTargets } from "./release-targets.mjs";

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const packageJson = readJson(join(repoRoot, "package.json"));
const args = parseArgs(process.argv.slice(2));
const inputDir = resolve(repoRoot, args.inputDir ?? "dist/downloaded");
const version = args.version ?? packageJson.version;
const distTag = args.tag ?? "latest";
const dryRun = args.dryRun ?? false;
const discoveredFiles = walkFiles(inputDir);

for (const target of releaseTargets) {
  const tarball = findRequiredFile(npmPackFileName(target.packageName, version));
  publishIfNeeded({
    packageName: target.packageName,
    version,
    tarball,
    access: "public",
    tag: distTag,
    dryRun
  });
}

publishIfNeeded({
  packageName: packageJson.name,
  version,
  tarball: findRequiredFile(`${packageJson.name}-${version}.tgz`),
  tag: distTag,
  dryRun
});

function parseArgs(argv) {
  const parsed = {};

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];

    if (arg === "--input-dir") {
      parsed.inputDir = argv[index + 1];
      index += 1;
      continue;
    }

    if (arg === "--version") {
      parsed.version = argv[index + 1];
      index += 1;
      continue;
    }

    if (arg === "--tag") {
      parsed.tag = argv[index + 1];
      index += 1;
      continue;
    }

    if (arg === "--dry-run") {
      parsed.dryRun = true;
      continue;
    }

    throw new Error(`Unknown argument: ${arg}`);
  }

  return parsed;
}

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function walkFiles(root) {
  const files = [];

  for (const entry of readdirSync(root, { withFileTypes: true })) {
    const path = join(root, entry.name);

    if (entry.isDirectory()) {
      files.push(...walkFiles(path));
    } else if (entry.isFile()) {
      files.push(path);
    }
  }

  return files;
}

function findRequiredFile(fileName) {
  const matches = discoveredFiles.filter((path) => basename(path) === fileName);

  if (matches.length !== 1) {
    throw new Error(`Expected exactly one file named ${fileName}, found ${matches.length}`);
  }

  return matches[0];
}

function publishIfNeeded({ packageName, version, tarball, access, tag, dryRun }) {
  const spec = `${packageName}@${version}`;
  const view = spawnSync("npm", ["view", spec, "version", "--registry", "https://registry.npmjs.org"], {
    encoding: "utf8"
  });

  if (view.status === 0 && view.stdout.trim() === version) {
    console.log(`${spec} already exists on npm; skipping ${basename(tarball)}`);
    return;
  }

  const lookupOutput = `${view.stdout}\n${view.stderr}`;
  if (view.status !== 0 && !lookupOutput.includes("E404")) {
    throw new Error(
      [
        `Failed to check npm package ${spec}`,
        "--- stdout ---",
        view.stdout,
        "--- stderr ---",
        view.stderr
      ].join("\n")
    );
  }

  const publishArgs = ["publish", tarball, "--tag", tag];

  if (access !== undefined) {
    publishArgs.push("--access", access);
  }

  if (dryRun) {
    publishArgs.push("--dry-run");
  }

  console.log(`publishing ${spec} from ${tarball}`);
  const publish = spawnSync("npm", publishArgs, {
    encoding: "utf8",
    stdio: "inherit"
  });

  if (publish.status !== 0) {
    throw new Error(`Failed to publish ${spec} from ${tarball}`);
  }
}
