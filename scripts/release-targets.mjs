// npm platform package target metadata used by release packaging helpers.
// This file must not contain CLI business behavior or validation matrices.

export const releaseTargets = [
  {
    target: "linux-x64-gnu",
    rustTarget: "x86_64-unknown-linux-gnu",
    packageName: "codex-ops-linux-x64-gnu",
    os: ["linux"],
    cpu: ["x64"],
    libc: ["glibc"],
    binaryName: "codex-ops"
  },
  {
    target: "linux-arm64-gnu",
    rustTarget: "aarch64-unknown-linux-gnu",
    packageName: "codex-ops-linux-arm64-gnu",
    os: ["linux"],
    cpu: ["arm64"],
    libc: ["glibc"],
    binaryName: "codex-ops"
  },
  {
    target: "darwin-x64",
    rustTarget: "x86_64-apple-darwin",
    packageName: "codex-ops-darwin-x64",
    os: ["darwin"],
    cpu: ["x64"],
    binaryName: "codex-ops"
  },
  {
    target: "darwin-arm64",
    rustTarget: "aarch64-apple-darwin",
    packageName: "codex-ops-darwin-arm64",
    os: ["darwin"],
    cpu: ["arm64"],
    binaryName: "codex-ops"
  },
  {
    target: "win32-x64-msvc",
    rustTarget: "x86_64-pc-windows-msvc",
    packageName: "codex-ops-win32-x64-msvc",
    os: ["win32"],
    cpu: ["x64"],
    binaryName: "codex-ops.exe"
  }
];

export function targetByName(targetName) {
  const target = releaseTargets.find((entry) => entry.target === targetName);

  if (target === undefined) {
    throw new Error(`Unknown release target: ${targetName}`);
  }

  return target;
}

export function currentReleaseTarget() {
  const platform = process.platform;
  const arch = process.arch;

  if (platform === "linux") {
    return targetByName(`linux-${arch}-${detectLinuxLibc()}`);
  }

  if (platform === "darwin") {
    return targetByName(`darwin-${arch}`);
  }

  if (platform === "win32") {
    return targetByName(`win32-${arch}-msvc`);
  }

  throw new Error(`Unsupported release target: ${platform}-${arch}`);
}

export function optionalDependencyMap(version) {
  return Object.fromEntries(releaseTargets.map((target) => [target.packageName, version]));
}

function detectLinuxLibc() {
  try {
    const report = process.report?.getReport?.();
    if (report?.header?.glibcVersionRuntime) {
      return "gnu";
    }
  } catch {
    return "gnu";
  }

  return "musl";
}
