// npm platform package target metadata used by release packaging helpers.
// This file must not contain CLI business behavior or validation matrices.

export const releaseTargets = [
  {
    target: "linux-x64-gnu",
    rustTarget: "x86_64-unknown-linux-gnu",
    packageName: "codex-ops-linux-x64-bin",
    os: ["linux"],
    cpu: ["x64"],
    libc: ["glibc"],
    binaryName: "codex-ops"
  },
  {
    target: "linux-arm64-gnu",
    rustTarget: "aarch64-unknown-linux-gnu",
    packageName: "codex-ops-linux-arm64-bin",
    os: ["linux"],
    cpu: ["arm64"],
    libc: ["glibc"],
    binaryName: "codex-ops"
  },
  {
    target: "darwin-x64",
    rustTarget: "x86_64-apple-darwin",
    packageName: "codex-ops-macos-x64-bin",
    os: ["darwin"],
    cpu: ["x64"],
    binaryName: "codex-ops"
  },
  {
    target: "darwin-arm64",
    rustTarget: "aarch64-apple-darwin",
    packageName: "codex-ops-macos-arm64-bin",
    os: ["darwin"],
    cpu: ["arm64"],
    binaryName: "codex-ops"
  },
  {
    target: "win32-x64-msvc",
    rustTarget: "x86_64-pc-windows-msvc",
    packageName: "codex-ops-windows-x64-bin",
    os: ["win32"],
    cpu: ["x64"],
    binaryName: "codex-ops.exe"
  }
];

export const expectedReleaseTargetNames = [
  "linux-x64-gnu",
  "linux-arm64-gnu",
  "darwin-x64",
  "darwin-arm64",
  "win32-x64-msvc"
];

export const unsupportedRuntimeTargets = [
  "linux-x64-musl",
  "linux-arm64-musl",
  "win32-arm64-msvc"
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
    const libc = detectLinuxLibc();
    const target = `linux-${arch}-${libc}`;

    if (libc === "musl") {
      throw new Error(
        `Unsupported release target: ${target}. Alpine/musl is not supported; supported Linux targets are linux-x64-gnu and linux-arm64-gnu.`
      );
    }

    return targetByName(target);
  }

  if (platform === "darwin") {
    return targetByName(`darwin-${arch}`);
  }

  if (platform === "win32") {
    const target = `win32-${arch}-msvc`;

    if (arch === "arm64") {
      throw new Error(
        `Unsupported release target: ${target}. Supported Windows target is win32-x64-msvc.`
      );
    }

    return targetByName(target);
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
