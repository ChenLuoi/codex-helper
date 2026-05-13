import { chmod, mkdir, rename, unlink, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import { dirname, join } from "node:path";

export type CodexHomeOptions = {
  codexHome?: string;
};

export function defaultCodexHome() {
  return process.env.CODEX_HOME ?? join(homedir(), ".codex");
}

export function resolveCodexHelperDir(options: CodexHomeOptions = {}) {
  return join(options.codexHome ?? defaultCodexHome(), "codex-helper");
}

export async function ensurePrivateDirectory(dir: string) {
  await mkdir(dir, { recursive: true, mode: 0o700 });
  await chmodBestEffort(dir, 0o700);
}

export async function writeSensitiveFile(filePath: string, content: string) {
  await ensurePrivateDirectory(dirname(filePath));

  const tempFile = join(
    dirname(filePath),
    `.${encodeURIComponent(`${Date.now()}-${process.pid}`)}.${encodeURIComponent(
      basenameForTemp(filePath)
    )}.tmp`
  );

  try {
    await writeFile(tempFile, content, { mode: 0o600 });
    await chmodBestEffort(tempFile, 0o600);
    await rename(tempFile, filePath);
    await chmodBestEffort(filePath, 0o600);
  } catch (error) {
    await unlink(tempFile).catch(() => undefined);
    throw error;
  }
}

async function chmodBestEffort(path: string, mode: number) {
  await chmod(path, mode).catch(() => undefined);
}

function basenameForTemp(filePath: string) {
  const parts = filePath.split(/[\\/]/);
  return parts.at(-1) ?? "codex-helper";
}
