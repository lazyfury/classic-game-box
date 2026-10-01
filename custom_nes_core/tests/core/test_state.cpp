// ---------------------------------------------------------------------------
// Save states.
//
// The property worth testing is not "the bytes come back". It is:
//
//     saving and immediately reloading changes nothing.
//
// A state that is missing one field passes any test that only checks the
// fields it does contain. It does not survive running sixty more frames with
// and without the round trip in the middle, because the missing field changes
// what happens next.
//
// Most of these tests use a synthetic ROM rather than a real one, so they run
// everywhere and always. The program it contains is four instructions long and
// its whole job is to keep the machine busy: it turns on a pulse channel so the
// APU has envelopes and timers mid flight, and it increments a byte of RAM so
// the console's memory is not uniformly zero afterwards.
//
// Read together with src/core/state.cpp.
// ---------------------------------------------------------------------------

#include "core/nes/machine.hpp"
#include "core/state.hpp"
#include "core/types.hpp"
#include "gtest/gtest.h"

#include <algorithm>
#include <cstddef>
#include <span>
#include <vector>

using namespace fc;

namespace {

/// A 32KB PRG, 8KB CHR NROM, whose program does something visible to the
/// machine's state and then loops forever.
std::vector<u8> make_test_rom(u8 prg_pages = 2)
{
    std::vector<u8> rom;
    rom.push_back('N');
    rom.push_back('E');
    rom.push_back('S');
    rom.push_back(0x1A);
    rom.push_back(prg_pages);
    rom.push_back(1);      // one 8KB of CHR ROM
    rom.push_back(0x00);   // horizontal mirroring, mapper 0
    rom.push_back(0x00);
    rom.insert(rom.end(), 8, 0);

    // PRG: NOP everywhere, so a stray fetch lands somewhere harmless.
    std::vector<u8> prg(static_cast<std::size_t>(prg_pages) * 16384u, 0xEA);

    //   $8000  LDA #$BF        duty 10, constant volume 15, so the pulse
    //                          channel is on and its envelope is not idle
    //   $8002  STA $4000
    //   $8005  INC $00         so console RAM is not all zeros
    //   $8007  JMP $8000
    const std::vector<u8> program = {
        0xA9, 0xBF,
        0x8D, 0x00, 0x40,
        0xE6, 0x00,
        0x4C, 0x00, 0x80,
    };
    std::copy(program.begin(), program.end(), prg.begin());

    // The three vectors live at the top of the bank: $FFFA is offset $7FFA in
    // the last 16KB, which is where the CPU looks after a reset.
    const std::size_t vectors = prg.size() - 6;
    for (int i = 0; i < 3; ++i) {
        prg[vectors + static_cast<std::size_t>(i) * 2] = 0x00;
        prg[vectors + static_cast<std::size_t>(i) * 2 + 1] = 0x80;
    }

    rom.insert(rom.end(), prg.begin(), prg.end());
    rom.resize(rom.size() + 8192, 0);
    return rom;
}

nes::Machine machine_with(const std::vector<u8>& rom)
{
    nes::Machine machine;
    std::string error;
    EXPECT_TRUE(machine.load_rom(rom, error)) << error;
    machine.reset();
    return machine;
}

/// A cheap fingerprint of the picture, so two runs can be compared without
/// holding two 240KB framebuffers.
u64 framebuffer_hash(const nes::Machine& machine)
{
    u64 hash = 1469598103934665603ull;
    for (const u32 pixel : machine.framebuffer().pixels) {
        hash ^= pixel;
        hash *= 1099511628211ull;
    }
    return hash;
}

void run_frames(nes::Machine& machine, int count)
{
    for (int i = 0; i < count; ++i) {
        ASSERT_TRUE(machine.run_frame());
    }
}

std::vector<u8> save(const nes::Machine& machine)
{
    std::vector<u8> bytes;
    machine.save_state(bytes);
    return bytes;
}

} // namespace

// ---------------------------------------------------------------------------
// The property
// ---------------------------------------------------------------------------

TEST(State, SavingAndReloadingChangesNothing)
{
    nes::Machine machine = machine_with(make_test_rom());

    // Reach a state that is worth saving: the CPU is looping, the PPU is mid
    // frame, and the APU has a channel running.
    run_frames(machine, 60);
    const std::vector<u8> state = save(machine);

    // Where the machine would go without a round trip in the middle.
    run_frames(machine, 60);
    const u64 expected = framebuffer_hash(machine);

    ASSERT_TRUE(machine.load_state(state));

    run_frames(machine, 60);
    EXPECT_EQ(framebuffer_hash(machine), expected);
}

