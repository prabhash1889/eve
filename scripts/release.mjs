#!/usr/bin/env node
// Build signed release bundles, then copy the MSI + NSIS artifacts into
//   <repo>/build/msi  and  <repo>/build/nsis
// The signing private key is loaded from the file at TAURI_KEY_PATH
// (default: ~/.tauri/eve.key) so it never has to live in the repo.
//
// Usage: node scripts/release.mjs            (full signed build + copy)
//        node scripts/release.mjs --copy-only (skip build, just copy existing)
import { spawnSync } from "node:child_process";
import {
  readFileSync,
  existsSync,
  mkdirSync,
  readdirSync,
  copyFileSync,
  rmSync,
} from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { homedir } from "node:os";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const copyOnly = process.argv.includes("--copy-only");

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

  console.log(`Building signed release v${version}...`);
  // `--features local-models` compiles whisper.cpp + llama.cpp for on-device
  // inference. Requires CMake + a C/C++ toolchain (MSVC) and LLVM/libclang for
  // bindgen on this machine; see src-tauri/Cargo.toml [features].
  const res = spawnSync("npx", ["tauri", "build", "--features", "local-models"], {
    cwd: root,
    env,
    stdio: "inherit",
    shell: true,
  });
  if (res.status !== 0) process.exit(res.status ?? 1);
}

// Copy artifacts out of src-tauri/target/release/bundle into build/{msi,nsis}.
const bundleDir = join(root, "src-tauri", "target", "release", "bundle");
const outDir = join(root, "build");

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
    console.log(`  build/${target}/${file}`);
  }
}

console.log(`\nRelease v${version} artifacts copied to build/msi and build/nsis`);
