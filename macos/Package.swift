// swift-tools-version:5.9
//
// Experimental Swift/macOS front end: AppKit window + CAMetalLayer, with the
// whole app (igui UI, wgpu renderer, libretro host, audio) linked in from Rust
// via the `cgb_mac_*` C ABI.
//
// Build the Rust side first:
//     cargo build                # -> target/debug/libcgb_app.a  (root package)
//     swift build --package-path macos  # then this package
//
// `CGB_RUST_PROFILE` selects `debug` (default) or `release`.

import Foundation
import PackageDescription

let packageDirectory = URL(fileURLWithPath: #filePath).deletingLastPathComponent().path
let repoRoot = URL(fileURLWithPath: packageDirectory).deletingLastPathComponent().path
let rustProfile = ProcessInfo.processInfo.environment["CGB_RUST_PROFILE"] ?? "debug"
let rustLibDir = ProcessInfo.processInfo.environment["CGB_RUST_LIB_DIR"]
    ?? "\(repoRoot)/target/\(rustProfile)"

let package = Package(
    name: "ClassicGameBoxMac",
    platforms: [
        .macOS(.v14)
    ],
    products: [
        .executable(name: "cgb-mac", targets: ["ClassicGameBoxMac"])
    ],
    targets: [
        // The Rust C ABI. SwiftPM generates `module CGBNative` from the
        // umbrella header in `include/`; the symbols themselves come from the
        // Rust static library linked below.
        .target(
            name: "CGBNative",
            path: "Sources/CGBNative"
        ),
        .executableTarget(
            name: "ClassicGameBoxMac",
            dependencies: ["CGBNative"],
            path: "Sources/ClassicGameBoxMac",
            linkerSettings: [
                .unsafeFlags(["-L", rustLibDir, "-lcgb_app"]),
                .linkedLibrary("c++"),
                .linkedFramework("AppKit"),
                .linkedFramework("Metal"),
                .linkedFramework("MetalKit"),
                .linkedFramework("QuartzCore"),
                .linkedFramework("GameController"),
                // `cgb-libretro`'s offscreen GL path (hardware cores) and the
                // frameworks pulled in by the Rust `cpal` audio backend.
                .linkedFramework("OpenGL"),
                .linkedFramework("CoreAudio"),
                .linkedFramework("AudioToolbox"),
                .linkedFramework("AudioUnit"),
                .linkedFramework("CoreMIDI"),
                .linkedFramework("CoreFoundation"),
            ]
        ),
    ]
)
