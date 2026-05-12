import pc from "picocolors";

export type ProjectSummary = {
  name: string;
  packageManager: string;
  createdAt: string;
};

export function createProjectSummary(
  name: string,
  packageManager = "npm",
  now = new Date()
): ProjectSummary {
  return {
    name: name.trim() || "codex-helper",
    packageManager,
    createdAt: now.toISOString()
  };
}

export function formatProjectSummary(summary: ProjectSummary): string {
  return [
    `${pc.bold("Project")}: ${pc.cyan(summary.name)}`,
    `${pc.bold("Package manager")}: ${summary.packageManager}`,
    `${pc.bold("Created at")}: ${summary.createdAt}`
  ].join("\n");
}
