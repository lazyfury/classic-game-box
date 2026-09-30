// Launch arguments.
//
// Usage: `cgb-mac [--library-dir <path>] [rom]`
//
// Unlike the first cut of this experiment, there is no core path here: the Rust
// side owns core selection (`cores/cores.json` + the downloaded-core registry),
// exactly as the main app does.

import Foundation

struct LaunchOptions {
    let rom: String?
    let libraryDir: String?

    static func parse(_ arguments: [String]) -> LaunchOptions {
        var rom: String?
        var libraryDir: String?
        var index = 1
        while index < arguments.count {
            let argument = arguments[index]
            switch argument {
            case "--library-dir":
                if index + 1 < arguments.count {
                    libraryDir = arguments[index + 1]
                    index += 2
                } else {
                    index += 1
                }
            case "-h", "--help":
                FileHandle.standardError.write(
                    Data("用法：cgb-mac [--library-dir <path>] [rom]\n".utf8)
                )
                exit(0)
            default:
                if rom == nil {
                    rom = argument
                }
                index += 1
            }
        }
        return LaunchOptions(rom: rom, libraryDir: libraryDir)
    }
}
