#!/usr/bin/env node

import { confirm, input, select } from "@inquirer/prompts";
import { Command } from "commander";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import ora from "ora";
import pc from "picocolors";
import {
  createProjectSummary,
  formatProjectSummary,
  type ProjectSummary
} from "./index.js";

const packageVersion = readPackageVersion();

export function createProgram(options: { output?: NodeJS.WritableStream } = {}) {
  const output = options.output ?? process.stdout;

  const program = new Command()
    .name("codex-helper")
    .description("Command line helpers for Codex-oriented development workflows.")
    .version(packageVersion)
    .option("-v, --verbose", "show more details");

  program
    .command("doctor")
    .description("Run a quick local environment check.")
    .action(async () => {
      const spinner = ora("Checking local environment").start();
      await new Promise((resolve) => setTimeout(resolve, 150));
      spinner.succeed("Local environment looks ready.");
      output.write(`${pc.green("Node.js")} ${process.version}\n`);
    });

  program
    .command("init")
    .description("Collect basic project preferences interactively.")
    .option("-n, --name <name>", "project name")
    .option("-p, --package-manager <manager>", "package manager")
    .option("--yes", "accept defaults and skip prompts")
    .action(async (commandOptions: {
      name?: string;
      packageManager?: string;
      yes?: boolean;
    }) => {
      const summary = await collectProjectSummary(commandOptions);
      output.write(`${formatProjectSummary(summary)}\n`);
    });

  return program;
}

export async function collectProjectSummary(options: {
  name?: string;
  packageManager?: string;
  yes?: boolean;
}): Promise<ProjectSummary> {
  if (options.yes) {
    return createProjectSummary(
      options.name ?? "codex-helper",
      options.packageManager ?? "npm"
    );
  }

  const name =
    options.name ??
    (await input({
      message: "Project name",
      default: "codex-helper"
    }));

  const packageManager =
    options.packageManager ??
    (await select({
      message: "Package manager",
      choices: [
        { name: "npm", value: "npm" },
        { name: "pnpm", value: "pnpm" },
        { name: "yarn", value: "yarn" }
      ],
      default: "npm"
    }));

  const shouldContinue = await confirm({
    message: "Generate project summary?",
    default: true
  });

  if (!shouldContinue) {
    throw new Error("Initialization cancelled.");
  }

  return createProjectSummary(name, packageManager);
}

export async function run(argv = process.argv) {
  await createProgram().parseAsync(argv);
}

function isMainModule() {
  return (
    process.argv[1] !== undefined &&
    fileURLToPath(import.meta.url) === resolve(process.argv[1])
  );
}

function readPackageVersion() {
  try {
    const packageJson = JSON.parse(
      readFileSync(new URL("../package.json", import.meta.url), "utf8")
    ) as { version?: unknown };

    return typeof packageJson.version === "string" ? packageJson.version : "0.0.0";
  } catch {
    return "0.0.0";
  }
}

function handleError(error: unknown) {
  const message = error instanceof Error ? error.message : String(error);
  console.error(pc.red(message));
  process.exitCode = 1;
}

if (isMainModule()) {
  run().catch(handleError);
}
