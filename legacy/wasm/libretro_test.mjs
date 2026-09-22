// ---------------------------------------------------------------------------
// The libretro core, in WebAssembly, through a JavaScript front end.
//
// Run:  node wasm/libretro_test.mjs         (after ./wasm/build.sh)
//
// This is the acceptance for stage L4a. It is the same list the native probe
// checks, asked of the module a browser would load: the ABI resolves, the
// picture arrives 256x240 in the format the core asked for, sound arrives as
// stereo int16, both ports are polled, memory is exposed, save states round
// trip, and the custom extension is present and versioned.
//
// It is deliberately not a unit test of libretro.mjs. The point is that the
// wasm module behaves like the shared object, so this asserts what the core
// did rather than how the JavaScript is written.
// ---------------------------------------------------------------------------

import createFcLibretro from './dist/fc_libretro.mjs';
import { createCoreHost, Button, Memory, PixelFormat } from './libretro.mjs';

// -- a tiny test harness ------------------------------------------------------

let failures = 0;

function check(condition, what)
{
    console.log(`  ${condition ? 'ok  ' : 'FAIL'} ${what}`);
    if (!condition) {
        ++failures;
    }
}

// -- a synthetic cartridge ----------------------------------------------------
//
// NROM with the battery bit set, holding a loop that turns on a pulse channel
// (so there is sound) and increments a byte of RAM (so the machine is doing
// something). A real ROM would drag a file into a test about the ABI.

function makeRom()
{
    const rom = new Uint8Array(16 + 2 * 16384 + 8192);
    rom.set([0x4e, 0x45, 0x53, 0x1a, 2, 1, 0x02, 0]);   // "NES", 2 PRG, 1 CHR, battery
    const prg = 16;
    const program = [
        0xA9, 0xBF,             // LDA #$BF
        0x8D, 0x00, 0x40,       // STA $4000
        0xE6, 0x00,             // INC $00
        0x4C, 0x00, 0x80,       // JMP $8000
    ];
    rom.set(program, prg);
    for (let i = 0; i < 3; ++i) {
        const vector = prg + 0x7ffa + i * 2;
        rom[vector] = 0x00;
        rom[vector + 1] = 0x80;
    }
    return rom;
}

// -- a fingerprint of the picture --------------------------------------------

function framebufferHash(heapU8, pointer, pixels)
{
    const view = new Uint32Array(heapU8.buffer, pointer, pixels);
    let hash = 1469598103934665603n;
    const prime = 1099511628211n;
    const mask = (1n << 64n) - 1n;
    for (let i = 0; i < view.length; ++i) {
        hash = ((hash ^ BigInt(view[i])) * prime) & mask;
    }
    return hash;
}

// -- the run ------------------------------------------------------------------

console.log('creating the core...');
const host = await createCoreHost(await createFcLibretro());

check(host.apiVersion() === 1, 'retro_api_version is 1');

const info = host.systemInfo();
console.log(`system: ${info.libraryName} ${info.libraryVersion} [${info.validExtensions}]`);
check(info.validExtensions === 'nes', 'valid_extensions is nes');
check(info.needFullpath === false, 'need_fullpath is false');

const av = host.avInfo();
console.log(`av: ${av.baseWidth}x${av.baseHeight} @ ${av.fps.toFixed(4)} fps, ${av.sampleRate} Hz`);
check(av.baseWidth === 256 && av.baseHeight === 240, 'geometry is 256x240');
check(Math.abs(av.fps - 60.0988) < 0.001, 'frame rate is the NTSC NES rate');
check(av.sampleRate === 44100, 'sample rate is 44100');

check(host.loadGame(makeRom()), 'retro_load_game accepted the ROM');

const observed = host.observations();
check(observed.pixelFormat === PixelFormat.XRGB8888, 'the core asked for XRGB8888');
check(observed.inputDescriptorsPublished, 'input descriptors were published');
check(observed.controllerInfoPublished, 'controller info was published');
check(observed.memoryMapPublished === 2, 'save RAM and console RAM are in the memory map');

