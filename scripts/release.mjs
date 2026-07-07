#!/usr/bin/env node
// Build signed release bundles, then MOVE the MSI + NSIS artifacts into
//   <repo>/build/<version>/{msi,nsis}        (CPU)
//   <repo>/build/<version>/cuda/{msi,nsis}   (CUDA)
// (a move, not a copy, so nothing is left behind inside src-tauri).
// CUDA installers are renamed with a `_cuda` tag because Tauri gives them the
// same filenames as the CPU build; the tag keeps the variant identifiable.
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
  rmSync,
  statSync,
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

// Move artifacts out of src-tauri/target/release/bundle into
// build/<version>/{msi,nsis} (CPU) or build/<version>/cuda/{msi,nsis} (CUDA).
// Each app version gets its own folder; rebuilding the same version overwrites
// its artifacts. The split keeps the two variants from clobbering each other
// when both are built for one release. Nothing is left in src-tauri afterwards.
const bundleDir = join(root, "src-tauri", "target", "release", "bundle");
const outDir = cuda ? join(root, "build", version, "cuda") : join(root, "build", version);

// Tauri writes each installer into a per-format subfolder of `bundle/`, and
// which formats exist depends on the host OS. Collect whatever this platform
// produces: Windows -> MSI + NSIS, macOS -> .app + .dmg, Linux -> deb/rpm/AppImage.
const BUNDLE_TARGETS = {
  win32: ["msi", "nsis"],
  darwin: ["dmg", "macos"],
  linux: ["deb", "rpm", "appimage"],
};
const targets = BUNDLE_TARGETS[process.platform] ?? ["msi", "nsis"];

for (const target of targets) {
  const src = join(bundleDir, target);
  if (!existsSync(src)) {
    console.warn(`No ${target} bundle found at ${src} — skipping.`);
    continue;
  }
  const dest = join(outDir, target);
  mkdirSync(dest, { recursive: true });
  for (const file of readdirSync(src)) {
    const from = join(src, file);
    // Tauri leaves helper DIRECTORIES next to the installers (e.g. the unpacked
    // `Eve_<version>_amd64/` deb staging tree on Linux, `Eve.app/` on macOS).
    // Only real files are installers/sidecars; copyFileSync on a directory
    // throws EISDIR and killed CI builds.
    if (statSync(from).isDirectory()) continue;
    // Tauri names CUDA installers identically to the CPU ones, so tag the CUDA
    // variant (installer + any .sig/.zip sidecars) with `_cuda` right after the
    // version to keep it distinguishable once collected.
    const outName = cuda ? file.replaceAll(`_${version}_`, `_${version}_cuda_`) : file;
    const to = join(dest, outName);
    // Move, not copy: copy then remove the source so nothing stays in src-tauri.
    // copy+rm (rather than renameSync) because on Windows renameSync throws when
    // the destination already exists, e.g. rebuilding the same version.
    copyFileSync(from, to);
    rmSync(from);
    console.log(`  ${to.slice(root.length + 1).replaceAll("\\", "/")}`);
  }
}

const rel = cuda ? `build/${version}/cuda` : `build/${version}`;
console.log(
  `\nRelease v${version} (${features}) artifacts moved to ${rel}/{${targets.join(",")}}`
);
if (cuda) {
  console.log(
    "Reminder: the CUDA build is NOT on the updater feed. Attach it to the GitHub release manually, labelled as the CUDA variant."
  );
}
