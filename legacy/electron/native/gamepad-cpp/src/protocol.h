// ---------------------------------------------------------------------------
// The wire format, and nothing else.
//
// One JSON object per line on stdout. The reader is src/main/gamepad.ts; the
// Swift helper writes the same bytes, and this file exists so the C++ one
// cannot quietly drift from it. Two messages:
//
//   {"pid":84213,"type":"hello","version":2}
//   {"type":"pads","pads":[
//        {"index":0,"id":"Xbox Controller",
//         "buttons":{"A":true,"B":false,...}}]}
//
// The key order is not part of the contract -- JSON objects are unordered and
// the reader looks keys up by name -- but it is kept the same as the Swift
// serialiser's sorted output so that a diff of two runs is readable.
// ---------------------------------------------------------------------------

#pragma once

#include <string>
#include <vector>

#include "backend.h"

namespace fc {

/// The eight names, in the order of the protocol and of the C enum on the
/// emulator's side. The whole chain -- helper, main process, renderer, core --
/// agrees on which switch is which because this is the one list.
/** Every name a reading carries: the console's eight, then the nine a command
 *  may be bound to. The order is the protocol's order and the renderer's. */
constexpr int kButtonCount = 17;
extern const char* const kButtonNames[kButtonCount];

/// One pad with its slot, as the renderer sees it.
struct PadReading {
    /// The slot, not the device. It is what the settings screen offers as
    /// "player 1" and the only name a pad has while two identical pads are
    /// connected.
    int index = 0;
    std::string id;
    PadButtons buttons;

    bool operator==(const PadReading& other) const = default;
};

/// The startup line, without a newline.
std::string helloMessage(long long pid);

/// One whole `pads` message, without a newline. Always every pad, never one
/// at a time: the list is what a disconnect is measured against.
std::string padsMessage(const std::vector<PadReading>& pads);

/// A JSON string literal, quotes included, with the characters JSON reserves
/// escaped. Device names come from the operating system and are not always
/// ASCII; UTF-8 bytes are passed through, which is what JSON wants.
std::string jsonString(const std::string& value);

}  // namespace fc
