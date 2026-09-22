// ---------------------------------------------------------------------------
// `pnpm run icon`
//
// Turns build/icon-source.png into the two files electron-builder looks for:
//
//   build/icon.icns   the macOS icon, rendered by make-icon.swift and packed
//                     by iconutil
//   build/icon.png    a plain 1024 PNG, for Linux and as a Windows fallback
//
// The rendering lives in Swift -- the macOS icon is a rounded square inset in
// a transparent canvas, not a resize, and CoreGraphics is always on a Mac.
// This script only sequences swift and iconutil, and stops the build with a
// readable message when either is missing.
//
//   node scripts/make-icon.mjs [source.png]
// ---------------------------------------------------------------------------

import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const electronRoot = fileURLToPath(new URL('..', import.meta.url));
const buildDir = join(electronRoot, 'build');
const swiftFile = join(electronRoot, 'scripts', 'make-icon.swift');

/** Everything this script says is prefixed, so `grep '\[icon\]'` sees it. */
function say(message) {
    console.log(`[icon] ${message}`);
}

function fail(message) {
    console.error(`[icon] ${message}`);
    process.exit(1);
}

/** Run a command, inherit its output, and stop the build if it fails. */
function run(program, args) {
    const result = spawnSync(program, args, { stdio: 'inherit', shell: false });
    if (result.error !== undefined) {
        fail(`could not run ${program}: ${result.error.message}`);
    }
    if (result.status !== 0) {
        fail(`${program} exited with ${result.status ?? 'a signal'}`);
    }
}

const source = process.argv[2] ?? join(buildDir, 'icon-source.png');
const iconset = join(buildDir, 'icon.iconset');
const icns = join(buildDir, 'icon.icns');

if (!existsSync(source)) {
    fail(`no source picture at ${source}. Pass one: node scripts/make-icon.mjs <source.png>`);
}
mkdirSync(buildDir, { recursive: true });

say(`rendering ${source}`);
run('swift', [swiftFile, source, iconset]);

if (process.platform !== 'darwin') {
    say('iconutil is macOS-only; wrote the .iconset and icon.png, skipped the .icns');
    process.exit(0);
}

say('packing build/icon.icns -- iconutil');
run('iconutil', ['-c', 'icns', iconset, '-o', icns]);

const bytes = statSync(icns).size;
say(`wrote build/icon.icns (${(bytes / 1024).toFixed(1)} KB)`);
