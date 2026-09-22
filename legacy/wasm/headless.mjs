#!/usr/bin/env node
// ---------------------------------------------------------------------------
// Run the wasm build in Node and dump frames. No Electron, no window.
//
//   node wasm/headless.mjs game.nes --frames 300 --outdir /tmp/wasm-frames
//   node wasm/headless.mjs game.nes --frames 600 \
//        --script 100:START=1,105:START=0,300:RIGHT=1 \
//        --snapshots 90,250,400,600
//
// Why this exists
// ---------------
// Two reasons, and the second one is the important one.
//
// It is the fastest way to find out whether a problem is in the emulator or
// in the front end: if the picture is right here, the core is fine and the bug
// is in Electron.
//
// And it is a *regression test that runs the shipping artifact*. The module
// this loads, wasm/dist/fc_core.wasm, is byte for byte the one the renderer
// loads. A native helper process would not give us that: the helper and the
// app are two builds, and passing the helper's tests says nothing about
// whether the app is running the same code.
//
// Output
// ------
// For every frame in --snapshots (default 1, 5, 30 and the last), one line:
//
//   hash <frame> <sha256 of the frame's R,G,B bytes>
//
// Those bytes are exactly what a PPM holds after its 15 byte header, so the
// same number comes out of `tail -c +16 frame.ppm | shasum -a 256`, and the
// same number comes out of the Electron renderer. Three builds, one number.
// ---------------------------------------------------------------------------

import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { join } from 'node:path';
import { Button, Emulator } from './emulator.mjs';

// Node's answer to "where is fc_core.wasm": right here, next to this file.
import createFcCore from './dist/fc_core.mjs';

// -- command line -----------------------------------------------------------

function parseArguments(argv) {
    const options = {
        romPath: null,
        frames: 60,
        outdir: null,
        dump: null,
        script: null,
        snapshots: null,
        wav: null,
        samples: null,
        save: null,
        roundtrip: 0,
        quiet: false,
        help: false,
    };

    for (let i = 0; i < argv.length; i += 1) {
        const argument = argv[i];
        switch (argument) {
        case '--frames':
            options.frames = Number.parseInt(argv[++i], 10);
            break;
        case '--outdir':
            options.outdir = argv[++i];
            break;
        case '--dump':
            options.dump = argv[++i];
            break;
        case '--script':
            options.script = argv[++i];
            break;
        case '--snapshots':
            options.snapshots = argv[++i];
            break;
        case '--wav':
            options.wav = argv[++i];
            break;
        case '--samples':
            options.samples = argv[++i];
            break;
        case '--save':
            options.save = argv[++i];
            break;
        case '--roundtrip':
            options.roundtrip = Number.parseInt(argv[++i], 10);
            break;
        case '--quiet':
            options.quiet = true;
            break;
        case '--help':
        case '-h':
            options.help = true;
            break;
        default:
            if (options.romPath === null) {
                options.romPath = argument;
            }
            break;
        }
    }
    return options;
}

const usage = `usage: node wasm/headless.mjs <rom.nes> [options]

  --frames N        how many frames to run            (default 60)
  --outdir DIR      write a PPM for each snapshot frame
  --dump FILE       write the final frame as a single PPM
  --script SPEC     press buttons at frames, as in 100:START=1,105:START=0
                    A change is applied before its frame runs, because the
                    game samples the controller once per frame during vblank.
  --snapshots LIST  frames to hash and dump           (default 1,5,30,last)
  --wav FILE        write everything the APU produced, as 16 bit PCM
  --samples FILE    write the same samples as raw float32, so two builds can
                    be compared bit for bit
  --save FILE       write a save state after the last frame
  --roundtrip N     save and immediately reload at frame N. If the state is
                    complete, the run is byte for byte the same as one
                    without it
  --quiet           print only the hash lines
`;

/**
 * Read "100:START=1,105:START=0".
 *
 * The same syntax, and the same meaning, as the Electron main process and the
 * native fc_headless tool. One scenario string drives every build being
 * compared.
 */
function parseScript(spec) {
    if (spec === null || spec.trim() === '') {
        return [];
    }

    return spec.split(',').map((entry) => {
        const text = entry.trim();
        const match = /^(\d+):([A-Z]+)=([01])$/.exec(text);
        if (match === null) {
            throw new Error(
                `--script: cannot read "${text}". Expected frame:BUTTON=0|1, as in 100:START=1`);
        }
        if (!(match[2] in Button)) {
            throw new Error(`--script: "${match[2]}" is not a button`);
        }
        return { frame: Number(match[1]), button: match[2], pressed: match[3] === '1' };
    });
}

function parseSnapshots(spec, frames) {
    if (spec === null || spec.trim() === '') {
        return [1, 5, 30, frames];
    }
    return spec
        .split(',')
        .map((entry) => Number.parseInt(entry.trim(), 10))
        .filter((frame) => Number.isInteger(frame) && frame >= 1 && frame <= frames);
}

