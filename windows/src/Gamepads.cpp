#ifndef _WIN32_WINNT
#define _WIN32_WINNT 0x0A00
#endif

#include "Gamepads.h"

#include <xinput.h>

#include "cgb_host.h"

namespace {

// Past this the stick also presses the matching D-pad direction, so a core
// that only reads buttons still moves.
constexpr SHORT kStickDpad = 16384;

}  // namespace

void Gamepads::Poll(CgbHostApp* app) {
    if (!app) {
        return;
    }
    for (DWORD port = 0; port < 2; ++port) {
        XINPUT_STATE state = {};
        DWORD result = XInputGetState(port, &state);
        bool now = (result == ERROR_SUCCESS);
        if (now != connected_[port]) {
            connected_[port] = now;
            cgb_host_gamepad_connected(app, port, now);
        }
        if (!now) {
            continue;
        }

        const XINPUT_GAMEPAD& pad = state.Gamepad;
        uint32_t buttons = 0;
        auto set = [&](WORD mask, int bit) {
            if (pad.wButtons & mask) {
                buttons |= (1u << bit);
            }
        };
        set(XINPUT_GAMEPAD_A, CGB_JOYPAD_B);
        set(XINPUT_GAMEPAD_B, CGB_JOYPAD_A);
        set(XINPUT_GAMEPAD_X, CGB_JOYPAD_Y);
        set(XINPUT_GAMEPAD_Y, CGB_JOYPAD_X);
        set(XINPUT_GAMEPAD_LEFT_SHOULDER, CGB_JOYPAD_L);
        set(XINPUT_GAMEPAD_RIGHT_SHOULDER, CGB_JOYPAD_R);
        set(XINPUT_GAMEPAD_LEFT_THUMB, CGB_JOYPAD_L3);
        set(XINPUT_GAMEPAD_RIGHT_THUMB, CGB_JOYPAD_R3);
        set(XINPUT_GAMEPAD_START, CGB_JOYPAD_START);
        set(XINPUT_GAMEPAD_BACK, CGB_JOYPAD_SELECT);
        if (pad.bLeftTrigger > XINPUT_GAMEPAD_TRIGGER_THRESHOLD) {
            buttons |= (1u << CGB_JOYPAD_L2);
        }
        if (pad.bRightTrigger > XINPUT_GAMEPAD_TRIGGER_THRESHOLD) {
            buttons |= (1u << CGB_JOYPAD_R2);
        }

        if (pad.sThumbLY > kStickDpad) {
            buttons |= (1u << CGB_JOYPAD_UP);
        }
        if (pad.sThumbLY < -kStickDpad) {
            buttons |= (1u << CGB_JOYPAD_DOWN);
        }
        if (pad.sThumbLX < -kStickDpad) {
            buttons |= (1u << CGB_JOYPAD_LEFT);
        }
        if (pad.sThumbLX > kStickDpad) {
            buttons |= (1u << CGB_JOYPAD_RIGHT);
        }

        // libretro's Y axis is positive down; XInput's is positive up.
        int16_t lx = static_cast<int16_t>(pad.sThumbLX);
        int16_t ly = static_cast<int16_t>(0 - static_cast<int>(pad.sThumbLY));
        int16_t rx = static_cast<int16_t>(pad.sThumbRX);
        int16_t ry = static_cast<int16_t>(0 - static_cast<int>(pad.sThumbRY));
        cgb_host_gamepad_state(app, port, buttons, lx, ly, rx, ry);
    }
}
