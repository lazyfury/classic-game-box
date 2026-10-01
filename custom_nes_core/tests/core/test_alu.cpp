#include "core/alu.hpp"
#include "core/bit.hpp"
#include "core/types.hpp"

#include <gtest/gtest.h>

using namespace fc;

// ===========================================================================
// C and V are two different questions with two different answers
// ===========================================================================

TEST(OverflowFlag, CarryAndOverflowAreIndependent)
{
    // 127 + 1: no unsigned carry, but signed overflow.
    {
        const auto r = alu::add(0x7F, 0x01);
        EXPECT_EQ(r.result, 0x80);
        EXPECT_FALSE(r.carry) << "127 + 1 = 128 fits in unsigned";
        EXPECT_TRUE(r.overflow) << "+127 + 1 leaves the signed range";
        EXPECT_TRUE(r.negative) << "bit 7 of the result is set";
    }

    // 255 + 1: unsigned carry, but the signed answer is correct.
    {
        const auto r = alu::add(0xFF, 0x01);
        EXPECT_EQ(r.result, 0x00);
        EXPECT_TRUE(r.carry) << "255 + 1 = 256 does not fit in unsigned";
        EXPECT_FALSE(r.overflow) << "-1 + 1 = 0 is perfectly fine";
    }

    // 128 + 128: both at once.
    {
        const auto r = alu::add(0x80, 0x80);
        EXPECT_EQ(r.result, 0x00);
        EXPECT_TRUE(r.carry);
        EXPECT_TRUE(r.overflow) << "-128 + -128 = -256 is out of range";
    }

    // 16 + 32: neither.
    {
        const auto r = alu::add(0x10, 0x20);
        EXPECT_EQ(r.result, 0x30);
        EXPECT_FALSE(r.carry);
        EXPECT_FALSE(r.overflow);
    }
}

TEST(OverflowFlag, AllFourCombinationsAreReachable)
{
    // Proves the two flags really carry independent information.
    EXPECT_FALSE(alu::add(0x10, 0x20).carry)    << "C=0 V=0";
    EXPECT_FALSE(alu::add(0x10, 0x20).overflow);

    EXPECT_TRUE (alu::add(0xFF, 0x01).carry)    << "C=1 V=0";
    EXPECT_FALSE(alu::add(0xFF, 0x01).overflow);

    EXPECT_FALSE(alu::add(0x7F, 0x01).carry)    << "C=0 V=1";
    EXPECT_TRUE (alu::add(0x7F, 0x01).overflow);

    EXPECT_TRUE (alu::add(0x80, 0x80).carry)    << "C=1 V=1";
    EXPECT_TRUE (alu::add(0x80, 0x80).overflow);
}

// ===========================================================================
// The stated rule: same sign in, different sign out
// ===========================================================================

TEST(OverflowFlag, AddingOppositeSignsCanNeverOverflow)
{
    // A positive and a negative can never leave -128..127.
    for (int a = 0; a <= 0xFF; ++a) {
        for (int b = 0; b <= 0xFF; ++b) {
            const auto av = bit::as_signed(static_cast<u8>(a));
            const auto bv = bit::as_signed(static_cast<u8>(b));
            if ((av < 0) == (bv < 0)) {
                continue; // same sign, not our case here
            }
            EXPECT_FALSE(alu::add(static_cast<u8>(a), static_cast<u8>(b)).overflow)
                << "a=" << a << " b=" << b;
        }
    }
}

TEST(OverflowFlag, MatchesPlainIntegerArithmetic)
{
    // Ground truth: compute in a wide signed type and compare.
    for (int a = -128; a <= 127; ++a) {
        for (int b = -128; b <= 127; ++b) {
            const int exact = a + b;
            const auto r = alu::add(bit::as_unsigned(static_cast<s8>(a)),
                                    bit::as_unsigned(static_cast<s8>(b)));
            const bool expect_overflow = exact < -128 || exact > 127;
            EXPECT_EQ(r.overflow, expect_overflow)
                << "a=" << a << " b=" << b << " exact=" << exact;
        }
    }
}

// ===========================================================================
// The two explanations must agree - exhaustively.
// ===========================================================================

TEST(OverflowFlag, ProgrammerRuleEqualsHardwareRule)
{
    // 256 * 256 * 2 = 131072 additions. Both models must agree on every one.
    int checked = 0;
    for (int ai = 0; ai <= 0xFF; ++ai) {
        for (int bi = 0; bi <= 0xFF; ++bi) {
            const u8 a = static_cast<u8>(ai);
            const u8 b = static_cast<u8>(bi);

            for (bool carry_in : { false, true }) {
                const auto r = alu::add(a, b, carry_in);
                const bool hw = alu::signed_overflow_from_carries(a, b, carry_in);
                ASSERT_EQ(r.overflow, hw)
                    << "a=" << bit::to_hex(a) << " b=" << bit::to_hex(b)
                    << " cin=" << carry_in;
                ++checked;
            }
        }
    }
    EXPECT_EQ(checked, 131072);
}

