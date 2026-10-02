#!/usr/bin/env bash
# Cross-build the C++/Win32 host with MinGW, from macOS/Linux.
#
# This is a smoke-test / pre-flight path, **not** the product build (that is
# `windows/scripts/build.bat` with MSVC). It produces a self-contained
# `windows/build-mingw/cgb-win.exe` (statically linked, no GCC runtime DLLs)
# that runs in a Windows VM. Requires `brew install mingw-w64`.
#
# The Rust side is cross-compiled for `x86_64-pc-windows-gnu`; `cc` uses the
# MinGW toolchain for the C dependencies (rusqlite, ring, …).
#
#   windows/scripts/cross-build-mingw.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TARGET=x86_64-pc-windows-gnu
CXX="${CXX_WINDOWS:-x86_64-w64-mingw32-g++}"
OUT="$ROOT/windows/build-mingw"

if ! command -v "$CXX" >/dev/null 2>&1; then
    echo "missing $CXX (macOS: brew install mingw-w64)" >&2
    exit 1
fi

export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER="${CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER:-x86_64-w64-mingw32-gcc}"
export CC_x86_64_pc_windows_gnu="${CC_x86_64_pc_windows_gnu:-x86_64-w64-mingw32-gcc}"
export CXX_x86_64_pc_windows_gnu="${CXX_x86_64_pc_windows_gnu:-$CXX}"
export AR_x86_64_pc_windows_gnu="${AR_x86_64_pc_windows_gnu:-x86_64-w64-mingw32-ar}"

cd "$ROOT"
cargo build --target "$TARGET"

mkdir -p "$OUT"
"$CXX" -std=c++17 -g -O0 \
    -DUNICODE -D_UNICODE -DNOMINMAX -D_WIN32_WINNT=0x0A00 -DWINVER=0x0A00 \
    -I windows/src -I src/native/include \
    windows/src/main.cpp \
    windows/src/WinWindow.cpp \
    windows/src/Input.cpp \
    windows/src/Gamepads.cpp \
    windows/src/LaunchOptions.cpp \
    -L "target/$TARGET/debug" -lcgb_app \
    -luser32 -lgdi32 -lshell32 -lole32 -loleaut32 -ladvapi32 -lcomctl32 -limm32 \
    -lws2_32 -lbcrypt -luserenv -lntdll -lxinput1_4 -lpropsys \
    -ld3dcompiler -lpathcch -lruntimeobject -lopengl32 -ldxgi -ld3d12 -ld3d11 \
    -static \
    -municode -mwindows \
    -o "$OUT/cgb-win.exe"

echo "built: $OUT/cgb-win.exe"
