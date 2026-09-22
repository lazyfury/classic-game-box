// ---------------------------------------------------------------------------
// mGBA, in WebAssembly, through the same CoreHost the NES core uses.
//
// Run:  node wasm/mgba_test.mjs /path/to/game.gba     (or .gb / .gbc)
//       node wasm/mgba_test.mjs                        # skips, no ROM
//
// Build the core first:  ./wasm/mgba/build.sh
//
// The point of this file is not mGBA. It is that a *different* emulator, built
// from a different project's source by a different build system, loads through
// wasm/libretro.mjs without that file being told which core it is. That is
// what "libretro is the contract" buys, and it is what makes adding a console
// a compile rather than a front end.
//
// The checks are the ones that mean something without knowing the game: the
// ABI answers, a cartridge loads, the picture is the size the core says, sound
// comes out, and a save state round trips. A real game is the caller's job.
//
// No ROM is committed here -- they are copyrighted -- so with no argument this
// prints how to run it and exits 0.
// ---------------------------------------------------------------------------

import createMgba from './dist/mgba_libretro.mjs';
import { createCoreHost } from './libretro.mjs';
import { readFileSync } from 'node:fs';

let failures = 0;
function check(condition, what)
{
    console.log(`  ${condition ? 'ok  ' : 'FAIL'} ${what}`);
    if (!condition) {
        ++failures;
    }
}

const romPath = process.argv[2];
if (!romPath) {
    console.log('no ROM given; skipping.');
    console.log('usage: node wasm/mgba_test.mjs /path/to/game.gba');
    process.exit(0);
}

const host = await createCoreHost(await createMgba());

check(host.apiVersion() === 1, 'retro_api_version is 1');

const info = host.systemInfo();
console.log(`system: ${info.libraryName} ${info.libraryVersion} [${info.validExtensions}]`);
check(info.validExtensions.includes('gba') || info.validExtensions.includes('gbc'),
      'the core advertises Game Boy extensions');

const rom = new Uint8Array(readFileSync(romPath));
check(host.loadRom(rom), `retro_load_game accepted ${rom.length} bytes`);

// Only safe to ask once a game is in: mGBA's geometry call reads its machine.
const av = host.avInfo();
console.log(`av: ${av.baseWidth}x${av.baseHeight} @ ${av.fps.toFixed(4)} fps, ${av.sampleRate} Hz`);
check(host.width === av.baseWidth && host.height === av.baseHeight,
      'the host reports the geometry the core gave');

for (let i = 0; i < 60; ++i) {
    host.runFrame();
}

const fb = host.framebuffer();
console.log(`framebuffer: ${fb.width}x${fb.height} ptr=${fb.pointer}`);
check(fb.pointer !== 0, 'the core produced a picture');
check(fb.width === av.baseWidth && fb.height === av.baseHeight,
      'the picture is the size the core advertised');

const samples = host.takeSamples();
console.log(`audio: ${samples.length} mono samples in the last frame`);
check(samples.length > 0, 'the core produced sound');

const state = host.saveState();
console.log(`save state: ${state ? state.length : 0} bytes`);
check(state !== null && state.length > 0, 'a save state exists');
check(state !== null && host.loadState(state), 'the save state round trips');

host.destroy();

console.log(`\n${failures === 0 ? 'PASS' : 'FAIL'} (${failures} failure${failures === 1 ? '' : 's'})\n`);
process.exit(failures === 0 ? 0 : 1);
