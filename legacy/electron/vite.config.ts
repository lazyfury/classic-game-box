import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { resolve } from 'node:path';

const here = __dirname;
const wasmDist = resolve(here, '..', 'wasm', 'dist');

export default defineConfig({
    root: here,

    // Relative asset URLs. In production the page is served from the custom
    // `app://` protocol rather than file://, so this is not strictly required
    // any more, but it keeps the build portable to a plain web server.
    base: './',

    // fc_core.mjs and fc_core.wasm are copied straight out of the emscripten
    // build into the site root, and served from there in dev.
    //
    //   /fc_core.mjs    the module loader
    //   /fc_core.wasm   the emulator
    //
    // Pointing at wasm/dist rather than copying into public/ means there is
    // exactly one copy of the artifact: the one `./wasm/build.sh` wrote. If
    // they could drift, they eventually would.
    publicDir: wasmDist,

    plugins: [react()],

    resolve: {
        alias: {
            // wasm/emulator.mjs is shared with Node (wasm/headless.mjs), so it
            // stays outside this package and is imported by path.
            '@wasm': resolve(here, '..', 'wasm', 'emulator.mjs'),
            // The libretro core's front end half. Same shape as wasm/emulator.mjs
            // (see electron/src/renderer/wasm.d.ts), so the renderer can pick
            // either ABI without the game loop changing.
            '@libretro': resolve(here, '..', 'wasm', 'libretro.mjs'),
        },
    },

    build: {
        outDir: 'dist',
        emptyOutDir: true,
        // Electron ships its own Chromium, so there is no reason to downlevel
        // to what a browser from 2019 would accept.
        target: 'chrome130',
        sourcemap: true,
    },

    server: {
        port: 5273,
        strictPort: true,
        // The dev server's root is electron/, but the emulator's JavaScript
        // lives one directory up. Without this Vite refuses to serve it.
        fs: { allow: [resolve(here, '..')] },

        // Cross origin isolation, so that `crossOriginIsolated` is true and
        // SharedArrayBuffer exists. Audio is the reason: the emulator runs on
        // the page's thread and the speaker is fed from the audio thread, and
        // a ring buffer shared between two threads is the only way to hand
        // samples over without the audio thread ever waiting.
        //
        // All three, and matching the app:// handler in src/main/index.ts
        // exactly. COEP: require-corp makes every subresource prove it opted
        // in, and without the resource policy on the responses that proof is
        // missing -- the page loads, and silently is not isolated.
        headers: {
            'Cross-Origin-Opener-Policy': 'same-origin',
            'Cross-Origin-Embedder-Policy': 'require-corp',
            'Cross-Origin-Resource-Policy': 'same-origin',
        },
    },
});
