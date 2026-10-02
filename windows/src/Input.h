// Win32 message → `cgb_host_*` helpers.
//
// Mirrors the pure mappings in `src/native/input.rs`; the WndProc cases in
// `WinWindow.cpp` call these and forward to the C ABI.
#pragma once

#include <windows.h>

#include <cstdint>
#include <string>

namespace cgbinput {

// The Win32 modifier state as the ABI's bit mask (1 shift, 2 ctrl, 4 alt,
// 8 meta/Windows).
uint32_t ModifierBits();

// The key's character for the current layout, with Ctrl / Alt / Windows masked
// out (Shift is kept) so a shortcut still carries its base character. Empty
// when the key produces no text (a named key, a modifier, a function key).
std::string KeyChar(UINT vk, LPARAM lParam);

// UTF-16 → UTF-8.
std::string ToUtf8(const std::wstring& text);

// Client-area pixel coordinates from a mouse message → logical viewport points
// (origin top-left), dividing by the backing scale.
void ClientToLogical(LPARAM lParam, float scale, float* x, float* y);

}  // namespace cgbinput