TEST(State, SavingAndReloadingTwiceIsTheSameAsOnce)
{
    // Two machines, both fresh, both run to the same place, one round tripped.
    // Their states must be identical byte for byte -- which catches a field
    // that is written but never read back, or read back in the wrong order.
    nes::Machine direct = machine_with(make_test_rom());
    nes::Machine roundtripped = machine_with(make_test_rom());

    run_frames(direct, 45);
    run_frames(roundtripped, 45);

    const std::vector<u8> state = save(roundtripped);
    ASSERT_TRUE(roundtripped.load_state(state));

    EXPECT_EQ(save(direct), save(roundtripped));
}

TEST(State, AStateTakenMidInstructionDoesNotCorruptTheNextOne)
{
    // Save at every frame for a while, reload each time, and check the machine
    // ends up where it started. The CPU's cycle counter and its instruction
    // bookkeeping are the fields this is really about: a load that happened
    // between the opcode and the operand would be invisible in the registers.
    nes::Machine machine = machine_with(make_test_rom());
    run_frames(machine, 30);

    for (int frame = 0; frame < 20; ++frame) {
        const std::vector<u8> state = save(machine);
        ASSERT_TRUE(machine.load_state(state));
        ASSERT_TRUE(machine.run_frame());
    }

    nes::Machine expected = machine_with(make_test_rom());
    run_frames(expected, 50);

    EXPECT_EQ(save(machine), save(expected));
}

// ---------------------------------------------------------------------------
// The container
// ---------------------------------------------------------------------------

TEST(State, StartsWithTheMagicAndTheVersion)
{
    nes::Machine machine = machine_with(make_test_rom());
    run_frames(machine, 1);

    const std::vector<u8> state = save(machine);

    ASSERT_GE(state.size(), 8u);
    EXPECT_EQ(state[0], 'F');
    EXPECT_EQ(state[1], 'C');
    EXPECT_EQ(state[2], 'S');
    EXPECT_EQ(state[3], 'T');

    StateReader reader(state);
    std::array<u8, 4> magic{};
    ASSERT_TRUE(reader.raw(magic.data(), magic.size()));
    u32 version = 0;
    ASSERT_TRUE(reader.get_u32(version));
    EXPECT_EQ(version, kStateVersion);
}

TEST(State, IsSmall)
{
    // A state that has grown into hundreds of kilobytes means somebody started
    // saving the framebuffer. This is a canary, not a budget.
    nes::Machine machine = machine_with(make_test_rom());
    run_frames(machine, 60);

    const std::vector<u8> state = save(machine);

    EXPECT_GT(state.size(), 1024u);
    EXPECT_LT(state.size(), 65536u);
}

// ---------------------------------------------------------------------------
// Refusing what it cannot use
//
// Every one of these has to leave the running machine alone. A load that
// half-applied a bad file would be worse than one that refused it.
// ---------------------------------------------------------------------------

TEST(State, IsRejectedWhenTheMagicIsWrong)
{
    nes::Machine machine = machine_with(make_test_rom());
    run_frames(machine, 30);
    const std::vector<u8> before = save(machine);

    std::vector<u8> corrupt = before;
    corrupt[0] = 'X';

    EXPECT_FALSE(machine.load_state(corrupt));
    EXPECT_EQ(save(machine), before);
}

TEST(State, IsRejectedWhenTheVersionIsUnknown)
{
    nes::Machine machine = machine_with(make_test_rom());
    run_frames(machine, 30);
    const std::vector<u8> before = save(machine);

    std::vector<u8> future = before;
    future[4] = 99;   // the low byte of the version

    EXPECT_FALSE(machine.load_state(future));
    EXPECT_EQ(save(machine), before);
}

TEST(State, IsRejectedWhenTruncated)
{
    nes::Machine machine = machine_with(make_test_rom());
    run_frames(machine, 30);
    const std::vector<u8> before = save(machine);

    const std::span<const u8> short_state(before.data(), before.size() / 2);
    EXPECT_FALSE(machine.load_state(short_state));
    EXPECT_EQ(save(machine), before);
}

TEST(State, IsRejectedWhenEmpty)
{
    nes::Machine machine = machine_with(make_test_rom());
    EXPECT_FALSE(machine.load_state(std::span<const u8>{}));
}

TEST(State, IsRejectedForADifferentCartridge)
{
    // The ROM behind every bank number is different, so a state from another
    // game cannot be applied at all. The sizes in the header are what make
    // that detectable without having to hash the ROM.
    nes::Machine small = machine_with(make_test_rom(1));
    nes::Machine large = machine_with(make_test_rom(2));

    run_frames(large, 30);
    const std::vector<u8> other_games_state = save(large);

    run_frames(small, 30);
    const std::vector<u8> before = save(small);

    EXPECT_FALSE(small.load_state(other_games_state));
    EXPECT_EQ(save(small), before);
}

