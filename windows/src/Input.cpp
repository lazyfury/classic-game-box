#ifndef _WIN32_WINNT
#define _WIN32_WINNT 0x0A00
#endif

#include "Input.h"

#include <windowsx.h>

namespace cgbinput {

uint32_t ModifierBits() {
    uint32_t bits = 0;
    if (GetKeyState(VK_SHIFT) & 0x8000) {
        bits |= 1;
    }
    if (GetKeyState(VK_CONTROL) & 0x8000) {
        bits |= 2;
    }
    if (GetKeyState(VK_MENU) & 0x8000) {
        bits |= 4;
    }
    if ((GetKeyState(VK_LWIN) | GetKeyState(VK_RWIN)) & 0x8000) {
        bits |= 8;
    }
    return bits;
}

std::string KeyChar(UINT vk, LPARAM lParam) {
    // Function keys and modifiers never carry a typed character.
    if (vk >= VK_F1 && vk <= VK_F24) {
        return {};
    }
    switch (vk) {
        case VK_SHIFT:
        case VK_LSHIFT:
        case VK_RSHIFT:
        case VK_CONTROL:
        case VK_LCONTROL:
        case VK_RCONTROL:
        case VK_MENU:
        case VK_LMENU:
        case VK_RMENU:
        case VK_LWIN:
        case VK_RWIN:
        case VK_CAPITAL:
            return {};
        default:
            break;
    }

    BYTE state[256];
    if (GetKeyboardState(state) == FALSE) {
        return {};
    }
    // Ignore Ctrl / Alt / Windows so a shortcut still yields its base
    // character, matching macOS `charactersIgnoringModifiers`.
    state[VK_CONTROL] = 0;
    state[VK_LCONTROL] = 0;
    state[VK_RCONTROL] = 0;
    state[VK_MENU] = 0;
    state[VK_LMENU] = 0;
    state[VK_RMENU] = 0;
    state[VK_LWIN] = 0;
    state[VK_RWIN] = 0;

    UINT scancode = static_cast<UINT>((lParam >> 16) & 0xFF);
    wchar_t buffer[8] = {};
    int count = ToUnicode(vk, scancode, state, buffer, 8, 0);
    if (count <= 0) {
        return {};
    }
    return ToUtf8(std::wstring(buffer, buffer + count));
}

std::string ToUtf8(const std::wstring& text) {
    if (text.empty()) {
        return {};
    }
    int size = WideCharToMultiByte(CP_UTF8, 0, text.c_str(), static_cast<int>(text.size()),
                                   nullptr, 0, nullptr, nullptr);
    if (size <= 0) {
        return {};
    }
    std::string out(static_cast<size_t>(size), '\0');
    WideCharToMultiByte(CP_UTF8, 0, text.c_str(), static_cast<int>(text.size()),
                        out.data(), size, nullptr, nullptr);
    return out;
}

void ClientToLogical(LPARAM lParam, float scale, float* x, float* y) {
    float px = static_cast<float>(GET_X_LPARAM(lParam));
    float py = static_cast<float>(GET_Y_LPARAM(lParam));
    if (scale <= 0.0f) {
        scale = 1.0f;
    }
    *x = px / scale;
    *y = py / scale;
}

}  // namespace cgbinput
