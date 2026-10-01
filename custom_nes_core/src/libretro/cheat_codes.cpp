#include "cheat_codes.hpp"

#include <array>
#include <cstddef>

namespace fc::libretro {

namespace {

// ---------------------------------------------------------------------------
// The Game Genie alphabet.
//
// Sixteen letters, in a deliberately scrambled order. The letters are not a
// number system: the code's characters are chosen so that a mistyped code is
// unlikely to be a different valid code, which is why the alphabet looks
// random. Position in this table is the 4 bit value each letter carries.
// ---------------------------------------------------------------------------

constexpr std::array<char, 16> kGameGenieAlphabet = {
    'A', 'P', 'Z', 'L', 'G', 'I', 'T', 'Y',
    'E', 'O', 'X', 'U', 'K', 'S', 'V', 'N',
};

/// The 4 bit value of one character, or -1 if it is not in the alphabet.
int game_genie_value(char c)
{
    if (c >= 'a' && c <= 'z') {
        c = static_cast<char>(c - 'a' + 'A');
    }
    for (std::size_t i = 0; i < kGameGenieAlphabet.size(); ++i) {
        if (kGameGenieAlphabet[i] == c) {
            return static_cast<int>(i);
        }
    }
    return -1;
}

/// One hex digit, or -1.
int hex_value(char c)
{
    if (c >= '0' && c <= '9') {
        return c - '0';
    }
    if (c >= 'a' && c <= 'f') {
        return c - 'a' + 10;
    }
    if (c >= 'A' && c <= 'F') {
        return c - 'A' + 10;
    }
    return -1;
}

/// A Game Genie code may be written with separators (the physical code is
/// printed as `XXX-XXX`), and libretro cheat files sometimes carry them.
/// Only the letters are meaningful.
std::string_view strip_separators(std::string_view code, std::array<char, 16>& buffer,
                                  std::size_t& length)
{
    length = 0;
    for (char c : code) {
        if (c == '-' || c == ' ' || c == '\t') {
            continue;
        }
        if (length >= buffer.size()) {
            // Too long to be any code this core knows; the caller will see a
            // length it does not accept.
            return std::string_view(buffer.data(), buffer.size() + 1);
        }
        buffer[length++] = c;
    }
    return std::string_view(buffer.data(), length);
}

} // namespace

// ---------------------------------------------------------------------------
// Game Genie
//
// The 16 bits of address and value are not stored in order; they are dealt
// out to the characters the way the address and data lines happened to be
// wired on the chip. The shifts below are that wiring, one character at a
// time. Reading it as anything other than a faithful copy would be a mistake:
// a code's meaning is decided by the hardware, and this is the one place where
// getting it subtly wrong produces a cheat that works on some cartridges and
// silently corrupts others.
//
// The base is $8000 because every Game Genie code patches PRG ROM, which is
// the only place the cartridge is asked for a byte.
// ---------------------------------------------------------------------------

DecodedCheat decode_game_genie(std::string_view code)
{
    std::array<char, 16> buffer{};
    std::size_t length = 0;
    const std::string_view cleaned = strip_separators(code, buffer, length);
    if (cleaned.size() != 6 && cleaned.size() != 8) {
        return {};
    }

    int n[8] = { 0, 0, 0, 0, 0, 0, 0, 0 };
    for (std::size_t i = 0; i < cleaned.size(); ++i) {
        n[i] = game_genie_value(cleaned[i]);
        if (n[i] < 0) {
            return {};
        }
    }

    u16 address = 0x8000;
    u8 value = 0;
    u8 compare = 0;

    value = static_cast<u8>(value | (n[0] & 0x07));
    value = static_cast<u8>(value | ((n[0] & 0x08) << 4));

    value = static_cast<u8>(value | ((n[1] & 0x07) << 4));
    address = static_cast<u16>(address | ((n[1] & 0x08) << 4));

    address = static_cast<u16>(address | ((n[2] & 0x07) << 4));

    address = static_cast<u16>(address | ((n[3] & 0x07) << 12));
    address = static_cast<u16>(address | (n[3] & 0x08));

    address = static_cast<u16>(address | (n[4] & 0x07));
    address = static_cast<u16>(address | ((n[4] & 0x08) << 8));

    if (cleaned.size() == 6) {
        address = static_cast<u16>(address | ((n[5] & 0x07) << 8));
        value = static_cast<u8>(value | (n[5] & 0x08));

        DecodedCheat result;
        result.ok = true;
        result.rom_patch = true;
        result.address = address;
        result.value = value;
        result.compare = -1;
        return result;
    }

    address = static_cast<u16>(address | ((n[5] & 0x07) << 8));
    compare = static_cast<u8>(compare | (n[5] & 0x08));

    compare = static_cast<u8>(compare | (n[6] & 0x07));
    compare = static_cast<u8>(compare | ((n[6] & 0x08) << 4));

    compare = static_cast<u8>(compare | ((n[7] & 0x07) << 4));
    value = static_cast<u8>(value | (n[7] & 0x08));

    DecodedCheat result;
    result.ok = true;
    result.rom_patch = true;
    result.address = address;
    result.value = value;
    result.compare = compare;
    return result;
}

// ---------------------------------------------------------------------------
// Pro Action Replay
//
// Eight hex digits, read as four bytes b0 b1 b2 b3:
//
//     address = (b1 << 8) | b2
//     value   = b3
//
// and b0 unused. That is the layout libretro-fceumm settled on in 2020 after
// its author found the other branch was the one that worked; this project
// matches the front end people actually run rather than invent a third
// reading. b0 is deliberately ignored rather than folded in, because giving
// it a meaning would turn a code that works elsewhere into one that does
// something different here.
// ---------------------------------------------------------------------------

DecodedCheat decode_pro_action_replay(std::string_view code)
{
    if (code.size() != 8) {
        return {};
    }

    int byte[4] = { 0, 0, 0, 0 };
    for (int i = 0; i < 4; ++i) {
        const int hi = hex_value(code[static_cast<std::size_t>(i) * 2]);
        const int lo = hex_value(code[static_cast<std::size_t>(i) * 2 + 1]);
        if (hi < 0 || lo < 0) {
            return {};
        }
        byte[i] = (hi << 4) | lo;
    }

    DecodedCheat result;
    result.ok = true;
    result.rom_patch = false;
    result.address = static_cast<u16>(byte[2] | (byte[1] << 8));
    result.value = static_cast<u8>(byte[3]);
    result.compare = -1;
    return result;
}

DecodedCheat decode_cheat(std::string_view code)
{
    DecodedCheat decoded = decode_game_genie(code);
    if (decoded.ok) {
        return decoded;
    }
    return decode_pro_action_replay(code);
}

} // namespace fc::libretro
