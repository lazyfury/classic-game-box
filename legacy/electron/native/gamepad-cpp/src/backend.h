// ---------------------------------------------------------------------------
// fc-gamepad -- the platform side of the helper.
//
// Everything above this file is portable: the protocol, the slot bookkeeping,
// the change detection. What differs between operating systems is only how a
// pad is found and read, and that is the whole of this interface.
//
// There are two implementations:
//
//   backend_windows.cpp   XInput. This is the one that exists because Windows
//                         needed a helper of its own.
//   backend_stub.cpp      nothing at all, for every other platform, so that
//                         the portable half can still be compiled and tested
//                         on a machine that is not Windows.
//
// The Swift helper in native/gamepad is still what macOS uses; the C++ one is
// not built there. Two implementations of one protocol is a maintenance cost,
// but it is the cheaper of the two costs: an Objective-C++ GameController
// backend would have to run a CFRunLoop to receive the framework's connect
// notifications, and getting that subtly wrong on the one platform where pads
// currently work is not worth the symmetry.
// ---------------------------------------------------------------------------

#pragma once

#include <memory>
#include <string>
#include <vector>

namespace fc {

/// One of the console's eight switches.
///
/// A struct rather than a map so that "has anything changed" is a value
/// comparison the compiler writes, rather than a field-by-field check that
/// somebody will eventually forget to extend when a ninth name appears.
struct PadButtons {
    // The console's eight. These are the game's.
    bool A = false;
    bool B = false;
    bool SELECT = false;
    bool START = false;
    bool UP = false;
    bool DOWN = false;
    bool LEFT = false;
    bool RIGHT = false;

    // And the ones the console has no switch for: the shoulders, the triggers,
    // the stick clicks, the two extra face buttons and the guide button. They
    // exist so that a command -- pause, screenshot, save, load -- can be bound
    // to a button that does not also press something in the game. Nothing in
    // the console can see them.
    bool FACE_X = false;
    bool FACE_Y = false;
    bool L1 = false;
    bool R1 = false;
    bool L2 = false;
    bool R2 = false;
    bool L3 = false;
    bool R3 = false;
    bool GUIDE = false;

    bool operator==(const PadButtons& other) const = default;
};

/// A pad, as one poll found it, before it has been given a slot.
struct PadSample {
    /// Stable identity for as long as the device stays connected. Two
    /// identical controllers report the same display name and have nothing
    /// else to tell them apart, so this is what a slot is keyed on. On
    /// Windows it is the XInput user index, which is exactly the thing XInput
    /// calls a controller.
    std::string key;

    /// The name to show a person, such as "Xbox Controller".
    std::string id;

    PadButtons buttons;
};

/// Reads whatever pads the operating system will hand over.
class Backend {
public:
    virtual ~Backend() = default;

    /// Every pad that is connected right now. Called at 60Hz.
    virtual std::vector<PadSample> poll() = 0;
};

/// The backend for the platform this was compiled for.
std::unique_ptr<Backend> makeBackend();

}  // namespace fc