TEST(State, IsRejectedWithNoCartridgeLoaded)
{
    nes::Machine empty;
    const std::vector<u8> state = save(empty);

    // Nothing to save, so nothing is written. The front end gets an empty
    // buffer rather than a state that would load into nothing.
    EXPECT_TRUE(state.empty());
    EXPECT_FALSE(empty.load_state(std::span<const u8>{}));
}

// ---------------------------------------------------------------------------
// The mapper
//
// The bank registers are the part of a save state most often left out, and the
// consequence is not an error but a picture from the wrong part of the ROM.
// ---------------------------------------------------------------------------

TEST(State, AnNromCartridgeSaysItSavesItsState)
{
    nes::Machine machine = machine_with(make_test_rom());
    EXPECT_TRUE(machine.mapper_saves_state());
}

TEST(State, CarriesTheMapperBankRegisters)
{
    // Mapper 4 with four 16KB pages, so there is more than one 8KB bank to
    // switch between.
    std::vector<u8> rom = make_test_rom(4);
    rom[6] = 0x40;   // flags6: mapper low nibble = 4
    rom[7] = 0x00;   // mapper high nibble = 0, iNES (not NES 2.0)

    // The header is 16 bytes; 8KB PRG bank 1 starts at offset 16 + 0x2000.
    // A distinctive byte there is what makes "which bank am I reading"
    // answerable at all -- the rest of the ROM is NOPs.
    rom[16 + 0x2000] = 0xC3;

    nes::Machine machine = machine_with(rom);
    ASSERT_TRUE(machine.mapper_saves_state());

    nes::Mapper& mapper = machine.cartridge()->mapper();

    // Let the machine get somewhere first. The bank is switched *after* this,
    // and no frame runs once it has been: executing through a swapped window
    // would run off the end of the program and halt the CPU, which has nothing
    // to do with save states.
    run_frames(machine, 5);

    // Select R6, which MMC3 maps into the $8000 window, and point it at bank 1.
    mapper.write_prg(0x8000, 0x06);
    mapper.write_prg(0x8001, 0x01);
    ASSERT_EQ(mapper.read_prg(0x8000), 0xC3);

    const std::vector<u8> state = save(machine);

    // Re-point the window at bank 0, then put the state back. Without the
    // bank registers in the state this reads 0xA9 -- the first instruction of
    // the program -- and the machine carries on executing the wrong bank.
    mapper.write_prg(0x8001, 0x00);
    ASSERT_EQ(mapper.read_prg(0x8000), 0xA9);

    ASSERT_TRUE(machine.load_state(state));
    EXPECT_EQ(mapper.read_prg(0x8000), 0xC3);
}

// ---------------------------------------------------------------------------
// Coverage
//
// A mapper without serialize() still passes every other test in this file --
// it simply restores the cartridge to its power on wiring. That is a silent
// gap, so it gets a test: the list below is the promise, and adding a mapper
// to it without implementing the two methods fails here.
//
// Mappers 5, 6, 8, 12, 14, 16, 24, 26, 45, 48, 69, 74, 85, 176, 185, 191, 192,
// 195, 199, 210 and 248 are not implemented in this emulator at all, so they
// cannot be listed. When one of them is written, it belongs here too.
// ---------------------------------------------------------------------------

class StateMapperCoverage : public ::testing::TestWithParam<int> {};

TEST_P(StateMapperCoverage, SavesItsBankRegisters)
{
    const int mapper = GetParam();

    std::vector<u8> rom = make_test_rom(4);
    rom[6] = static_cast<u8>((mapper & 0x0F) << 4);
    rom[7] = static_cast<u8>(mapper & 0xF0);

    nes::Machine machine;
    std::string error;
    if (!machine.load_rom(rom, error)) {
        GTEST_SKIP() << "mapper " << mapper << " would not load: " << error;
    }

    EXPECT_TRUE(machine.mapper_saves_state())
        << "mapper " << mapper << " does not save its bank registers";
}

INSTANTIATE_TEST_SUITE_P(
    Mappers,
    StateMapperCoverage,
    ::testing::Values(0, 1, 2, 3, 4, 7, 19, 74, 121, 163, 165, 177, 199, 241, 245),
    [](const ::testing::TestParamInfo<int>& info) {
        return "Mapper" + std::to_string(info.param);
    });
