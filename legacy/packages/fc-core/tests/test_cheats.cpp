// ---------------------------------------------------------------------------
// Tests for cheats: a byte, at an address, put back.
//
// Two things are worth testing and they are different. The list itself is a
// plain container, and the interesting part of it is that applying writes
// *through the bus* -- so that RAM mirroring holds and a cheat written against
// $075A lands on the same byte as $0F5A. And the machine applies the list at
// the start of a frame, which is the only place a freeze actually happens.
//
//   ctest --test-dir build
// ---------------------------------------------------------------------------

#include "core/nes/bus.hpp"
#include "core/nes/cheats.hpp"
#include "core/nes/machine.hpp"

#include <gtest/gtest.h>

#include <cstdint>
#include <string>
#include <vector>

namespace fc::nes {
namespace {

/// A 32KB/8KB mapper 0 image with `program` at $8000. The same shape
/// test_ffi.cpp builds, for the same reason: a real .nes file, in memory.
std::vector<u8> make_rom(const std::vector<u8>& program, u16 origin = 0x8000)
{
    std::vector<u8> rom(16 + 32768 + 8192, 0);
    rom[0] = 'N';
    rom[1] = 'E';
    rom[2] = 'S';
    rom[3] = 0x1A;
    rom[4] = 2;
    rom[5] = 1;

    for (std::size_t i = 0; i < program.size(); ++i) {
        rom[16 + (origin - 0x8000) + i] = program[i];
    }
    rom[16 + 0x7FFC] = static_cast<u8>(origin & 0x00FF);
    rom[16 + 0x7FFD] = static_cast<u8>(origin >> 8);
    return rom;
}

} // namespace

// -- peek -------------------------------------------------------------------

TEST(Cheats, PeekReadsRamAndItsMirrors)
{
    NesBus bus;
    bus.write(0x075A, 0x42);

    EXPECT_EQ(bus.peek(0x075A), 0x42);
    EXPECT_EQ(bus.peek(0x0F5A), 0x42);
    EXPECT_EQ(bus.peek(0x175A), 0x42);
}

TEST(Cheats, PeekIsNotABusCycle)
{
    // Reading $2002 through the bus clears the vblank flag and reading $2007
    // advances VRAM. A debugger that changed either of those would be
    // measuring something other than what it was asked about.
    NesBus bus;
    EXPECT_EQ(bus.peek(0x2002), 0x00);
    EXPECT_EQ(bus.peek(0x2007), 0x00);
    EXPECT_EQ(bus.peek(0x4015), 0x00);
}

TEST(Cheats, PeekDoesNotBankSwitchTheCartridge)
{
    // A ROM address is not a cheat target, and on a few mappers reading one
    // changes the banks. Peek answers 0 rather than taking that risk.
    NesBus bus;
    EXPECT_EQ(bus.peek(0x8000), 0x00);
    EXPECT_EQ(bus.peek(0xFFFF), 0x00);
}

// -- the list ---------------------------------------------------------------

TEST(Cheats, ApplyWritesTheFrozenAndEnabledEntries)
{
    NesBus bus;
    CheatSet cheats;
    const Cheat list[] = {
        {0x075A, 0x63, true, true},   // frozen and enabled: written
        {0x075B, 0x01, false, true},  // a one-shot poke: apply() leaves it
        {0x075C, 0x09, true, false},  // switched off: not written
    };
    cheats.set(list);

    cheats.apply(bus);

    EXPECT_EQ(bus.peek(0x075A), 0x63);
    EXPECT_EQ(bus.peek(0x075B), 0x00);
    EXPECT_EQ(bus.peek(0x075C), 0x00);
}

TEST(Cheats, ApplyGoesThroughTheBusSoMirrorsAgree)
{
    NesBus bus;
    CheatSet cheats;
    cheats.set(std::vector<Cheat>{{0x0F5A, 0x99, true, true}});

    cheats.apply(bus);

    // Written at the mirrored address, read at the canonical one.
    EXPECT_EQ(bus.peek(0x075A), 0x99);
}

TEST(Cheats, SetReplacesTheWholeList)
{
    CheatSet cheats;
    cheats.set(std::vector<Cheat>{{0x10, 1, true, true}, {0x11, 2, true, true}});
    EXPECT_EQ(cheats.size(), 2u);
    EXPECT_EQ(cheats.at(0).address, 0x10);

    cheats.set(std::vector<Cheat>{{0x20, 3, true, true}});
    EXPECT_EQ(cheats.size(), 1u);
    EXPECT_EQ(cheats.at(0).address, 0x20);

    cheats.clear();
    EXPECT_TRUE(cheats.empty());
    EXPECT_EQ(cheats.size(), 0u);
}

// -- the machine ------------------------------------------------------------

TEST(Cheats, AFramePutsAFrozenCheatBack)
{
    Machine machine;
    std::string error;

    // $8000: JMP $8000 -- spin forever, so nothing the CPU does can be
    // mistaken for the cheat working.
    ASSERT_TRUE(machine.load_rom(make_rom({0x4C, 0x00, 0x80}), error)) << error;
    machine.reset();

    machine.cheats().set(std::vector<Cheat>{{0x075A, 0x63, true, true}});

    ASSERT_TRUE(machine.run_frame());
    EXPECT_EQ(machine.bus().peek(0x075A), 0x63);
}

TEST(Cheats, ACheatThatIsSwitchedOffDoesNothing)
{
    Machine machine;
    std::string error;
    ASSERT_TRUE(machine.load_rom(make_rom({0x4C, 0x00, 0x80}), error)) << error;
    machine.reset();

    machine.cheats().set(std::vector<Cheat>{{0x075A, 0x63, true, false}});

    ASSERT_TRUE(machine.run_frame());
    EXPECT_EQ(machine.bus().peek(0x075A), 0x00);
}

TEST(Cheats, AFrameWritesOverWhatTheGameLeftThere)
{
    Machine machine;
    std::string error;
    ASSERT_TRUE(machine.load_rom(make_rom({0x4C, 0x00, 0x80}), error)) << error;
    machine.reset();

    // The value a game would have written, and then let run down.
    machine.bus().write(0x075A, 0x02);
    EXPECT_EQ(machine.bus().peek(0x075A), 0x02);

    machine.cheats().set(std::vector<Cheat>{{0x075A, 0x63, true, true}});
    ASSERT_TRUE(machine.run_frame());

    EXPECT_EQ(machine.bus().peek(0x075A), 0x63);
}

} // namespace fc::nes
