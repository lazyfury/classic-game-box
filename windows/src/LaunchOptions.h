// Command-line options for the C++ shell.
//
// Mirrors `macos/Sources/ClassicGameBoxMac/LaunchOptions.swift`: a library
// folder and/or a ROM to start. Parsing lives here so `main.cpp` only wires the
// window together.
#pragma once

#include <string>

struct LaunchOptions {
    std::wstring libraryDir;  // empty = none
    std::wstring rom;         // empty = none
};

// Parse a Win32 command line (no program name), supporting double-quoted
// values. Unknown flags are ignored; the first bare token is the ROM.
LaunchOptions ParseLaunchOptions(const wchar_t* cmdLine);
