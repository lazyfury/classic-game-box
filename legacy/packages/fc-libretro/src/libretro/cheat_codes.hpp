#pragma once

// ---------------------------------------------------------------------------
// The cheat code languages a libretro front end speaks.
//
// libretro hands a core cheat codes as strings, never as the address/value
// pairs this project's own front end uses, so the core has to understand the
// two languages players actually have in front of them:
//
//   Game Genie        six or eight letters, e.g. "SXIOPO" or "GOSSIPAA"
//   Pro Action Replay eight hexadecimal digits
//
// They mean different things, and the difference matters:
//
//   * a Game Genie code patches a *ROM read*. The cartridge answers with a
//     byte, and the cheat replaces that byte when the address matches -- and,
//     for an eight letter code, only when the byte also matches a second
//     value. That is how a code can change one instruction without touching
//     code that happens to live at the same address in a different bank.
//   * a Pro Action Replay code writes to *RAM*, and stays written. It is the
//     same mechanism as the cheat panel's address/value/freeze.
//
// Neither decoder touches the machine. They turn a string into a value, which
// is what makes the bit layouts in this file testable on their own.
//
// The Game Genie layout below is the one FCEUX and every libretro NES core
// use; it is reproduced rather than invented, because a code's meaning is a
// property of the hardware, not of this project.
// ---------------------------------------------------------------------------

#include "core/types.hpp"

#include <string_view>

namespace fc::libretro {

/// What one code string means, once decoded.
struct DecodedCheat {
    /// False when the string is not a code this core understands.
    bool ok = false;

    /// True for Game Genie: replace the byte read from `address`. False for
    /// Pro Action Replay: write `value` into `address` and keep it there.
    bool rom_patch = false;

    u16 address = 0;
    u8 value = 0;

    /// For an eight letter Game Genie code, the byte the ROM must already
    /// contain for the patch to apply. -1 means "no condition", which is what
    /// a six letter code and every PAR code mean.
    int compare = -1;
};

/// Decode a six or eight letter Game Genie code. Case insensitive.
[[nodiscard]] DecodedCheat decode_game_genie(std::string_view code);

/// Decode an eight digit Pro Action Replay code.
[[nodiscard]] DecodedCheat decode_pro_action_replay(std::string_view code);

/// Try Game Genie, then Pro Action Replay.
///
/// The order matters for the rare string that is both, such as eight letters
/// from A to F: FCEUX resolves those as Game Genie and so does this, because
/// agreeing with the rest of the ecosystem is worth more than the ambiguity
/// being resolved this project's way.
[[nodiscard]] DecodedCheat decode_cheat(std::string_view code);

} // namespace fc::libretro