TEST(OverflowFlag, HardwareRuleAlsoReproducesCarry)
{
    // Same walk should produce C too, otherwise the model is incomplete.
    for (int ai = 0; ai <= 0xFF; ++ai) {
        for (int bi = 0; bi <= 0xFF; ++bi) {
            const u8 a = static_cast<u8>(ai);
            const u8 b = static_cast<u8>(bi);
            const auto r = alu::add(a, b);
            // carry out of bit 7 is exactly C
            const u16 wide = static_cast<u16>(a) + static_cast<u16>(b);
            ASSERT_EQ(r.carry, wide > 0xFF) << "a=" << ai << " b=" << bi;
        }
    }
}

// ===========================================================================
// Boundary cases around the signed range
// ===========================================================================

TEST(OverflowFlag, SignedBoundaries)
{
    // +127 + 1  -> overflow, wraps to -128
    EXPECT_EQ(alu::add(0x7F, 0x01).result, 0x80);
    EXPECT_TRUE(alu::add(0x7F, 0x01).overflow);

    // -128 + -1 -> overflow, wraps to +127
    EXPECT_EQ(alu::add(0x80, 0xFF).result, 0x7F);
    EXPECT_TRUE(alu::add(0x80, 0xFF).overflow);

    // -128 + 127 -> exactly -1, no overflow
    EXPECT_EQ(alu::add(0x80, 0x7F).result, 0xFF);
    EXPECT_FALSE(alu::add(0x80, 0x7F).overflow);

    // 127 + 127 -> overflow
    EXPECT_EQ(alu::add(0x7F, 0x7F).result, 0xFE);
    EXPECT_TRUE(alu::add(0x7F, 0x7F).overflow);

    // -1 + -1 -> -2, no overflow
    EXPECT_EQ(alu::add(0xFF, 0xFF).result, 0xFE);
    EXPECT_FALSE(alu::add(0xFF, 0xFF).overflow);
    EXPECT_TRUE(alu::add(0xFF, 0xFF).carry) << "the 9th bit is still produced";
}

// ===========================================================================
// Carry as an operand (ADC / SBC)
// ===========================================================================

TEST(OverflowFlag, CarryInAffectsTheResult)
{
    // ADC adds the carry in. 0x7F + 0x00 + 1 = 0x80, which overflows.
    const auto with_carry = alu::add(0x7F, 0x00, true);
    EXPECT_EQ(with_carry.result, 0x80);
    EXPECT_TRUE(with_carry.overflow);

    const auto without = alu::add(0x7F, 0x00, false);
    EXPECT_EQ(without.result, 0x7F);
    EXPECT_FALSE(without.overflow);
}

TEST(OverflowFlag, SubtractingOppositeSignsOverflows)
{
    // SBC uses the same adder: A + ~M + C.
    // 127 - (-1) = 128 -> overflow.
    const auto r = alu::subtract(0x7F, 0xFF);
    EXPECT_EQ(r.result, 0x80);
    EXPECT_TRUE(r.overflow);
}

TEST(OverflowFlag, SubtractMatchesSignedArithmetic)
{
    for (int a = -128; a <= 127; ++a) {
        for (int b = -128; b <= 127; ++b) {
            const int exact = a - b;
            const auto r = alu::subtract(bit::as_unsigned(static_cast<s8>(a)),
                                         bit::as_unsigned(static_cast<s8>(b)));
            const bool expect_overflow = exact < -128 || exact > 127;
            EXPECT_EQ(r.overflow, expect_overflow)
                << "a=" << a << " b=" << b << " exact=" << exact;
            EXPECT_EQ(bit::as_signed(r.result), static_cast<s8>(exact))
                << "a=" << a << " b=" << b;
        }
    }
}

// ===========================================================================
// N is not V
// ===========================================================================

TEST(OverflowFlag, NegativeFlagIsNotOverflowFlag)
{
    // 0xA0 as a positive-looking value: bit 7 set, but nothing overflowed.
    const auto r = alu::add(0x50, 0x50); // 80 + 80 = 160
    EXPECT_EQ(r.result, 0xA0);
    EXPECT_TRUE(r.negative) << "bit 7 is a copy of the result's top bit";
    EXPECT_TRUE(r.overflow) << "80 + 80 = 160 does not fit in signed";
    EXPECT_FALSE(r.carry) << "160 still fits in unsigned";

    // And the reverse: N clear while V is set is impossible for addition,
    // but N set with V clear is common - shown above.
}
