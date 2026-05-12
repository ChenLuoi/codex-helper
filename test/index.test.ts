import { describe, expect, it } from "vitest";
import { createProjectSummary, formatProjectSummary } from "../src/index.js";

describe("project summary", () => {
  it("uses defaults for an empty project name", () => {
    const summary = createProjectSummary("  ", "npm", new Date("2026-01-01T00:00:00.000Z"));

    expect(summary).toEqual({
      name: "codex-helper",
      packageManager: "npm",
      createdAt: "2026-01-01T00:00:00.000Z"
    });
  });

  it("formats a readable summary", () => {
    const summary = createProjectSummary("demo", "pnpm", new Date("2026-01-01T00:00:00.000Z"));

    expect(formatProjectSummary(summary)).toContain("demo");
    expect(formatProjectSummary(summary)).toContain("pnpm");
  });
});
