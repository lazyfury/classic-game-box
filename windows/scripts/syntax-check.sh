#!/usr/bin/env bash
# Best-effort C++ syntax/type check for the Win32 host, from macOS/Linux.
#
# It needs a Windows cross-compiler (mingw-w64) for its headers, but does NOT
# link or run — the real build is `windows/scripts/build.bat` on Windows. Use it
# as a fast pre-flight before touching the shell.
#
#   windows/scripts/syntax-check.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CXX="${CXX_WINDOWS:-x86_64-w64-mingw32-g++}"

if ! command -v "$CXX" >/dev/null 2>&1; then
    echo "missing $CXX (macOS: brew install mingw-w64)" >&2
    exit 1
fi

cd "$ROOT"
exec "$CXX" -std=c++17 -fsyntax-only -Wall -Wextra \
    -DUNICODE -D_UNICODE -DNOMINMAX -D_WIN32_WINNT=0x0A00 -DWINVER=0x0A00 \
    -I windows/src -I src/native/include \
    windows/src/main.cpp \
    windows/src/WinWindow.cpp \
    windows/src/Input.cpp \
    windows/src/LaunchOptions.cpp
