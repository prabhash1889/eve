#!/usr/bin/env node
// Build the CPU and/or CUDA installers for ONE version bump, into:
//   build/<version>/cpu/{msi,nsis}    (local-whisper + local-parakeet)
//   build/<version>/cuda/{msi,nsis}   (local-whisper-cuda + local-parakeet)
//
// Usage:
//   node scripts/build.mjs            BOTH variants, bump patch
//   node scripts/build.mjs 0.3.0      BOTH variants, set version 0.3.0
//   node scripts/build.mjs --cpu      CPU only
//   node scripts/build.mjs --cuda     CUDA only
// (npm: `npm run build:all`, `npm run build:cpu`, `npm run build:cuda`;
//  append `-- 0.3.0` to pin a version.)
//
// CPU  -> runs on ANY machine (Groq cloud + Parakeet on CPU; whisper on CPU).
// CUDA -> GPU whisper on NVIDIA. Needs the CUDA toolchain - run from an x64 MSVC
//         prompt. Both are UNSIGNED (no signing key, no in-app auto-update);
//         Windows SmartScreen shows "More info -> Run anyway" the first time.

import { spawnSync } from "node:child_process";
import {
  readFileSync,
  writeFileSync,
  existsSync,
  mkdirSync,
  readdirSync,
  copyFileSync,
  rmSync,
  statSync,
} from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { tmpdir } from "node:os";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const argv = process.argv.slice(2);
// Optional explicit version (x.y.z); anything else falls through to a patch bump.
const versionArg = argv.find((a) => /^\d+\.\d+\.\d+$/.test(a));

// Which variants to build. Default (no flag) is BOTH, CPU first so a missing
// CUDA toolchain still leaves you a working CPU installer.
const wantCpu = argv.includes("--cpu");
const wantCuda = argv.includes("--cuda");
const variants = wantCpu === wantCuda ? ["cpu", "cuda"] : wantCpu ? ["cpu"] : ["cuda"];

const FEATURES = {
  cpu: "local-whisper,local-parakeet",
  cuda: "local-whisper-cuda,local-parakeet",
};

// Installer subfolders Tauri writes, by host OS.
const BUNDLE_TARGETS = {
  win32: ["msi", "nsis"],
  darwin: ["dmg", "macos"],
  linux: ["deb", "rpm", "appimage"],
};
const targets = BUNDLE_TARGETS[process.platform] ?? ["msi", "nsis"];

function run(cmd, args, extraEnv) {
  const res = spawnSync(cmd, args, {
    cwd: root,
    stdio: "inherit",
    shell: process.platform === "win32", // npx/node resolution needs a shell on Windows
    env: { ...process.env, ...extraEnv },
  });
  if (res.status !== 0) process.exit(res.status ?? 1);
}

// Move the installers Tauri just built out of src-tauri into
// build/<version>/<variant>/, so nothing is left behind in src-tauri.
function collect(version, variant) {
  const bundleDir = join(root, "src-tauri", "target", "release", "bundle");
  for (const target of targets) {
    const src = join(bundleDir, target);
    if (!existsSync(src)) continue;
    const dest = join(root, "build", version, variant, target);
    mkdirSync(dest, { recursive: true });
    for (const file of readdirSync(src)) {
      const from = join(src, file);
      if (statSync(from).isDirectory()) continue; // skip Tauri's staging dirs
      copyFileSync(from, join(dest, file));
      rmSync(from); // move, not copy: leave nothing in src-tauri
    }
  }
}

// 1. Bump the version ONCE (explicit x.y.z if given, else patch). Both variants
//    share this single version.
run("node", [join("scripts", "bump-version.mjs"), versionArg || "patch"]);
const version = JSON.parse(
  readFileSync(join(root, "package.json"), "utf8")
).version;

// 2. Unsigned build: turn off updater artifacts (they'd otherwise demand a
//    signing key). A temp FILE, not inline JSON, so the shell can't mangle it.
const noupd = join(tmpdir(), "eve-no-updater.json");
writeFileSync(noupd, JSON.stringify({ bundle: { createUpdaterArtifacts: false } }));

// CUDA GPU coverage: sm_89 (RTX 40) + sm_120 (RTX 50) real kernels plus sm_89
// PTX so newer NVIDIA GPUs JIT-compile at first run. Override with CUDAARCHS=120
// for a faster single-GPU build. This wins over the (non-forced) CUDAARCHS in
// src-tauri/.cargo/config.toml.
const cudaArchs = process.env.CUDAARCHS || "89-real;120-real;89-virtual";

// 3. Build each variant and move its installers into its own subfolder.
for (const variant of variants) {
  console.log(`\n=== Building ${variant.toUpperCase()} v${version} ===`);
  run(
    "npx",
    ["tauri", "build", "--features", FEATURES[variant], "--config", noupd],
    variant === "cuda" ? { CUDAARCHS: cudaArchs } : undefined
  );
  collect(version, variant);
  console.log(`  -> build/${version}/${variant}/`);
}

console.log(
  `\nDone. v${version} installers in build/${version}/{${variants.join(",")}}/`
);
