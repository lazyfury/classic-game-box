// ---------------------------------------------------------------------------
// Cheat code decoding.
//
// The decoders turn a string into an address and a value, and nothing else, so
// this is the one part of the libretro work that can be tested without a
// machine. That matters more here than usual: a Game Genie code's meaning is a
// property of a chip that was wired a particular way, and a decoder that is
// subtly wrong produces a cheat that works on one cartridge and corrupts
// another. The vectors below pin the bit layout down.
//
// Read together with src/libretro/cheat_codes.cpp.
// ---------------------------------------------------------------------------

#include "cheat_codes.hpp"
#include "core/types.hpp"

#include <gtest/gtest.h>

using namespace fc;
using fc::libretro::DecodedCheat;
using fc::libretro::decode_cheat;
using fc::libretro::decode_game_genie;
using fc::libretro::decode_pro_action_replay;

// ---------------------------------------------------------------------------
// Game Genie
// ---------------------------------------------------------------------------

TEST(CheatCodes, SixLetterGameGenieDecodesItsAddressAndValue)
{
    // Two of the codes people actually type. The first is the example the
    // Game Genie documentation has used for decades.
    const DecodedCheat gossip = decode_game_genie("GOSSIP");
    ASSERT_TRUE(gossip.ok);
    EXPECT_TRUE(gossip.rom_patch);
    EXPECT_EQ(gossip.address, 0xD1DDu);
    EXPECT_EQ(gossip.value, 0x14u);
    // Six letters carry no compare byte, so the patch always applies.
    EXPECT_EQ(gossip.compare, -1);

    const DecodedCheat lives = decode_game_genie("SXIOPO");
    ASSERT_TRUE(lives.ok);
    EXPECT_EQ(lives.address, 0x91D9u);
    EXPECT_EQ(lives.value, 0xADu);
    EXPECT_EQ(lives.compare, -1);
}

TEST(CheatCodes, EightLetterGameGenieCarriesACompareByte)
{
    const DecodedCheat code = decode_game_genie("GOSSIPAA");
    ASSERT_TRUE(code.ok);
    EXPECT_TRUE(code.rom_patch);
    EXPECT_EQ(code.address, 0xD1DDu);
    EXPECT_EQ(code.value, 0x14u);
    EXPECT_EQ(code.compare, 0x00);

    const DecodedCheat other = decode_game_genie("SXIOPOXX");
    ASSERT_TRUE(other.ok);
    EXPECT_EQ(other.address, 0x91D9u);
    EXPECT_EQ(other.value, 0xADu);
    EXPECT_EQ(other.compare, 0xAA);
}

TEST(CheatCodes, EveryGameGenieAddressIsInPrgRom)
{
    // The base is $8000 because the Game Genie only ever patches the
    // cartridge's program. Whatever the letters, the top bit is set.
    for (const char* code : { "AAAAAA", "NNNNNN", "SXIOPO", "GOSSIPAA" }) {
        const DecodedCheat decoded = decode_game_genie(code);
        ASSERT_TRUE(decoded.ok) << code;
        EXPECT_GE(decoded.address, 0x8000u) << code;
        EXPECT_LE(decoded.address, 0xFFFFu) << code;
    }
}

TEST(CheatCodes, TheGameGenieAlphabetIsSixteenLettersAndNothingElse)
{
    EXPECT_TRUE(decode_game_genie("AAAAAA").ok);
    EXPECT_TRUE(decode_game_genie("NNNNNN").ok);
    // Case does not matter: a player types what is printed on the code.
    EXPECT_TRUE(decode_game_genie("gossip").ok);

    // Q, R, W, H and the digits are not in the alphabet, so a string with one
    // is not a code rather than a code that silently decodes to zero.
    EXPECT_FALSE(decode_game_genie("QQQQQQ").ok);
    EXPECT_FALSE(decode_game_genie("123456").ok);
    EXPECT_FALSE(decode_game_genie("HELLOO").ok);
}

TEST(CheatCodes, GameGenieRejectsTheWrongLengths)
{
    EXPECT_FALSE(decode_game_genie("GOSSI").ok);
    EXPECT_FALSE(decode_game_genie("GOSSIPX").ok);   // seven
    EXPECT_FALSE(decode_game_genie("GOSSIPZZZ").ok);
    EXPECT_FALSE(decode_game_genie("").ok);
}

TEST(CheatCodes, GameGenieIgnoresThePrintedSeparator)
{
    // A physical Game Genie code is printed as XXX-XXX.
    const DecodedCheat with_dash = decode_game_genie("GOS-SIP");
    const DecodedCheat without = decode_game_genie("GOSSIP");
    ASSERT_TRUE(with_dash.ok);
    EXPECT_EQ(with_dash.address, without.address);
    EXPECT_EQ(with_dash.value, without.value);
}

// ---------------------------------------------------------------------------
// Pro Action Replay
// ---------------------------------------------------------------------------

TEST(CheatCodes, ProActionReplayDecodesAddressAndValue)
{
    const DecodedCheat code = decode_pro_action_replay("000010A1");
    ASSERT_TRUE(code.ok);
    // Not a ROM patch: PAR writes RAM and keeps it written.
    EXPECT_FALSE(code.rom_patch);
    EXPECT_EQ(code.address, 0x0010u);
    EXPECT_EQ(code.value, 0xA1u);
    EXPECT_EQ(code.compare, -1);

    const DecodedCheat high = decode_pro_action_replay("00ABCDEF");
    ASSERT_TRUE(high.ok);
    EXPECT_EQ(high.address, 0xABCDu);
    EXPECT_EQ(high.value, 0xEFu);
}

TEST(CheatCodes, ProActionReplayIsHexAndExactlyEightDigits)
{
    EXPECT_TRUE(decode_pro_action_replay("00abcdef").ok);
    EXPECT_FALSE(decode_pro_action_replay("00ABCDE").ok);    // seven
    EXPECT_FALSE(decode_pro_action_replay("00ABCDEF0").ok);  // nine
    EXPECT_FALSE(decode_pro_action_replay("00ABCGEF").ok);   // G is not hex
    EXPECT_FALSE(decode_pro_action_replay("").ok);
}

// ---------------------------------------------------------------------------
// The dispatcher
// ---------------------------------------------------------------------------

TEST(CheatCodes, TheDispatcherTriesGameGenieFirst)
{
    // Letters outside A-F can only be a Game Genie code.
    const DecodedCheat genie = decode_cheat("SXIOPO");
    ASSERT_TRUE(genie.ok);
    EXPECT_TRUE(genie.rom_patch);
    EXPECT_EQ(genie.address, 0x91D9u);

    // Digits can only be a PAR code.
    const DecodedCheat par = decode_cheat("000010A1");
    ASSERT_TRUE(par.ok);
    EXPECT_FALSE(par.rom_patch);
    EXPECT_EQ(par.address, 0x0010u);
}

TEST(CheatCodes, AnUnreadableStringIsNotAValidCheat)
{
    EXPECT_FALSE(decode_cheat("NOT A CODE").ok);
    EXPECT_FALSE(decode_cheat("").ok);
}
