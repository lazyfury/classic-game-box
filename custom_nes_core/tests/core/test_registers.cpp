#include "core/cpu/registers.hpp"

#include <gtest/gtest.h>

using namespace fc;

// ===========================================================================
// Reset state
// ===========================================================================

TEST(Registers, ResetLeavesInterruptsDisabled)
{
    Registers r;
    r.reset();

    // P = 0x24 = 0010 0100: the unused bit (5) reads as 1, I (2) is set.
    EXPECT_EQ(r.p, 0x24);
    EXPECT_TRUE(r.flag(Flag::IrqDisable));
    EXPECT_TRUE(r.flag(Flag::Unused));

    EXPECT_FALSE(r.flag(Flag::Carry));
    EXPECT_FALSE(r.flag(Flag::Zero));
    EXPECT_FALSE(r.flag(Flag::Decimal));
    EXPECT_FALSE(r.flag(Flag::Break));
    EXPECT_FALSE(r.flag(Flag::Overflow));
    EXPECT_FALSE(r.flag(Flag::Negative));
}

TEST(Registers, ResetStackPointerIsFD)
{
    Registers r;
    r.reset();

    // The reset sequence moves SP from $FF down to $FD without writing.
    EXPECT_EQ(r.sp, kResetStackPointer);
    EXPECT_EQ(r.sp, 0xFD);
}

TEST(Registers, ResetClearsTheDataRegisters)
{
    Registers r;
    r.a = 0x11;
    r.x = 0x22;
    r.y = 0x33;
    r.reset();

    EXPECT_EQ(r.a, 0);
    EXPECT_EQ(r.x, 0);
    EXPECT_EQ(r.y, 0);
}

// ===========================================================================
// Flag bit positions must match the real chip
// ===========================================================================

TEST(Registers, FlagBitPositionsMatchThe6502)
{
    Registers r;
    r.p = 0;

    struct Case { Flag flag; u8 mask; };
    const Case cases[] = {
        { Flag::Carry,      0x01 },
        { Flag::Zero,       0x02 },
        { Flag::IrqDisable, 0x04 },
        { Flag::Decimal,    0x08 },
        { Flag::Break,      0x10 },
        { Flag::Unused,     0x20 },
        { Flag::Overflow,   0x40 },
        { Flag::Negative,   0x80 },
    };

    for (const auto& c : cases) {
        r.p = 0;
        r.set_flag(c.flag, true);
        EXPECT_EQ(r.p, c.mask) << "flag at bit " << static_cast<int>(c.flag);

        r.p = 0xFF;
        r.set_flag(c.flag, false);
        EXPECT_EQ(r.p, static_cast<u8>(~c.mask)) << "clearing bit " << static_cast<int>(c.flag);
    }
}

TEST(Registers, SettingAFlagLeavesTheOthersAlone)
{
    Registers r;
    r.p = 0x24; // I and unused set

    r.set_flag(Flag::Carry, true);
    EXPECT_EQ(r.p, 0x25);

    r.set_flag(Flag::Negative, true);
    EXPECT_EQ(r.p, 0xA5);

    r.set_flag(Flag::Carry, false);
    EXPECT_EQ(r.p, 0xA4);
}

// ===========================================================================
// update_nz - the most used flag update on the 6502
// ===========================================================================

TEST(Registers, UpdateNzCopiesBitSevenIntoN)
{
    Registers r;

    r.update_nz(0x00);
    EXPECT_TRUE(r.flag(Flag::Zero));
    EXPECT_FALSE(r.flag(Flag::Negative));

    r.update_nz(0x7F);
    EXPECT_FALSE(r.flag(Flag::Zero));
    EXPECT_FALSE(r.flag(Flag::Negative));

    r.update_nz(0x80);
    EXPECT_FALSE(r.flag(Flag::Zero));
    EXPECT_TRUE(r.flag(Flag::Negative));

    r.update_nz(0xFF);
    EXPECT_FALSE(r.flag(Flag::Zero));
    EXPECT_TRUE(r.flag(Flag::Negative));
}

TEST(Registers, UpdateNzIsAFullSweep)
{
    Registers r;
    for (int i = 0; i <= 0xFF; ++i) {
        const u8 v = static_cast<u8>(i);
        r.update_nz(v);
        EXPECT_EQ(r.flag(Flag::Zero), v == 0) << "value " << i;
        EXPECT_EQ(r.flag(Flag::Negative), (v & 0x80) != 0) << "value " << i;
    }
}

TEST(Registers, UpdateNzDoesNotTouchOtherFlags)
{
    Registers r;
    r.p = 0x00;
    r.set_flag(Flag::Carry, true);
    r.set_flag(Flag::IrqDisable, true);

    r.update_nz(0x00);

    EXPECT_TRUE(r.flag(Flag::Carry)) << "C must survive";
    EXPECT_TRUE(r.flag(Flag::IrqDisable)) << "I must survive";
}

// ===========================================================================
// Debug rendering
// ===========================================================================

TEST(Registers, StatusStringShowsSetFlags)
{
    Registers r;

    r.p = 0x00;
    EXPECT_EQ(r.status_string(), "........");

    r.p = 0x24; // I and unused
    EXPECT_EQ(r.status_string(), "..-..I..");

    r.p = 0xFF;
    EXPECT_EQ(r.status_string(), "NV-BDIZC");

    // 0x83 = 1000 0011 -> N (bit 7), Z (bit 1), C (bit 0). Bit 5 is clear.
    r.p = 0x83;
    EXPECT_EQ(r.status_string(), "N.....ZC");
}
