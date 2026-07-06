#!/usr/bin/env node
// Build signed release bundles, then copy the MSI + NSIS artifacts into
//   <repo>/build/msi  and  <repo>/build/nsis
// The signing private key is loaded from the file at TAURI_KEY_PATH
// (default: ~/.tauri/eve.key) so it never has to live in the repo.
//
// Usage: node scripts/release.mjs             (CPU build: signed, updater channel)
//        node scripts/release.mjs --cuda      (CUDA build: power-user artifact)
//        node scripts/release.mjs --copy-only (skip build, just copy existing)
//        (--cuda --copy-only copies into the cuda/ subfolder)
//
// Build matrix (parity Phase B): the CPU build (`local-whisper`) is the release
// default and the ONLY updater channel - tauri-plugin-updater has one
// `latest.json` per platform, so two variants cannot share a feed. The CUDA
// build (`local-whisper-cuda`) is a manually-downloaded power-user artifact:
// attach its installer to the GitHub release by hand, clearly labelled, and
// never let it into `latest.json`. CUDA prerequisites are machine-specific
// (CUDA toolkit + a CMake generator nvcc accepts - e.g. Ninja + CUDAARCHS via
// src-tauri/.cargo/config.toml [env]); see src-tauri/Cargo.toml [features].
import { spawnSync } from "node:child_process";
import {
  readFileSync,
  existsSync,
  mkdirSync,
  readdirSync,
  copyFileSync,
} from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { homedir } from "node:os";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const copyOnly = process.argv.includes("--copy-only");
const cuda = process.argv.includes("--cuda");
const features = cuda ? "local-whisper-cuda" : "local-whisper";

const version = JSON.parse(
  readFileSync(join(root, "package.json"), "utf8")
).version;

if (!copyOnly) {
  // Load the private signing key from disk into the env Tauri expects.
  const keyPath =
    process.env.TAURI_KEY_PATH || join(homedir(), ".tauri", "eve.key");
  if (!existsSync(keyPath)) {
    console.error(`Signing key not found at: ${keyPath}`);
    console.error("Set TAURI_KEY_PATH or run: npx tauri signer generate -w ~/.tauri/eve.key");
    process.exit(1);
  }
  const env = {
    ...process.env,
    TAURI_SIGNING_PRIVATE_KEY: readFileSync(keyPath, "utf8"),
    // Key was generated with an empty password; keep it explicit.
    TAURI_SIGNING_PRIVATE_KEY_PASSWORD:
      process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD ?? "",
  };

  console.log(`Building signed release v${version} (${features})...`);
  // `local-whisper` compiles whisper.cpp for on-device speech-to-text. Requires
  // CMake + a C/C++ toolchain (MSVC) and LLVM/libclang for bindgen on this
  // machine; `local-whisper-cuda` additionally needs the CUDA toolkit. See
  // src-tauri/Cargo.toml [features].
  const res = spawnSync("npx", ["tauri", "build", "--features", features], {
    cwd: root,
    env,
    stdio: "inherit",
    shell: true,
  });
  if (res.status !== 0) process.exit(res.status ?? 1);
}

// Copy artifacts out of src-tauri/target/release/bundle into
// build/<version>/{msi,nsis} (CPU) or build/<version>/cuda/{msi,nsis} (CUDA).
// Each app version gets its own folder; rebuilding the same version overwrites
// its artifacts. The split keeps the two variants from clobbering each other
// when both are built for one release.
const bundleDir = join(root, "src-tauri", "target", "release", "bundle");
const outDir = cuda ? join(root, "build", version, "cuda") : join(root, "build", version);

for (const target of ["msi", "nsis"]) {
  const src = join(bundleDir, target);
  if (!existsSync(src)) {
    console.warn(`No ${target} bundle found at ${src} — skipping.`);
    continue;
  }
  const dest = join(outDir, target);
  mkdirSync(dest, { recursive: true });
  for (const file of readdirSync(src)) {
    copyFileSync(join(src, file), join(dest, file));
    console.log(`  ${join(dest, file).slice(root.length + 1).replaceAll("\\", "/")}`);
  }
}

const rel = cuda ? `build/${version}/cuda` : `build/${version}`;
console.log(`\nRelease v${version} (${features}) artifacts copied to ${rel}/msi and ${rel}/nsis`);
if (cuda) {
  console.log(
    "Reminder: the CUDA build is NOT on the updater feed. Attach it to the GitHub release manually, labelled as the CUDA variant."
  );
}
