// ---------------------------------------------------------------------------
// `pnpm run build:native`
//
// Builds the gamepad helper and puts it at native/bin/, the fixed path the
// main process spawns. There are two helpers and one protocol:
//
//   macOS    native/gamepad/      a Swift package, Apple's GameController
//   Windows  native/gamepad-cpp/  C++, XInput
//
// This is a Node script rather than a shell script because package.json runs
// `node scripts/...` the same way on every platform, whereas `bash` on
// Windows is a thing the user may or may not have installed.
//
//   node scripts/build-native.mjs          # the platform's helper
//   node scripts/build-native.mjs --cpp    # force the C++ build (any platform)
//
// The C++ build is the real one on Windows. On macOS it builds the portable
// half with the empty backend, which is useful for compiling and testing that
// half without a Windows machine -- not for reading pads. It does not replace
// the Swift helper in native/bin/.
// ---------------------------------------------------------------------------

import { spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const electronRoot = fileURLToPath(new URL('..', import.meta.url));
const nativeDir = join(electronRoot, 'native');
const binDir = join(nativeDir, 'bin');

/** Everything this script says is prefixed, so `grep '\[native\]'` sees it. */
function say(message) {
    console.log(`[native] ${message}`);
}

function fail(message) {
    console.error(`[native] ${message}`);
    process.exit(1);
}

/** True when the program can be found on PATH. `spawnSync` reports ENOENT. */
function have(program, args = ['--version']) {
    const result = spawnSync(program, args, { stdio: 'ignore', shell: false });
    return result.error === undefined && result.status === 0;
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

/** Copy the built binary to the one fixed name the main process knows. */
function install(from, name) {
    if (!existsSync(from)) {
        fail(`the build did not produce ${from}`);
    }
    mkdirSync(binDir, { recursive: true });
    const to = join(binDir, name);
    copyFileSync(from, to);
    say(`wrote ${to}`);
}

// ---------------------------------------------------------------------------
// macOS: the Swift helper.
// ---------------------------------------------------------------------------

function buildSwift() {
    if (!have('swift')) {
        fail('swift not found: install the Xcode command line tools (xcode-select --install)');
    }
    say('building fc-gamepad -- swift (GameController)');
    run('swift', ['build', '--package-path', join(nativeDir, 'gamepad'), '-c', 'release']);
    install(join(nativeDir, 'gamepad', '.build', 'release', 'fc-gamepad'), 'fc-gamepad');
}

// ---------------------------------------------------------------------------
// Windows (and anywhere else, for the portable half): the C++ helper.
// ---------------------------------------------------------------------------

function buildCpp({ install: shouldInstall }) {
    if (!have('cmake', ['--version'])) {
        fail('cmake not found: install CMake, or build the Swift helper on macOS');
    }

    const sourceDir = join(nativeDir, 'gamepad-cpp');
    const buildDir = join(sourceDir, 'build');
    const executable = process.platform === 'win32' ? 'fc-gamepad.exe' : 'fc-gamepad';

    say(`building fc-gamepad -- c++ (${process.platform === 'win32' ? 'XInput' : 'portable stub'})`);
    run('cmake', ['-S', sourceDir, '-B', buildDir, '-DCMAKE_BUILD_TYPE=Release']);
    run('cmake', ['--build', buildDir, '--config', 'Release']);

    // A deterministic output path, set up in the CMakeLists; the fallbacks
    // are for a hand-run build before that property existed.
    const candidates = [
        join(buildDir, 'out', executable),
        join(buildDir, 'Release', executable),
        join(buildDir, executable),
    ];
    const built = candidates.find((path) => existsSync(path));
    if (built === undefined) {
        fail(`the build produced none of: ${candidates.join(', ')}`);
    }

    if (!shouldInstall) {
        // macOS keeps the Swift helper. Copying the stub over it would be a
        // working gamepad replaced by one that reads nothing, which is the
        // worst possible thing for a build flag to do quietly.
        say(`built ${built} (not installed: this platform uses the Swift helper)`);
        return;
    }
    install(built, executable);
}

// ---------------------------------------------------------------------------

const forceCpp = process.argv.includes('--cpp');

if (forceCpp) {
    buildCpp({ install: process.platform !== 'darwin' });
} else if (process.platform === 'darwin') {
    buildSwift();
} else {
    buildCpp({ install: true });
}
