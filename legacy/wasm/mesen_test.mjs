// ---------------------------------------------------------------------------
// Mesen, in WebAssembly, through the same CoreHost the NES and mGBA cores use.
//
// Run:  node wasm/mesen_test.mjs /path/to/game.nes
//       node wasm/mesen_test.mjs                # skips, no ROM
//
// Build the core first:  ./wasm/mesen/build.sh
//
// Mesen is the second core for the *same* console as fc_libretro. That is the
// point: adding "an emulator" to this project is compiling another libretro
// core, and wasm/libretro.mjs drives it without being told which one it has.
// The player picks between the two in 设置 → 模拟器核心.
//
// The checks are the ones that mean something without knowing the game: the
// ABI answers, a cartridge loads, the picture is the size the core says, sound
// comes out, and a save state round trips.
//
// No ROM is committed here -- they are copyrighted -- so with no argument this
// prints how to run it and exits 0.
// ---------------------------------------------------------------------------

import createMesen from './dist/mesen_libretro.mjs';
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
    console.log('usage: node wasm/mesen_test.mjs /path/to/game.nes');
    process.exit(0);
}

const host = await createCoreHost(await createMesen());

check(host.apiVersion() === 1, 'retro_api_version is 1');

const info = host.systemInfo();
console.log(`system: ${info.libraryName} ${info.libraryVersion} [${info.validExtensions}]`);
check(info.validExtensions.includes('nes'), 'the core advertises NES extensions');

const rom = new Uint8Array(readFileSync(romPath));
check(host.loadRom(rom), `retro_load_game accepted ${rom.length} bytes`);

// Mesen answers this before a game is in, but the load has happened anyway.
const av = host.avInfo();
console.log(`av: ${av.baseWidth}x${av.baseHeight} @ ${av.fps.toFixed(4)} fps, ${av.sampleRate} Hz`);
check(host.width === 256 && host.height === 240, 'the console is 256x240');

for (let i = 0; i < 120; ++i) {
    host.runFrame();
}

const fb = host.framebuffer();
console.log(`framebuffer: ${fb.width}x${fb.height} ptr=${fb.pointer} pitch=${fb.pitch}`);
check(fb.pointer !== 0, 'the core produced a picture');
check(fb.width === 256 && fb.height === 240, 'the picture is the size advertised');

// A frame that is not one flat colour is a frame with something on it; the
// title screen of any game passes this, a broken core does not.
const pixels = host.framebufferBytes();
const seen = new Set();
for (let i = 0; i + 3 < pixels.length; i += 4) {
    seen.add((pixels[i + 2] << 16) | (pixels[i + 1] << 8) | pixels[i]);
    if (seen.size > 3) {
        break;
    }
}
console.log(`distinct colours (up to 4 checked): ${seen.size}`);
check(seen.size > 1, 'the frame is not one flat colour');

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