// -- pixels -----------------------------------------------------------------

/**
 * The framebuffer as R,G,B triples.
 *
 * The emulator stores 0x00RRGGBB, so little endian in memory the bytes run
 * B, G, R, 0. A PPM wants R, G, B in that order. That reshuffle is the entire
 * content of this function, and it is the reason the core does not promise a
 * byte order to anyone: the right layout depends on where the pixels are
 * going.
 */
function toRgb(bytes, width, height) {
    const rgb = Buffer.allocUnsafe(width * height * 3);
    for (let pixel = 0, out = 0; out < rgb.length; pixel += 4, out += 3) {
        rgb[out] = bytes[pixel + 2];     // R
        rgb[out + 1] = bytes[pixel + 1]; // G
        rgb[out + 2] = bytes[pixel];     // B
    }
    return rgb;
}

function writePpm(path, rgb, width, height) {
    const header = Buffer.from(`P6\n${width} ${height}\n255\n`, 'ascii');
    writeFileSync(path, Buffer.concat([header, rgb]));
}

// -- audio ------------------------------------------------------------------

/**
 * The same samples as raw little endian float32, with no header and no
 * quantisation.
 *
 * The WAV below is for listening; this is for comparing. A single least
 * significant bit lost to the conversion would hide exactly the kind of drift
 * a cross compilation check exists to find.
 */
function writeSamples(path, samples) {
    writeFileSync(path, Buffer.from(samples.buffer, samples.byteOffset, samples.byteLength));
}

/** A mono 16 bit PCM WAV. 44 bytes of header and then the samples. */
function writeWav(path, samples, sampleRate) {
    const dataBytes = samples.length * 2;
    const buffer = Buffer.alloc(44 + dataBytes);

    buffer.write('RIFF', 0, 'ascii');
    buffer.writeUInt32LE(36 + dataBytes, 4);
    buffer.write('WAVE', 8, 'ascii');
    buffer.write('fmt ', 12, 'ascii');
    buffer.writeUInt32LE(16, 16);            // header size
    buffer.writeUInt16LE(1, 20);             // PCM
    buffer.writeUInt16LE(1, 22);             // mono
    buffer.writeUInt32LE(sampleRate, 24);
    buffer.writeUInt32LE(sampleRate * 2, 28); // bytes per second
    buffer.writeUInt16LE(2, 32);             // block align
    buffer.writeUInt16LE(16, 34);            // bits per sample
    buffer.write('data', 36, 'ascii');
    buffer.writeUInt32LE(dataBytes, 40);

    for (let i = 0; i < samples.length; i += 1) {
        const clamped = Math.max(-1, Math.min(1, samples[i]));
        buffer.writeInt16LE(Math.trunc(clamped * 32767), 44 + i * 2);
    }

    writeFileSync(path, buffer);
}

// -- main -------------------------------------------------------------------

