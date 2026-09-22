// swift-tools-version:5.9
// ---------------------------------------------------------------------------
// fc-gamepad
//
// A tiny macOS helper that reads game controllers through Apple's
// GameController framework and reports them, one JSON object per line, on
// stdout.
//
// Why this exists at all is the whole story of the Electron front end's
// gamepad support. The browser has a Gamepad API and it works -- but on macOS
// merely touching it starts Chromium's own HID service in the browser process,
// and once that service is up the application cannot quit: `app.quit()`,
// `app.exit()` and even `process.exit()` leave the process in an
// uninterruptible wait until somebody sends SIGKILL.
//
// A separate process has none of that problem. It can be killed. And because
// it goes through the same GameController framework the retired Swift front
// end used for real pads, there is exactly one description of "how a pad maps
// onto the console's eight switches" on this side of the pipe.
//
// Read-only, standard output only. The protocol is documented in README.md.
// ---------------------------------------------------------------------------

import PackageDescription

let package = Package(
    name: "fc-gamepad",
    platforms: [.macOS(.v11)],
    targets: [
        .executableTarget(
            name: "fc-gamepad",
            path: "Sources/fc-gamepad"
        ),
    ],
    swiftLanguageVersions: [.v5]
)
