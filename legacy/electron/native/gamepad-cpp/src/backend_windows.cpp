// ---------------------------------------------------------------------------
// Windows: XInput.
//
// XInput is the system's own view of an Xbox-style controller, and it is what
// a Windows player almost always has: an Xbox pad, or a third-party pad that
// has an XInput mode. It is a plain C API in xinput1_4.dll (xinput1_3.dll on
// older systems), so there is nothing to ship and nothing to install.
//
// The mapping is the console's, and deliberately the same eight switches as
// the Swift helper and the renderer's browser GamepadSource, so a pad behaves
// the same whichever path is running:
//
//   NES A      XInput A          (the bottom face button, where A is printed)
//   NES B      XInput B          (the right face button)
//   NES Start  XInput START
//   NES Select XInput BACK
//   NES D-pad  the d-pad, with the left stick as a duplicate
//
// The stick duplicates the d-pad because most people reach for the stick. The
// deadzone is 0.5 because a worn stick drifts and a drifting stick walks the
// player into a wall.
//
// XInput's Y axis is +1 up and -1 down, which matches the GameController
// framework and is the opposite of the browser's Gamepad API; the sign is
// handled here and nowhere else.
//
// What XInput does not cover: a DualShock or DualSense in its native mode,
// which speaks HID rather than XInput (on Windows it appears as a gamepad with
// no name and, usually, no input). Windows.Gaming.Input would cover those,
// but it is WinRT and a much larger surface than this helper needs; the honest
// answer for now is a note in the log rather than a guess.
// ---------------------------------------------------------------------------

#ifdef _WIN32

#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <xinput.h>

#include <string>
#include <utility>
#include <vector>

#include "backend.h"

