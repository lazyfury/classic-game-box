// ---------------------------------------------------------------------------
// `npm run dev`
//
// Vite serves the renderer, Electron shows it, and the two are wired together
// by one environment variable. Any arguments after `--` are passed on to
// Electron, so this works too:
//
//   npm run dev -- --rom "/path/to/another game.nes"
//
// Writing this as a script rather than using two terminals is not laziness:
// Electron must not start until the dev server is actually listening, and
// hardcoding a port and hoping is exactly the kind of race that turns into
// ten minutes of "why is the window blank".
//
// This process is also where the start up trace begins. It is the earliest
// thing that runs, so its start time becomes FC_BOOT_T0 and every child --
// the Vite server, Electron's main process, its preload, the renderer --
// measures itself against it. See src/shared/boot.ts. The lines below are
// deliberately in the same format as that module's, so `grep '\[boot\]'` on
// the terminal output is one continuous timeline from `npm run dev` to the
// first frame on screen.
// ---------------------------------------------------------------------------

import { spawn, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

import { createServer } from 'vite';
import electronPath from 'electron';

const electronRoot = fileURLToPath(new URL('..', import.meta.url));

// The one clock. An already-set value means this script was started by another
// one that had already begun timing; otherwise, now.
const BOOT_T0 = Number(process.env.FC_BOOT_T0) || Date.now();
process.env.FC_BOOT_T0 = String(BOOT_T0);

function bootLog(scope, phase, detail) {
    const now = Date.now();
    const at = new Date(now).toISOString().slice(11, 23);
    const elapsed = String(now - BOOT_T0).padStart(6, ' ');
    const tail = detail === undefined ? '' : `  ${detail}`;
    console.log(`[boot] ${at} +${elapsed}ms  ${scope.padEnd(9)}${phase}${tail}`);
}

bootLog('dev', 'script started', import.meta.url);

// The main process is not part of Vite's world: it is compiled by tsc into
// dist-electron, and Electron loads it from there. Without this step, editing
// src/main/index.ts changes nothing until somebody remembers to run the build
// -- and the symptom is a feature that silently does not exist, which is a
// miserable way to spend an afternoon.
bootLog('dev', 'tsc: compiling the main process...');
const compileStarted = Date.now();
// `pnpm` is `pnpm.cmd` on Windows, and `spawn` without a shell does not go
// through PATHEXT for a `.cmd`. Naming the file is the portable fix; asking
// for a shell would put a space in a path through cmd's quoting rules.
const pnpm = process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm';
const compile = spawnSync(pnpm, ['exec', 'tsc', '-p', 'tsconfig.electron.json'], {
    cwd: electronRoot,
    stdio: 'inherit',
    shell: false,
});
bootLog('dev', 'tsc: done', `${Date.now() - compileStarted}ms (exit ${compile.status ?? 'signal'})`);

if (compile.status !== 0) {
    console.error('[dev] the main process failed to compile');
    process.exit(compile.status ?? 1);
}

bootLog('dev', 'vite: createServer...');
const viteStarted = Date.now();
const server = await createServer({
    configFile: fileURLToPath(new URL('../vite.config.ts', import.meta.url)),
});
bootLog('dev', 'vite: createServer done', `${Date.now() - viteStarted}ms`);

bootLog('dev', 'vite: listen...');
const listenStarted = Date.now();
await server.listen();
bootLog('dev', 'vite: listening', `${Date.now() - listenStarted}ms`);

const url = server.resolvedUrls?.local?.[0];
if (url === undefined) {
    console.error('[dev] vite started but reported no local url');
    await server.close();
    process.exit(1);
}

console.log(`[dev] renderer  ${url}`);
console.log(`[dev] emulator  ../wasm/dist/fc_core.wasm`);
console.log('[dev] quit with ctrl-c');

bootLog('dev', 'spawning electron', `${electronPath} ${process.argv.slice(2).join(' ')}`.trim());
const electron = spawn(electronPath, ['.', ...process.argv.slice(2)], {
    cwd: electronRoot,
    stdio: 'inherit',
    env: { ...process.env, VITE_DEV_SERVER_URL: url },
});

let closing = false;
async function shutdown(code) {
    if (closing) {
        return;
    }
    closing = true;
    bootLog('dev', 'electron exited', `code=${code ?? 0}`);
    await server.close();
    process.exit(code ?? 0);
}

electron.on('exit', (code) => void shutdown(code));

// Ctrl-C reaches the whole process group, so Electron usually exits on its
// own. This is for the case where it does not.
process.on('SIGINT', () => electron.kill('SIGINT'));
process.on('SIGTERM', () => electron.kill('SIGTERM'));
