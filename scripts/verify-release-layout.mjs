#!/usr/bin/env node
// npm release layout dry-run. Keep this script focused on npm pack/install
// behavior and shim-to-platform-binary resolution.

import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import { currentReleaseTarget, npmPackFileName } from "./release-targets.mjs";
import { stageReleaseArtifact } from "./package-release-artifact.mjs";

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const args = parseArgs(process.argv.slice(2));
const target = currentReleaseTarget();
const binary = resolve(repoRoot, args.binary ?? join("target", "release", target.binaryName));
const tempRoot = mkdtempSync(join(tmpdir(), "codex-ops-release-"));
const packDir = join(tempRoot, "packs");
const installDir = join(tempRoot, "install");
const npmCacheDir = join(tempRoot, "npm-cache");
const packageJson = JSON.parse(readFileSync(join(repoRoot, "package.json"), "utf8"));

mkdirSync(packDir, { recursive: true });
mkdirSync(npmCacheDir, { recursive: true });

run(process.execPath, ["scripts/check-release.mjs", "--binary", binary], { cwd: repoRoot });

const staged = stageReleaseArtifact({
  target: target.target,
  rustTarget: target.rustTarget,
  binary,
  outputDir: join(tempRoot, "staged")
});

run("npm", ["pack", "--pack-destination", packDir], { cwd: repoRoot, env: npmEnv(process.env) });
run("npm", ["pack", staged.npmPackageDir, "--pack-destination", packDir], {
  cwd: repoRoot,
  env: npmEnv(process.env)
});

const mainPack = findExactPack(packDir, `codex-ops-${packageJson.version}.tgz`);
const platformPack = findExactPack(packDir, npmPackFileName(target.packageName, packageJson.version));

run(
  "npm",
  [
    "install",
    "--prefix",
    installDir,
    join(packDir, mainPack),
    join(packDir, platformPack),
    "--omit=optional",
    "--ignore-scripts",
    "--no-audit",
    "--no-fund",
    "--offline"
  ],
  { cwd: repoRoot, env: npmEnv(process.env) }
);

const installedBin = join(installDir, "node_modules", ".bin", process.platform === "win32" ? "codex-ops.cmd" : "codex-ops");
run(installedBin, ["--version"], {
  cwd: repoRoot,
  env: withoutOverride(process.env)
});
run(installedBin, ["--help"], {
  cwd: repoRoot,
  env: withoutOverride(process.env)
});

console.log(`release layout dry-run passed for ${target.target}`);
console.log(`staged artifact: ${staged.artifactDir}`);
console.log(`staged npm package: ${staged.npmPackageDir}`);

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

function run(command, args, options) {
  const result = spawnSync(command, args, {
    ...options,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"]
  });

  if (result.status === 0) {
    return result;
  }

  throw new Error(
    [
      `Command failed: ${command} ${args.join(" ")}`,
      `exit: ${result.status}`,
      "--- stdout ---",
      result.stdout,
      "--- stderr ---",
      result.stderr
    ].join("\n")
  );
}

function findExactPack(directory, fileName) {
  const matches = readdirSync(directory).filter((file) => file === fileName);

  if (matches.length !== 1) {
    throw new Error(`Expected ${fileName} in ${directory}, found ${matches.length}`);
  }

  return matches[0];
}

function withoutOverride(env) {
  const next = { ...env };
  delete next.CODEX_OPS_RUST_BINARY;
  return next;
}

function npmEnv(env) {
  return {
    ...env,
    npm_config_cache: npmCacheDir
  };
}