async function main() {
    const options = parseArguments(process.argv.slice(2));

    if (options.help || options.romPath === null) {
        process.stdout.write(usage);
        return options.help ? 0 : 1;
    }

    let script;
    let snapshots;
    try {
        script = parseScript(options.script);
        snapshots = new Set(parseSnapshots(options.snapshots, options.frames));
    } catch (error) {
        process.stderr.write(`${error.message}\n`);
        return 2;
    }

    const rom = readFileSync(options.romPath);
    const emulator = await Emulator.create({ module: await createFcCore() });

    if (!emulator.loadRom(new Uint8Array(rom))) {
        process.stderr.write(`could not load ${options.romPath}: ${emulator.lastError}\n`);
        return 1;
    }

    if (!options.quiet) {
        process.stdout.write(`ROM            : ${options.romPath}\n`);
        process.stdout.write(`ROM size       : ${rom.length} bytes\n`);
        process.stdout.write(`cartridge      : ${emulator.romSummary}\n`);
        process.stdout.write(`screen         : ${emulator.width}x${emulator.height}\n`);
        process.stdout.write(`sample rate    : ${emulator.sampleRate}\n`);
    }

    // Machine readable, always printed. Whether the mapper in the slot saves
    // its bank registers decides whether a round trip test can say anything
    // about this ROM at all.
    process.stdout.write(`mapperstate ${emulator.mapperSavesState ? 1 : 0}\n`);

    // Exactly what demo_ppu does before its loop, so the builds can be
    // compared byte for byte.
    emulator.reset();

    if (options.outdir !== null) {
        mkdirSync(options.outdir, { recursive: true });
    }

    // Group the script by frame, so the inner loop does not search.
    const byFrame = new Map();
    for (const event of script) {
        const events = byFrame.get(event.frame) ?? [];
        events.push(event);
        byFrame.set(event.frame, events);
    }

    let framesRun = 0;
    let audibleFrames = 0;
    let peak = 0;
    let halted = false;
    let lastRgb = null;

    // Samples are copied out of the emulator's scratch buffer in chunks, and
    // only when somebody asked for them: 440,000 floats is not worth moving
    // for a test that only wants a picture.
    const wantAudio = options.wav !== null || options.samples !== null;
    const audioChunks = [];

    for (let frame = 1; frame <= options.frames; frame += 1) {
        // The round trip. Saving and immediately reloading has to change
        // nothing at all: same registers, same RAM, same PPU phase, same APU
        // envelope positions, same mapper banks. If any one of those is
        // missing, this run and the run without --roundtrip diverge, and the
        // difference shows up in the frame hashes and in the samples rather
        // than needing a separate comparison.
        if (options.roundtrip > 0 && frame === options.roundtrip) {
            const state = emulator.saveState();
            if (state === null || !emulator.loadState(state)) {
                process.stderr.write(`could not round trip a state at frame ${frame}: `
                    + `${emulator.lastError}\n`);
                emulator.destroy();
                return 1;
            }
            if (!options.quiet) {
                process.stdout.write(`roundtrip      : saved and reloaded at frame ${frame}`
                    + ` (${state.length} bytes)\n`);
            }
        }

        for (const event of byFrame.get(frame) ?? []) {
            emulator.setButton(Button[event.button], event.pressed);
        }

        if (!emulator.runFrame()) {
            process.stderr.write(`the CPU halted at frame ${frame} (emulator bug)\n`);
            halted = true;
            break;
        }
        framesRun += 1;

        // Drain the audio every frame. The APU queues samples until someone
        // takes them, so a front end that never drains would grow the queue
        // without bound. Here the samples are only measured, which is how this
        // tool can say whether the game is making noise.
        const samples = emulator.takeSamples();
        if (samples.length > 0) {
            let loud = false;
            for (let i = 0; i < samples.length; i += 1) {
                const sample = samples[i];
                if (sample > peak) {
                    peak = sample;
                }
                if (sample > 0.01) {
                    loud = true;
                }
            }
            if (loud) {
                audibleFrames += 1;
            }
            if (wantAudio) {
                // takeSamples returns a view onto the emulator's scratch
                // buffer, valid only until the next call, so this copy is not
                // optional.
                audioChunks.push(samples.slice());
            }
        }

        if (snapshots.has(frame)) {
            const rgb = toRgb(emulator.framebufferBytes(), emulator.width, emulator.height);
            lastRgb = rgb;

            if (options.outdir !== null) {
                writePpm(join(options.outdir, `frame_${frame}.ppm`), rgb, emulator.width, emulator.height);
            }

            // The machine readable line. Always printed: --quiet silences the
            // commentary below, not the interface a script is reading.
            process.stdout.write(
                `hash ${frame} ${createHash('sha256').update(rgb).digest('hex')}\n`);
        }
    }

    if (options.dump !== null) {
        const rgb = lastRgb ?? toRgb(emulator.framebufferBytes(), emulator.width, emulator.height);
        writePpm(options.dump, rgb, emulator.width, emulator.height);
        if (!options.quiet) {
            process.stdout.write(`wrote          : ${options.dump}\n`);
        }
    }

    if (wantAudio) {
        const total = audioChunks.reduce((sum, chunk) => sum + chunk.length, 0);
        const all = new Float32Array(total);
        let offset = 0;
        for (const chunk of audioChunks) {
            all.set(chunk, offset);
            offset += chunk.length;
        }

        if (options.samples !== null) {
            writeSamples(options.samples, all);
        }
        if (options.wav !== null) {
            writeWav(options.wav, all, emulator.sampleRate);
        }
    }

    if (options.save !== null) {
        const state = emulator.saveState();
        if (state === null) {
            process.stderr.write('could not save a state\n');
            emulator.destroy();
            return 1;
        }
        writeFileSync(options.save, state);
        if (!options.quiet) {
            process.stdout.write(`state written  : ${options.save} (${state.length} bytes)\n`);
        }
    }

    if (!options.quiet) {
        process.stdout.write(`frames run     : ${framesRun}\n`);
        process.stdout.write(`frame counter  : ${emulator.frameCount}\n`);
        process.stdout.write(`cpu cycles     : ${emulator.totalCycles}\n`);
        process.stdout.write(`cpu pc         : $${emulator.cpuPc.toString(16).toUpperCase().padStart(4, '0')}\n`);
        process.stdout.write(`halted         : ${emulator.isHalted}\n`);
        process.stdout.write(`audio peak     : ${peak.toFixed(3)}\n`);
        process.stdout.write(`frames audible : ${audibleFrames}\n`);
        process.stdout.write(`audio samples  : ${audioChunks.reduce((sum, chunk) => sum + chunk.length, 0)}\n`);
        process.stdout.write(`mapper state   : ${emulator.mapperSavesState ? 'saved' : 'NOT saved'}\n`);
    }

    emulator.destroy();
    return halted ? 1 : 0;
}

process.exit(await main());
