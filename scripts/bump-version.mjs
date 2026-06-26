#!/usr/bin/env node
// Bump the app version in lockstep across the three places it lives:
//   package.json, src-tauri/tauri.conf.json, src-tauri/Cargo.toml
// Usage: node scripts/bump-version.mjs <patch|minor|major|x.y.z>
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const arg = process.argv[2];
if (!arg) {
  console.error("Usage: node scripts/bump-version.mjs <patch|minor|major|x.y.z>");
  process.exit(1);
}

const pkgPath = join(root, "package.json");
const confPath = join(root, "src-tauri", "tauri.conf.json");
const cargoPath = join(root, "src-tauri", "Cargo.toml");

const pkg = JSON.parse(readFileSync(pkgPath, "utf8"));
const current = pkg.version;

function nextVersion(cur, kind) {
  if (/^\d+\.\d+\.\d+$/.test(kind)) return kind; // explicit version
  const [maj, min, pat] = cur.split(".").map(Number);
  if (kind === "major") return `${maj + 1}.0.0`;
  if (kind === "minor") return `${maj}.${min + 1}.0`;
  if (kind === "patch") return `${maj}.${min}.${pat + 1}`;
  throw new Error(`Unknown bump kind: ${kind}`);
}

const next = nextVersion(current, arg);

// package.json
pkg.version = next;
writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + "\n");

// tauri.conf.json
const conf = JSON.parse(readFileSync(confPath, "utf8"));
conf.version = next;
writeFileSync(confPath, JSON.stringify(conf, null, 2) + "\n");

// Cargo.toml — only the [package] version (first `version = "..."`)
let cargo = readFileSync(cargoPath, "utf8");
cargo = cargo.replace(/^version = ".*"$/m, `version = "${next}"`);
writeFileSync(cargoPath, cargo);

console.log(`Version bumped: ${current} -> ${next}`);
console.log("Updated: package.json, src-tauri/tauri.conf.json, src-tauri/Cargo.toml");