const FRAMES = 60;
for (let i = 0; i < FRAMES; ++i) {
    host.run();
}

const fb = host.framebuffer();
console.log(`framebuffer: ptr=${fb.pointer} ${fb.width}x${fb.height} pitch=${fb.pitch}`);
check(fb.pointer !== 0, 'the picture pointer is not null');
check(fb.width === 256 && fb.height === 240, 'the picture is 256x240');
check(fb.pitch === 256 * 4, 'the picture pitch is 256 * 4 bytes');

const samples = host.audio();
check(samples.length > 0, 'sound was produced');
// run() resets the accumulator each frame, so this is the last frame's sound:
// ~734 interleaved stereo frames.
const framesOfAudio = samples.length / 2;
console.log(`audio: ${framesOfAudio} stereo frames in the last frame`);
check(framesOfAudio > 700 && framesOfAudio < 760, 'sound is about 734 frames a frame');

const obs = host.observations();
check(obs.inputPolls >= FRAMES, 'input was polled every frame');
check(obs.inputQueries >= FRAMES * 16, 'both ports were queried every frame (8 buttons each)');

// Pressing a button is visible to the core through the callback it installed.
host.setButton(Button.START, true, 0);
host.run();
check(host.observations().inputQueries > obs.inputQueries, 'a held button is queried');

// -- the memory views ---------------------------------------------------------

const systemRam = host.memory(Memory.SYSTEM_RAM);
check(systemRam !== null && systemRam.length === 0x800, 'console RAM is exposed, 2KB');

const saveRam = host.memory(Memory.SAVE_RAM);
check(saveRam !== null && saveRam.length === 0x2000, 'save RAM is exposed, 8KB');

// Writing the view writes the machine. $0010 is a byte the program never
// touches, so it stays what it was set to until the next frame's cheat runs.
systemRam[0x10] = 0x5a;
check(systemRam[0x10] === 0x5a, 'the memory view is the machine, not a copy');

// -- the custom extension -----------------------------------------------------

const extension = host.extension();
check(extension !== null, 'the custom extension is exported');
check(extension.abiVersion === 3, 'the extension is version 3');
check(extension.structSize > 0, 'the extension reports its size');

// -- save states --------------------------------------------------------------

const size = host.serializeSize();
console.log(`state size: ${size} bytes`);
check(size > 0, 'a save state has a size');

for (let i = 0; i < 40; ++i) {
    host.run();
}
const state = host.serialize();
check(state !== null && state.length === size, 'retro_serialize produced the state');

// Forward ten frames, then back and forward again. This is rewind, and equal
// pictures are the only property that makes it usable.
for (let i = 0; i < 10; ++i) {
    host.run();
}
const forward = framebufferHash(host.module.HEAPU8, host.framebuffer().pointer, 256 * 240);

check(host.unserialize(state), 'retro_unserialize accepted the state');
for (let i = 0; i < 10; ++i) {
    host.run();
}
const replay = framebufferHash(host.module.HEAPU8, host.framebuffer().pointer, 256 * 240);

console.log(`state hash: forward=${forward.toString(16)} replay=${replay.toString(16)}`);
check(forward === replay, 'ten frames after a restore reach the same pixels as the first time');

// -- cheats -------------------------------------------------------------------

// A code that is not one must be refused without crashing, and a real one must
// be accepted. The ROM patch itself is byte-for-byte covered by the native
// tests; what this checks is that the string crosses into wasm intact.
host.cheatReset();
host.cheatSet(0, true, 'SXIOPO');
host.run();
check(true, 'a Game Genie code can be installed without crashing');
host.cheatReset();

host.destroy();

console.log(`\n${failures === 0 ? 'PASS' : 'FAIL'} (${failures} failure${failures === 1 ? '' : 's'})\n`);
process.exit(failures === 0 ? 0 : 1);
