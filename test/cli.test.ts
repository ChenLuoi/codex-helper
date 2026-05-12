import { mkdtemp, rm, symlink } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Writable } from "node:stream";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { createProgram, isMainModule } from "../src/cli.js";

class MemoryStream extends Writable {
  chunks: string[] = [];

  _write(
    chunk: Buffer | string,
    _encoding: BufferEncoding,
    callback: (error?: Error | null) => void
  ) {
    this.chunks.push(chunk.toString());
    callback();
  }

  toString() {
    return this.chunks.join("");
  }
}

describe("cli", () => {
  it("prints help text", async () => {
    const output = new MemoryStream();
    const program = createProgram({ output });

    program.exitOverride();
    program.configureOutput({
      writeOut: (text) => output.write(text),
      writeErr: (text) => output.write(text)
    });

    try {
      await program.parseAsync(["node", "codex-helper", "--help"]);
    } catch {
      // Commander throws after printing help when exitOverride is enabled.
    }

    expect(output.toString()).toContain("codex-helper");
    expect(output.toString()).toContain("doctor");
    expect(output.toString()).not.toContain("init");
    expect(output.toString()).toContain("stat");
  });

  it("prints stat help text", async () => {
    const program = createProgram();
    const statCommand = program.commands.find((command) => command.name() === "stat");
    const helpText = statCommand?.helpInformation() ?? "";

    expect(helpText).toContain("stat [options] [view] [session]");
    expect(helpText).toContain("--format");
    expect(helpText).toContain("--today");
    expect(helpText).toContain("hour, day, week, month");
    expect(helpText).toContain("--sort");
    expect(helpText).toContain("--limit");
    expect(helpText).toContain("--verbose");
    expect(helpText).toContain("--top");
  });

  it("recognizes npm bin symlink execution as the main module", async () => {
    const tempDir = await mkdtemp(join(tmpdir(), "codex-helper-cli-"));
    const cliPath = fileURLToPath(new URL("../src/cli.ts", import.meta.url));
    const binPath = join(tempDir, "codex-helper");

    try {
      await symlink(cliPath, binPath);

      expect(isMainModule(binPath)).toBe(true);
    } finally {
      await rm(tempDir, { force: true, recursive: true });
    }
  });
});
