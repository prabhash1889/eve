#!/usr/bin/env node
// Local release build. Bumps the version, builds an UNSIGNED installer for the
// chosen variant, and (via tauri.mjs) moves the installers into
//   <repo>/build/<version>/        (CPU)
//   <repo>/build/<version>/cuda/   (CUDA)
// so nothing is left inside src-tauri.
//
// Usage:
//   node scripts/build.mjs                 CPU build,  bump patch
//   node scripts/build.mjs --cuda          CUDA build, bump patch
//   node scripts/build.mjs 0.3.0           CPU build,  set version 0.3.0
//   node scripts/build.mjs --cuda 0.3.0    CUDA build, set version 0.3.0
// (via npm: `npm run build:cpu`, `npm run build:cuda`, or
//  `npm run build:cuda -- 0.3.0` to pin a version.)
//
// Variants:
//   CPU  (local-whisper + local-parakeet) - runs on ANY machine. Groq cloud +
//        Parakeet on CPU; whisper on CPU (slow for large models).
//   CUDA (local-whisper-cuda + local-parakeet) - GPU whisper on NVIDIA. Needs
//        the CUDA toolchain (see src-tauri/.cargo/config.toml). Runs only on
//        NVIDIA machines with a matching CUDA runtime.
//
// Both are UNSIGNED (no signing key, no in-app auto-update). Windows SmartScreen
// shows "More info -> Run anyway" the first time.

import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { tmpdir } from "node:os";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const argv = process.argv.slice(2);
const cuda = argv.includes("--cuda");
// Optional explicit version (x.y.z); anything else falls through to a patch bump.
const versionArg = argv.find((a) => /^\d+\.\d+\.\d+$/.test(a));

function run(cmd, args, extraEnv) {
  const res = spawnSync(cmd, args, {
    cwd: root,
    stdio: "inherit",
    shell: process.platform === "win32", // npx/node resolution needs a shell on Windows
    env: { ...process.env, ...extraEnv },
  });
  if (res.status !== 0) process.exit(res.status ?? 1);
}

// 1. Bump the version in lockstep (explicit x.y.z if given, else patch).
run("node", [join("scripts", "bump-version.mjs"), versionArg || "patch"]);
const version = JSON.parse(
  readFileSync(join(root, "package.json"), "utf8")
).version;

// 2. Unsigned build: turn off updater artifacts (they'd otherwise demand a
//    signing key). A temp FILE, not inline JSON, so the shell can't mangle it.
const noupd = join(tmpdir(), "eve-no-updater.json");
writeFileSync(noupd, JSON.stringify({ bundle: { createUpdaterArtifacts: false } }));

const features = cuda
  ? "local-whisper-cuda,local-parakeet"
  : "local-whisper,local-parakeet";

// CUDA GPU coverage. sm_89 (RTX 40) + sm_120 (RTX 50) real kernels for speed,
// plus sm_89 PTX so newer NVIDIA GPUs JIT-compile at first run. Override with
// e.g. CUDAARCHS=120 for a faster single-GPU build. This process env wins over
// the (non-forced) CUDAARCHS in src-tauri/.cargo/config.toml.
const cudaEnv = cuda
  ? { CUDAARCHS: process.env.CUDAARCHS || "89-real;120-real;89-virtual" }
  : undefined;

// 3. Build + move (tauri.mjs runs `tauri build` then relocates the installers;
//    it routes the CUDA variant into build/<version>/cuda/).
run(
  "node",
  [
    join("scripts", "tauri.mjs"),
    "build",
    "--features",
    features,
    "--config",
    noupd,
  ],
  cudaEnv
);

console.log(
  `\nBuilt v${version} (${features}) -> build/${version}/${cuda ? "cuda/" : ""}`
);
