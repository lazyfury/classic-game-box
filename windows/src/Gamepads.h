// XInput → libretro gamepad snapshots.
//
// Mirrors `macos/Sources/ClassicGameBoxMac/Gamepads.swift`: the shell reads the
// native controller API and writes a snapshot through the C ABI; Rust applies
// it to its `InputState` once per frame. XInput uses the same layout as an Xbox
// controller, so the mapping is the canonical one.
#pragma once

#include <windows.h>

struct CgbWinApp;

class Gamepads {
public:
    // Read both ports and update the snapshot. Cheap; call once per frame.
    void Poll(CgbWinApp* app);

private:
    bool connected_[2] = {false, false};
};
