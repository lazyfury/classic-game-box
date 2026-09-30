// swift-tools-version:5.9
//
// Experimental Swift/macOS front end: AppKit window + MTKView renderer, with
// the emulator (libretro host, audio, save paths) linked in from Rust via the
// `cgb-ffi` C ABI. No winit, no wgpu, no igui on this path.
//
// Build the Rust side first:
//     cargo build -p cgb-ffi            # -> target/debug/libcgb_ffi.a
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
        .macOS(.v13)
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
                .unsafeFlags(["-L", rustLibDir, "-lcgb_mac"]),
                .linkedLibrary("c++"),
                .linkedFramework("AppKit"),
                .linkedFramework("Metal"),
                .linkedFramework("MetalKit"),
                .linkedFramework("QuartzCore"),
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
