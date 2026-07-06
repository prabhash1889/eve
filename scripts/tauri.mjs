// Wrapper for the `tauri` CLI invoked via `npm run tauri <subcommand>`.
//
// For `npm run tauri build` it runs the real build (forwarding any extra
// flags) and then copies the produced installers into
//   <repo>/build/<version>/{msi,nsis}        (CPU / default)
//   <repo>/build/<version>/cuda/{msi,nsis}   (CUDA feature builds)
// via `scripts/release.mjs --copy-only`, so every build lands in a
// version-named folder at the repo root. Any other subcommand (dev, icon,
// ...) is passed straight through to tauri.
//
// The signed updater release flow stays `npm run release` (see release.mjs);
// this wrapper only adds the artifact collection to plain local builds.

import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, join, resolve } from 'node:path';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const args = process.argv.slice(2);

function run(cmd, cmdArgs) {
  const res = spawnSync(cmd, cmdArgs, {
    cwd: root,
    stdio: 'inherit',
    shell: process.platform === 'win32', // npx/node resolution needs a shell on Windows
  });
  if (res.status !== 0) process.exit(res.status ?? 1);
}

run('npx', ['tauri', ...args]);

if (args[0] === 'build') {
  // Keep the CUDA variant in its own subfolder so the two builds of one
  // version never clobber each other (see the build matrix in release.mjs).
  const cuda = args.some((a) => a.includes('local-whisper-cuda'));
  run('node', [join('scripts', 'release.mjs'), '--copy-only', ...(cuda ? ['--cuda'] : [])]);
}
