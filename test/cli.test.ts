import { Writable } from "node:stream";
import { describe, expect, it } from "vitest";
import { collectProjectSummary, createProgram } from "../src/cli.js";

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
    expect(output.toString()).toContain("init");
  });

  it("collects defaults without interactive prompts", async () => {
    const summary = await collectProjectSummary({ yes: true });

    expect(summary.name).toBe("codex-helper");
    expect(summary.packageManager).toBe("npm");
  });
});
