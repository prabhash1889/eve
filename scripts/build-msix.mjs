#!/usr/bin/env node
// Package the Store build into an (unsigned) .msix for the Microsoft Store.
//
// Prereq: `npm run build:store` has produced src-tauri/target/release/Eve.exe
// plus its resources/ (bundled Parakeet) and DLLs. Needs the Windows 10/11 SDK
// (makeappx.exe). The Store re-signs on submission, so this does NOT sign -
// signing is only needed to sideload-test locally (see packaging/README.md).
//
// Usage:
//   npm run build:store        # build the payload first
//   npm run build:msix         # then pack it
//
// Identity: reserve the app name in Partner Center, then set:
//   MSIX_IDENTITY_NAME, MSIX_PUBLISHER, MSIX_PUBLISHER_DISPLAY
// (env vars). Placeholders are used otherwise and the Store will reject those.

import { spawnSync } from "node:child_process";
import {
  readFileSync,
  writeFileSync,
  existsSync,
  mkdirSync,
  rmSync,
  cpSync,
  copyFileSync,
  readdirSync,
} from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
// MSIX version is 4-part; the revision (4th part) MUST be 0 for the Store.
const version = `${pkg.version}.0`;

// Store identity - reserve the name in Partner Center, then set these env vars.
const NAME = process.env.MSIX_IDENTITY_NAME || "REPLACE.WithYourReservedName";
const PUBLISHER =
  process.env.MSIX_PUBLISHER || "CN=REPLACE-WITH-PARTNER-CENTER-PUBLISHER-ID";
const PUBLISHER_DISPLAY = process.env.MSIX_PUBLISHER_DISPLAY || "Your Publisher Name";

const releaseDir = join(root, "src-tauri", "target", "release");
const exe = join(releaseDir, "Eve.exe");
if (!existsSync(exe)) {
  console.error(`Missing ${exe}\nRun \`npm run build:store\` first.`);
  process.exit(1);
}

// Locate makeappx.exe in the Windows SDK (newest installed version wins).
function findSdkTool(name) {
  const bases = [
    "C:/Program Files (x86)/Windows Kits/10/bin",
    "C:/Program Files/Windows Kits/10/bin",
  ];
  for (const base of bases) {
    if (!existsSync(base)) continue;
    const versions = readdirSync(base)
      .filter((d) => /^10\./.test(d))
      .sort()
      .reverse();
    for (const v of versions) {
      const p = join(base, v, "x64", name);
      if (existsSync(p)) return p;
    }
    const flat = join(base, "x64", name);
    if (existsSync(flat)) return flat;
  }
  return null;
}
const makeappx = findSdkTool("makeappx.exe");
if (!makeappx) {
  console.error(
    "makeappx.exe not found. Install the Windows 10/11 SDK (the 'App packaging tools' / MSIX component)."
  );
  process.exit(1);
}

// Assemble the package layout under build/msix-layout.
const layout = join(root, "build", "msix-layout");
rmSync(layout, { recursive: true, force: true });
mkdirSync(layout, { recursive: true });

// 1. Binary + every DLL sitting next to it (WebView2Loader, onnxruntime, ...).
copyFileSync(exe, join(layout, "Eve.exe"));
for (const f of readdirSync(releaseDir)) {
  if (f.endsWith(".dll")) copyFileSync(join(releaseDir, f), join(layout, f));
}

// 2. Bundled resources (Parakeet weights). Preserve the relative path the app
//    resolves at runtime: resources/models/parakeet-tdt-0.6b-v2/*.
const resSrc = join(releaseDir, "resources");
if (existsSync(resSrc)) {
  cpSync(resSrc, join(layout, "resources"), { recursive: true });
} else {
  console.warn(
    "WARN: no resources/ in target/release - offline Parakeet will NOT be bundled."
  );
}

// 3. Tile assets. The app icon stands in for every required tile so packing
//    succeeds; replace build/msix-layout/Assets with correctly-sized tiles
//    before Store submission (the Store checks tile dimensions).
const assets = join(layout, "Assets");
mkdirSync(assets, { recursive: true });
// Prefer the correctly-sized tiles in packaging/assets; fall back to the app
// icon only if one is missing (dimension-wrong, but keeps packing from failing).
const assetSrc = join(root, "packaging", "assets");
const iconFallback = join(root, "src-tauri", "icons", "128x128.png");
let usedIconFallback = false;
for (const name of [
  "StoreLogo.png",
  "Square150x150Logo.png",
  "Square44x44Logo.png",
  "Wide310x150Logo.png",
  "SplashScreen.png",
]) {
  const from = join(assetSrc, name);
  const ok = existsSync(from);
  if (!ok) usedIconFallback = true;
  copyFileSync(ok ? from : iconFallback, join(assets, name));
}

// 4. Manifest from the template.
const manifest = readFileSync(
  join(root, "packaging", "AppxManifest.xml.template"),
  "utf8"
)
  // replaceAll: the tokens also appear in the manifest's header comment, so a
  // first-occurrence replace would substitute the comment and leave the real
  // Identity attributes untouched. Order PUBLISHER_DISPLAY before PUBLISHER so
  // the shorter token doesn't partially match the longer one.
  .replaceAll("{PUBLISHER_DISPLAY}", PUBLISHER_DISPLAY)
  .replaceAll("{PUBLISHER}", PUBLISHER)
  .replaceAll("{NAME}", NAME)
  .replaceAll("{VERSION}", version);
writeFileSync(join(layout, "AppxManifest.xml"), manifest);

// 5. Pack.
const outDir = join(root, "build", version);
mkdirSync(outDir, { recursive: true });
const out = join(outDir, `Eve-${version}-store.msix`);
const res = spawnSync(makeappx, ["pack", "/o", "/d", layout, "/p", out], {
  stdio: "inherit",
});
if (res.status !== 0) process.exit(res.status ?? 1);

console.log(`\nMSIX written: ${out}`);
if (NAME.startsWith("REPLACE") || PUBLISHER.includes("REPLACE")) {
  console.log(
    "NOTE: identity is a placeholder. Set MSIX_IDENTITY_NAME / MSIX_PUBLISHER /\n" +
      "      MSIX_PUBLISHER_DISPLAY to your Partner Center values before submitting."
  );
}
if (usedIconFallback) {
  console.log(
    "Some tiles fell back to the app icon (wrong dimensions). Add sized tiles to\n" +
      "      packaging/assets/ before Store submission."
  );
}