namespace fc {
namespace {

/// How far the stick has to move before it counts as a direction.
constexpr float kStickDeadzone = 0.5f;

/// XInputGetState on an empty slot is documented to be slow, and the usual
/// advice is not to ask every frame. Connected slots are read every poll;
/// empty ones are probed this often, which is half a second at 60Hz -- fast
/// enough that plugging a pad in feels immediate.
constexpr unsigned kProbeEvery = 30;

/// The four device classes XInput names, as a name a person can read. The
/// capabilities call is the only place XInput says anything about the shape
/// of the device; there is no product string in the API.
std::string nameForSubtype(BYTE subtype) {
    switch (subtype) {
#ifdef XINPUT_DEVSUBTYPE_ARCADE_STICK
        case XINPUT_DEVSUBTYPE_ARCADE_STICK: return "Xbox Arcade Stick";
#endif
#ifdef XINPUT_DEVSUBTYPE_WHEEL
        case XINPUT_DEVSUBTYPE_WHEEL: return "Xbox Wheel";
#endif
#ifdef XINPUT_DEVSUBTYPE_FLIGHT_STICK
        case XINPUT_DEVSUBTYPE_FLIGHT_STICK: return "Xbox Flight Stick";
#endif
#ifdef XINPUT_DEVSUBTYPE_DANCE_PAD
        case XINPUT_DEVSUBTYPE_DANCE_PAD: return "Xbox Dance Pad";
#endif
#ifdef XINPUT_DEVSUBTYPE_GUITAR
        case XINPUT_DEVSUBTYPE_GUITAR: return "Xbox Guitar";
#endif
#ifdef XINPUT_DEVSUBTYPE_GUITAR_ALTERNATE
        case XINPUT_DEVSUBTYPE_GUITAR_ALTERNATE: return "Xbox Guitar";
#endif
#ifdef XINPUT_DEVSUBTYPE_GUITAR_BASS
        case XINPUT_DEVSUBTYPE_GUITAR_BASS: return "Xbox Bass";
#endif
#ifdef XINPUT_DEVSUBTYPE_DRUM_KIT
        case XINPUT_DEVSUBTYPE_DRUM_KIT: return "Xbox Drum Kit";
#endif
        default: return "Xbox Controller";
    }
}

/// One XINPUT_GAMEPAD, as the console's eight switches.
PadButtons readButtons(const XINPUT_GAMEPAD& pad) {
    const WORD mask = pad.wButtons;

    PadButtons buttons;
    buttons.A = (mask & XINPUT_GAMEPAD_A) != 0;
    buttons.B = (mask & XINPUT_GAMEPAD_B) != 0;
    buttons.START = (mask & XINPUT_GAMEPAD_START) != 0;
    buttons.SELECT = (mask & XINPUT_GAMEPAD_BACK) != 0;
    buttons.UP = (mask & XINPUT_GAMEPAD_DPAD_UP) != 0;
    buttons.DOWN = (mask & XINPUT_GAMEPAD_DPAD_DOWN) != 0;
    buttons.LEFT = (mask & XINPUT_GAMEPAD_DPAD_LEFT) != 0;
    buttons.RIGHT = (mask & XINPUT_GAMEPAD_DPAD_RIGHT) != 0;

    // The ones the game cannot see, and the reason they are read at all: a
    // command -- pause, screenshot, save, load -- wants a button that does not
    // also press something in the game. XInput has no guide button (it is the
    // one thing the API deliberately does not hand over), so GUIDE stays up.
    buttons.FACE_X = (mask & XINPUT_GAMEPAD_X) != 0;
    buttons.FACE_Y = (mask & XINPUT_GAMEPAD_Y) != 0;
    buttons.L1 = (mask & XINPUT_GAMEPAD_LEFT_SHOULDER) != 0;
    buttons.R1 = (mask & XINPUT_GAMEPAD_RIGHT_SHOULDER) != 0;
    buttons.L3 = (mask & XINPUT_GAMEPAD_LEFT_THUMB) != 0;
    buttons.R3 = (mask & XINPUT_GAMEPAD_RIGHT_THUMB) != 0;

    // Triggers are bytes, not switches. Half way down is where a command
    // starts, because the alternative is one that fires on a brush against the
    // shoulder.
    buttons.L2 = pad.bLeftTrigger > XINPUT_GAMEPAD_TRIGGER_THRESHOLD;
    buttons.R2 = pad.bRightTrigger > XINPUT_GAMEPAD_TRIGGER_THRESHOLD;

    // The full range is -32768..32767. The asymmetry at the bottom is real
    // and costs half a step at worst, which no deadzone cares about.
    const float x = static_cast<float>(pad.sThumbLX) / 32767.0f;
    const float y = static_cast<float>(pad.sThumbLY) / 32767.0f;
    buttons.LEFT = buttons.LEFT || x < -kStickDeadzone;
    buttons.RIGHT = buttons.RIGHT || x > kStickDeadzone;
    // XInput's Y is positive upwards, so up is the positive side.
    buttons.UP = buttons.UP || y > kStickDeadzone;
    buttons.DOWN = buttons.DOWN || y < -kStickDeadzone;

    return buttons;
}

class XInputBackend final : public Backend {
public:
    std::vector<PadSample> poll() override {
        std::vector<PadSample> pads;
        for (DWORD slot = 0; slot < XUSER_MAX_COUNT; ++slot) {
            if (!connected_[slot] && (counter_ % kProbeEvery) != 0) {
                continue;
            }

            XINPUT_STATE state{};
            if (XInputGetState(slot, &state) != ERROR_SUCCESS) {
                // A pad that has gone, or an empty slot. Either way there is
                // nothing to report, and the reporter will release the slot
                // the moment this key stops appearing.
                connected_[slot] = false;
                names_[slot].clear();
                continue;
            }

            if (!connected_[slot]) {
                connected_[slot] = true;
                names_[slot] = describe(slot);
            }

            PadSample sample;
            sample.key = "xinput:" + std::to_string(slot);
            sample.id = names_[slot];
            sample.buttons = readButtons(state.Gamepad);
            pads.push_back(std::move(sample));
        }
        ++counter_;
        return pads;
    }

private:
    /// The name to show for a slot. Called once, when the pad is first seen.
    std::string describe(DWORD slot) {
        XINPUT_CAPABILITIES caps{};
        if (XInputGetCapabilities(slot, 0, &caps) == ERROR_SUCCESS) {
            return nameForSubtype(caps.SubType);
        }
        return "Xbox Controller";
    }

    bool connected_[XUSER_MAX_COUNT] = {};
    std::string names_[XUSER_MAX_COUNT];
    unsigned counter_ = 0;
};

}  // namespace

std::unique_ptr<Backend> makeBackend() {
    return std::make_unique<XInputBackend>();
}

}  // namespace fc

#endif  // _WIN32
