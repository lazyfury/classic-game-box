#include "core/bit.hpp"
#include "core/types.hpp"

#include <gtest/gtest.h>

using namespace fc;

// ---------------------------------------------------------------------------
// Bit access
// ---------------------------------------------------------------------------

TEST(Bit, TestReadsIndividualBits)
{
    const u8 value = 0b1010'0101; // bits 7,5,2,0 are set

    EXPECT_TRUE(bit::test(value, 7));
    EXPECT_FALSE(bit::test(value, 6));
    EXPECT_TRUE(bit::test(value, 5));
    EXPECT_FALSE(bit::test(value, 4));
    EXPECT_FALSE(bit::test(value, 3));
    EXPECT_TRUE(bit::test(value, 2));
    EXPECT_FALSE(bit::test(value, 1));
    EXPECT_TRUE(bit::test(value, 0));
}

TEST(Bit, SetClearToggleTouchOnlyTheTargetBit)
{
    const u8 start = 0b0000'0000;

    EXPECT_EQ(bit::set(start, 3), 0b0000'1000);
    EXPECT_EQ(bit::set(start, 7), 0b1000'0000);

    const u8 all_on = 0b1111'1111;
    EXPECT_EQ(bit::clear(all_on, 0), 0b1111'1110);
    EXPECT_EQ(bit::clear(all_on, 7), 0b0111'1111);

    EXPECT_EQ(bit::toggle(start, 0), 0b0000'0001);
    EXPECT_EQ(bit::toggle(all_on, 0), 0b1111'1110);

    // Toggling twice is the identity operation.
    EXPECT_EQ(bit::toggle(bit::toggle(start, 5), 5), start);
}

TEST(Bit, AssignIsSetOrClear)
{
    const u8 start = 0b0000'0000;
    EXPECT_EQ(bit::assign(start, 2, true), 0b0000'0100);
    EXPECT_EQ(bit::assign(0xFF, 2, false), 0b1111'1011);
}

// ---------------------------------------------------------------------------
// Nibbles and fields
// ---------------------------------------------------------------------------

TEST(Bit, NibblesAreHexDigits)
{
    const u8 value = 0xAB;
    EXPECT_EQ(bit::low_nibble(value), 0x0B);
    EXPECT_EQ(bit::high_nibble(value), 0xA0);
}

TEST(Bit, ExtractPullsOutARange)
{
    // 0b1011'0110, take bits 5..2 -> 0b1101
    const u8 value = 0b1011'0110;
    EXPECT_EQ(bit::extract(value, 5, 2), 0b0000'1101);

    // A full byte extract is the byte itself.
    EXPECT_EQ(bit::extract(value, 7, 0), value);
}

// ---------------------------------------------------------------------------
// Population count and parity (the 6502 keeps parity in flag P)
// ---------------------------------------------------------------------------

TEST(Bit, CountSetBits)
{
    EXPECT_EQ(bit::count_set_bits(0x00), 0);
    EXPECT_EQ(bit::count_set_bits(0x01), 1);
    EXPECT_EQ(bit::count_set_bits(0xFF), 8);
    EXPECT_EQ(bit::count_set_bits(0b1010'1010), 4);
}

TEST(Bit, EvenParityFlag)
{
    EXPECT_TRUE(bit::even_parity(0x00));  // 0 ones
    EXPECT_TRUE(bit::even_parity(0b0000'0011)); // 2 ones
    EXPECT_FALSE(bit::even_parity(0b0000'0001)); // 1 one
    EXPECT_TRUE(bit::even_parity(0xFF)); // 8 ones
}

// ---------------------------------------------------------------------------
// Two's complement
// ---------------------------------------------------------------------------

TEST(TwosComplement, NegateIsInvertPlusOne)
{
    EXPECT_EQ(bit::negate(u8{1}), 0xFF);
    EXPECT_EQ(bit::negate(u8{3}), 0xFD);
    EXPECT_EQ(bit::negate(u8{0}), 0x00);
    EXPECT_EQ(bit::negate(u8{0x7F}), 0x81);
}

TEST(TwosComplement, NegationIsItsOwnInverse)
{
    // -(-x) == x for every byte value.
    for (int i = 0; i <= 0xFF; ++i) {
        const u8 v = static_cast<u8>(i);
        EXPECT_EQ(bit::negate(bit::negate(v)), v) << "value = " << i;
    }
}

TEST(TwosComplement, SignedViewOfTheSameBits)
{
    EXPECT_EQ(bit::as_signed(0x00), 0);
    EXPECT_EQ(bit::as_signed(0x7F), 127);
    EXPECT_EQ(bit::as_signed(0x80), -128);
    EXPECT_EQ(bit::as_signed(0xFF), -1);

    // Round trip.
    for (int i = -128; i <= 127; ++i) {
        const s8 s = static_cast<s8>(i);
        EXPECT_EQ(bit::as_signed(bit::as_unsigned(s)), s);
    }
}

TEST(TwosComplement, SubtractionIsAdditionOfTheNegation)
{
    // 5 - 3 == 5 + (-3), computed purely with an adder.
    const u8 result = static_cast<u8>(u8{5} + bit::negate(u8{3}));
    EXPECT_EQ(result, 2);
}

TEST(Arithmetic, EightBitArithmeticWraps)
{
    // A real 8 bit ALU silently drops the 9th bit. This is not a bug,
    // it is the behaviour the carry flag exists to report.
    const u8 max = 0xFF;
    const u8 wrapped = static_cast<u8>(max + u8{1});
    EXPECT_EQ(wrapped, 0);
}

// ---------------------------------------------------------------------------
// Little endian 16 bit helpers
// ---------------------------------------------------------------------------

TEST(LittleEndian, SplitAndGlueA16BitAddress)
{
    const u16 address = 0x1234;

    EXPECT_EQ(bit::lo_byte(address), 0x34);
    EXPECT_EQ(bit::hi_byte(address), 0x12);
    EXPECT_EQ(bit::make_u16(bit::lo_byte(address), bit::hi_byte(address)), address);
}

TEST(LittleEndian, GluesInTheRightOrder)
{
    // If these were swapped we would get 0x3412 - a classic emulator bug.
    EXPECT_EQ(bit::make_u16(0x34, 0x12), 0x1234);
    EXPECT_NE(bit::make_u16(0x34, 0x12), 0x3412);
}

// ---------------------------------------------------------------------------
// Textual representation
// ---------------------------------------------------------------------------

TEST(Format, BinaryWithNibbleGrouping)
{
    EXPECT_EQ(bit::to_binary(u8{0x42}, 8, true), "0100 0010");
    EXPECT_EQ(bit::to_binary(u8{0x42}, 8, false), "01000010");
    EXPECT_EQ(bit::to_binary(u8{0x00}), "0000 0000");
    EXPECT_EQ(bit::to_binary(u8{0xFF}), "1111 1111");
}

TEST(Format, BinaryOfSixteenBits)
{
    EXPECT_EQ(bit::to_binary(u16{0x1234}, 16, true), "0001 0010 0011 0100");
    EXPECT_EQ(bit::to_binary(u16{0x1234}, 16, false), "0001001000110100");
}

TEST(Format, HexStaysPadded)
{
    EXPECT_EQ(bit::to_hex(u8{0x00}), "0x00");
    EXPECT_EQ(bit::to_hex(u8{0x42}), "0x42");
    EXPECT_EQ(bit::to_hex(u8{0xFF}), "0xFF");
    EXPECT_EQ(bit::to_hex(u16{0x0001}), "0x0001");
    EXPECT_EQ(bit::to_hex(u16{0x1234}), "0x1234");
}
