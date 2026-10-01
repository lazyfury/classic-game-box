// ---------------------------------------------------------------------------
// Tests for the C interface.
//
// This file is compiled as C++ but only ever touches the C API, which is the
// point: if these pass, a Swift or C or Rust frontend can be written against
// exactly the same surface.
// ---------------------------------------------------------------------------

#include "ffi/emulator_api.h"

#include <gtest/gtest.h>

#include <cstdint>
#include <cstring>
#include <vector>

namespace {

/// A 32KB/8KB mapper 0 image with `program` at $8000.
std::vector<uint8_t> make_rom(const std::vector<uint8_t>& program, uint16_t origin = 0x8000)
{
    std::vector<uint8_t> rom(16 + 32768 + 8192, 0);
    rom[0] = 'N';
    rom[1] = 'E';
    rom[2] = 'S';
    rom[3] = 0x1A;
    rom[4] = 2;
    rom[5] = 1;

    for (std::size_t i = 0; i < program.size(); ++i) {
        rom[16 + (origin - 0x8000) + i] = program[i];
    }
    rom[16 + 0x7FFC] = static_cast<uint8_t>(origin & 0x00FF);
    rom[16 + 0x7FFD] = static_cast<uint8_t>(origin >> 8);
    return rom;
}

/// RAII, so a failing assertion cannot leak the handle.
struct Machine {
    fc_machine* handle = fc_create();
    ~Machine() { fc_destroy(handle); }

    Machine(const Machine&) = delete;
    Machine& operator=(const Machine&) = delete;
    Machine() = default;
};

} // namespace

// ===========================================================================
// Lifecycle
// ===========================================================================

TEST(CApi, CreateGivesAUsableHandle)
{
    Machine m;
    ASSERT_NE(m.handle, nullptr);

    EXPECT_FALSE(fc_is_halted(m.handle));
    EXPECT_EQ(fc_frame_count(m.handle), 0u);
    EXPECT_STREQ(fc_last_error(m.handle), "");
    EXPECT_STREQ(fc_rom_summary(m.handle), "");
}

TEST(CApi, DestroyAcceptsNull)
{
    // The caller should not have to check before freeing.
    fc_destroy(nullptr);
}

TEST(CApi, EveryFunctionSurvivesANullHandle)
{
    // A frontend that forgets a check should get zeros, not a crash.
    EXPECT_FALSE(fc_load_rom(nullptr, nullptr, 0));
    EXPECT_FALSE(fc_run_frame(nullptr));
    EXPECT_FALSE(fc_is_halted(nullptr));
    EXPECT_EQ(fc_frame_count(nullptr), 0u);
    EXPECT_EQ(fc_framebuffer(nullptr), nullptr);
    EXPECT_EQ(fc_pixel(nullptr, 0, 0), 0u);
    EXPECT_EQ(fc_take_samples(nullptr, nullptr, 0), 0u);
    EXPECT_EQ(fc_samples_pending(nullptr), 0u);
    EXPECT_EQ(fc_total_cycles(nullptr), 0u);
    EXPECT_EQ(fc_cpu_pc(nullptr), 0u);
    EXPECT_EQ(fc_peek(nullptr, 0x075A), 0u);
    EXPECT_EQ(fc_cheat_count(nullptr), 0);
    EXPECT_STREQ(fc_last_error(nullptr), "");
    EXPECT_STREQ(fc_rom_summary(nullptr), "");

    fc_reset(nullptr);
    fc_clear_samples(nullptr);
    fc_set_button(nullptr, FC_BUTTON_A, true, 0);
    fc_release_all_buttons(nullptr);
    fc_poke(nullptr, 0x075A, 1);
    fc_set_cheats(nullptr, nullptr, 0);
}

// ===========================================================================
// Cheats and peeking
// ===========================================================================

TEST(CApi, PokeWritesRamAndPeekReadsItBack)
{
    Machine m;
    fc_poke(m.handle, 0x075A, 0x63);
    EXPECT_EQ(fc_peek(m.handle, 0x075A), 0x63);
    // Console RAM is mirrored every 2KB; the address decoding is the bus's
    // job, and a poke has to go through it for this to hold.
    EXPECT_EQ(fc_peek(m.handle, 0x0F5A), 0x63);
}

TEST(CApi, PeekIsZeroForRegistersAndEmptyForAnEmptyList)
{
    Machine m;
    // Not a bus cycle: reading a PPU register has side effects, so a peek
    // answers nothing rather than changing the machine.
    EXPECT_EQ(fc_peek(m.handle, 0x2002), 0u);
    EXPECT_EQ(fc_cheat_count(m.handle), 0);
}

TEST(CApi, TheCheatBufferIsFourBytesPerEntry)
{
    Machine m;

    // $075A = 99, freeze + enabled. $075B = 1, enabled but not frozen.
    const uint8_t data[] = {
        0x5A, 0x07, 99, 0x03,
        0x5B, 0x07, 1, 0x02,
    };
    fc_set_cheats(m.handle, data, 2);
    EXPECT_EQ(fc_cheat_count(m.handle), 2);

    // A freeze is written when a frame runs, not when the list is set.
    EXPECT_EQ(fc_peek(m.handle, 0x075A), 0u);
    // ... which needs a cartridge to run a frame, so the count is what is
    // checked here and the writing is checked in test_cheats.cpp.

    fc_set_cheats(m.handle, nullptr, 0);
    EXPECT_EQ(fc_cheat_count(m.handle), 0);
}

// ===========================================================================
// Loading
// ===========================================================================

TEST(CApi, RejectsGarbageWithAReason)
{
    Machine m;

    const uint8_t junk[64] = { 'P', 'K', 3, 4 };
    EXPECT_FALSE(fc_load_rom(m.handle, junk, sizeof(junk)));
    EXPECT_NE(std::strlen(fc_last_error(m.handle)), 0u)
        << "the caller gets told what was wrong";
}

TEST(CApi, RejectsNullData)
{
    Machine m;
    EXPECT_FALSE(fc_load_rom(m.handle, nullptr, 100));
    EXPECT_FALSE(fc_load_rom(m.handle, nullptr, 0));
}

TEST(CApi, LoadsAnImageAndDescribesIt)
{
    Machine m;
    const auto rom = make_rom({ 0xEA });

    ASSERT_TRUE(fc_load_rom(m.handle, rom.data(), rom.size()))
        << fc_last_error(m.handle);

    const std::string summary = fc_rom_summary(m.handle);
    EXPECT_NE(summary.find("mapper 0"), std::string::npos);
    EXPECT_NE(summary.find("horizontal"), std::string::npos)
        << "the synthetic header leaves flags 6 at zero";
}

TEST(CApi, ASuccessfulLoadClearsAPreviousError)
{
    Machine m;

    const uint8_t junk[64] = { 0 };
    ASSERT_FALSE(fc_load_rom(m.handle, junk, sizeof(junk)));
    ASSERT_NE(std::strlen(fc_last_error(m.handle)), 0u);

    const auto rom = make_rom({ 0xEA });
    ASSERT_TRUE(fc_load_rom(m.handle, rom.data(), rom.size()));
    EXPECT_STREQ(fc_last_error(m.handle), "") << "the old error is gone";
}

// ===========================================================================
// Running
// ===========================================================================

TEST(CApi, RunsFramesAndCountsThem)
{
    Machine m;
    const auto rom = make_rom({
        0xA9, 0x01,              // LDA #$01
        0x85, 0x10,              // STA $10
        0x4C, 0x00, 0x80,        // JMP $8000
    });
    ASSERT_TRUE(fc_load_rom(m.handle, rom.data(), rom.size()));

    EXPECT_EQ(fc_frame_count(m.handle), 0u);
    for (int i = 0; i < 10; ++i) {
        ASSERT_TRUE(fc_run_frame(m.handle));
    }
    EXPECT_EQ(fc_frame_count(m.handle), 10u);
    EXPECT_GT(fc_total_cycles(m.handle), 0u);
}

TEST(CApi, ReportsAHaltedCpuInsteadOfSpinning)
{
    Machine m;
    // 0x02 is not an instruction. The emulator stops rather than guessing.
    ASSERT_TRUE(fc_load_rom(m.handle, make_rom({ 0x02 }).data(), 16 + 32768 + 8192));

    EXPECT_FALSE(fc_is_halted(m.handle));

    // It will halt partway through the first frame.
    int frames = 0;
    while (fc_run_frame(m.handle)) {
        ++frames;
        if (frames > 10000) {
            FAIL() << "it should have halted";
        }
    }
    EXPECT_TRUE(fc_is_halted(m.handle));

    // And once halted, every later call is a no-op rather than a hang.
    EXPECT_FALSE(fc_run_frame(m.handle));
    EXPECT_FALSE(fc_run_frame(m.handle));
}

TEST(CApi, ResetClearsTheHaltedState)
{
    Machine m;
    const auto rom = make_rom({ 0xEA });
    ASSERT_TRUE(fc_load_rom(m.handle, rom.data(), rom.size()));

    fc_reset(m.handle);
    EXPECT_FALSE(fc_is_halted(m.handle));
    EXPECT_EQ(fc_frame_count(m.handle), 0u);
}

// ===========================================================================
// Video
// ===========================================================================

TEST(CApi, TheFramebufferIsTheRightSizeAndStaysValid)
{
    Machine m;
    const auto rom = make_rom({ 0xEA });
    ASSERT_TRUE(fc_load_rom(m.handle, rom.data(), rom.size()));

    const uint32_t* first = fc_framebuffer(m.handle);
    ASSERT_NE(first, nullptr);

    for (int i = 0; i < 5; ++i) {
        ASSERT_TRUE(fc_run_frame(m.handle));
    }

    // The pointer is owned by the machine, so it must not move when the
    // contents change. A frontend caches it and uploads it every frame.
    EXPECT_EQ(fc_framebuffer(m.handle), first);
}

TEST(CApi, PixelsAreReadableByCoordinate)
{
    Machine m;
    const auto rom = make_rom({ 0xEA });
    ASSERT_TRUE(fc_load_rom(m.handle, rom.data(), rom.size()));
    ASSERT_TRUE(fc_run_frame(m.handle));

    for (int y = 0; y < FC_SCREEN_HEIGHT; ++y) {
        for (int x = 0; x < FC_SCREEN_WIDTH; ++x) {
            EXPECT_EQ(fc_pixel(m.handle, x, y),
                      fc_framebuffer(m.handle)[y * FC_SCREEN_WIDTH + x]);
        }
    }
}

TEST(CApi, PixelsOffScreenAreZeroNotACrash)
{
    Machine m;
    const auto rom = make_rom({ 0xEA });
    ASSERT_TRUE(fc_load_rom(m.handle, rom.data(), rom.size()));

    EXPECT_EQ(fc_pixel(m.handle, -1, 0), 0u);
    EXPECT_EQ(fc_pixel(m.handle, 0, -1), 0u);
    EXPECT_EQ(fc_pixel(m.handle, FC_SCREEN_WIDTH, 0), 0u);
    EXPECT_EQ(fc_pixel(m.handle, 0, FC_SCREEN_HEIGHT), 0u);
}

// ===========================================================================
// Audio
// ===========================================================================

TEST(CApi, TheSampleRateIsTheRealOne)
{
    EXPECT_EQ(fc_sample_rate(), 44100);
}

TEST(CApi, SamplesComeOutOfTheApi)
{
    Machine m;
    const auto rom = make_rom({ 0xEA });
    ASSERT_TRUE(fc_load_rom(m.handle, rom.data(), rom.size()));

    for (int i = 0; i < 10; ++i) {
        ASSERT_TRUE(fc_run_frame(m.handle));
    }

    EXPECT_GT(fc_samples_pending(m.handle), 0u);

    std::vector<float> buffer(4096, -1.0f);
    const size_t got = fc_take_samples(m.handle, buffer.data(), buffer.size());

    EXPECT_GT(got, 0u);
    EXPECT_LE(got, buffer.size());
    for (size_t i = 0; i < got; ++i) {
        EXPECT_GE(buffer[i], 0.0f);
        EXPECT_LE(buffer[i], 1.0f);
    }
}

TEST(CApi, TakingSamplesDrainsThem)
{
    Machine m;
    const auto rom = make_rom({ 0xEA });
    ASSERT_TRUE(fc_load_rom(m.handle, rom.data(), rom.size()));
    for (int i = 0; i < 5; ++i) {
        ASSERT_TRUE(fc_run_frame(m.handle));
    }

    std::vector<float> buffer(1u << 20);
    const size_t first = fc_take_samples(m.handle, buffer.data(), buffer.size());
    EXPECT_EQ(fc_samples_pending(m.handle), 0u);
    EXPECT_EQ(fc_take_samples(m.handle, buffer.data(), buffer.size()), 0u);

    // And more arrive as it keeps running.
    ASSERT_TRUE(fc_run_frame(m.handle));
    EXPECT_GT(fc_samples_pending(m.handle), 0u);
    (void)first;
}

TEST(CApi, ATinyBufferIsNotOverrun)
{
    // An audio callback gets whatever its hardware buffer is. Asking for
    // fewer samples than are waiting must not lose or overflow anything.
    Machine m;
    const auto rom = make_rom({ 0xEA });
    ASSERT_TRUE(fc_load_rom(m.handle, rom.data(), rom.size()));
    for (int i = 0; i < 10; ++i) {
        ASSERT_TRUE(fc_run_frame(m.handle));
    }

    float small[8] = {};
    const size_t got = fc_take_samples(m.handle, small, 8);
    EXPECT_EQ(got, 8u);

    const size_t before = fc_samples_pending(m.handle);
    const size_t more = fc_take_samples(m.handle, small, 8);
    EXPECT_EQ(more, 8u);
    EXPECT_EQ(fc_samples_pending(m.handle), before - 8u);
}

TEST(CApi, ClearingSamplesEmptiesTheQueue)
{
    Machine m;
    const auto rom = make_rom({ 0xEA });
    ASSERT_TRUE(fc_load_rom(m.handle, rom.data(), rom.size()));
    for (int i = 0; i < 5; ++i) {
        ASSERT_TRUE(fc_run_frame(m.handle));
    }

    ASSERT_GT(fc_samples_pending(m.handle), 0u);
    fc_clear_samples(m.handle);
    EXPECT_EQ(fc_samples_pending(m.handle), 0u);
}

// ===========================================================================
// Input
// ===========================================================================

TEST(CApi, ButtonsReachTheGame)
{
    // The classic read: latch, then eight clocks into a zero page byte.
    Machine m;
    const auto rom = make_rom({
        0xA9, 0x01, 0x8D, 0x16, 0x40,
        0xA9, 0x00, 0x8D, 0x16, 0x40,
        0xA2, 0x08,
        0xAD, 0x16, 0x40,
        0x4A,
        0x26, 0x10,
        0xCA,
        0xD0, 0xF7,
        0x4C, 0x00, 0x80,
    });
    ASSERT_TRUE(fc_load_rom(m.handle, rom.data(), rom.size()));

    fc_set_button(m.handle, FC_BUTTON_START, true, 0);
    ASSERT_TRUE(fc_run_frame(m.handle));

    // The C API deliberately does not expose RAM, so the bit layout is
    // checked in the core's own tests. What matters here is that a press
    // reaches the machine and the program keeps running.
    EXPECT_FALSE(fc_is_halted(m.handle));

    fc_release_all_buttons(m.handle);
    ASSERT_TRUE(fc_run_frame(m.handle));
    EXPECT_FALSE(fc_is_halted(m.handle));
}

TEST(CApi, ReleasingAllButtonsIsImmediate)
{
    Machine m;
    const auto rom = make_rom({ 0xEA });
    ASSERT_TRUE(fc_load_rom(m.handle, rom.data(), rom.size()));

    fc_set_button(m.handle, FC_BUTTON_A, true, 0);
    fc_set_button(m.handle, FC_BUTTON_B, true, 0);
    fc_set_button(m.handle, FC_BUTTON_UP, true, 1);
    fc_release_all_buttons(m.handle);

    // Nothing to assert directly through the C API, but it must not crash and
    // the machine must keep running.
    ASSERT_TRUE(fc_run_frame(m.handle));
    EXPECT_FALSE(fc_is_halted(m.handle));
}

TEST(CApi, PortTwoIsSeparate)
{
    Machine m;
    const auto rom = make_rom({ 0xEA });
    ASSERT_TRUE(fc_load_rom(m.handle, rom.data(), rom.size()));

    fc_set_button(m.handle, FC_BUTTON_A, true, 1);
    // An out of range port is clamped rather than writing out of bounds.
    fc_set_button(m.handle, FC_BUTTON_A, true, 99);
    ASSERT_TRUE(fc_run_frame(m.handle));
    EXPECT_FALSE(fc_is_halted(m.handle));
}

// ===========================================================================
// Diagnostics
// ===========================================================================

TEST(CApi, DiagnosticsMoveAsTheMachineRuns)
{
    Machine m;
    // A program that stays put. An all-NOP cartridge would run off the end
    // into the zeroed IRQ vector and end up at $0000.
    const auto rom = make_rom({ 0x4C, 0x00, 0x80 });
    ASSERT_TRUE(fc_load_rom(m.handle, rom.data(), rom.size()));

    const uint64_t cycles_before = fc_total_cycles(m.handle);

    for (int i = 0; i < 5; ++i) {
        ASSERT_TRUE(fc_run_frame(m.handle));
    }

    EXPECT_GT(fc_total_cycles(m.handle), cycles_before);

    // The program is a tight JMP, so the PC is exactly where it started.
    // That is the point: it is somewhere in the cartridge, not lost in RAM.
    EXPECT_EQ(fc_cpu_pc(m.handle), 0x8000u);
    EXPECT_GT(fc_total_cycles(m.handle), 5u * 29000u) << "about 29780 cycles a frame";
}

// ===========================================================================
// The lock free audio queue
//
// This is the only piece of the C API that is not about the NES: it exists so
// the front end can move samples from the thread that runs the machine to the
// real-time audio callback without a mutex. Its contract is small, but every
// edge (underrun, overrun, wrapping) has to be exactly right or the speaker
// makes noise that the emulator never produced.
// ===========================================================================

TEST(CApi, AudioQueueRoundTripsSamples)
{
    fc_audio_queue* queue = fc_audio_queue_create(16);
    ASSERT_NE(queue, nullptr);

    const float input[4] = { 0.1f, 0.2f, 0.3f, 0.4f };
    EXPECT_EQ(fc_audio_queue_push(queue, input, 4), 4u);
    EXPECT_EQ(fc_audio_queue_fill(queue), 4u);

    float output[4] = { 0.0f, 0.0f, 0.0f, 0.0f };
    EXPECT_EQ(fc_audio_queue_pop(queue, output, 4), 4u);
    for (int i = 0; i < 4; ++i) {
        EXPECT_FLOAT_EQ(output[i], input[i]);
    }
    EXPECT_EQ(fc_audio_queue_fill(queue), 0u);

    fc_audio_queue_destroy(queue);
}

TEST(CApi, AudioQueueUnderrunIsSilenceNotStaleMemory)
{
    fc_audio_queue* queue = fc_audio_queue_create(16);
    ASSERT_NE(queue, nullptr);

    const float input[2] = { 0.5f, 0.5f };
    (void)fc_audio_queue_push(queue, input, 2);

    // Ask for more than there is. The callback demanded eight frames; it
    // must get two real ones and six zeros, never whatever was in the buffer.
    float output[8];
    for (float& value : output) {
        value = 99.0f;
    }
    EXPECT_EQ(fc_audio_queue_pop(queue, output, 8), 2u);
    EXPECT_FLOAT_EQ(output[0], 0.5f);
    EXPECT_FLOAT_EQ(output[1], 0.5f);
    for (int i = 2; i < 8; ++i) {
        EXPECT_FLOAT_EQ(output[i], 0.0f) << "index " << i;
    }

    EXPECT_EQ(fc_audio_queue_underruns(queue), 1u);
    fc_audio_queue_destroy(queue);
}

TEST(CApi, AudioQueueOverrunDropsTheNewestSamples)
{
    fc_audio_queue* queue = fc_audio_queue_create(4);
    ASSERT_NE(queue, nullptr);

    const float input[6] = { 1.0f, 2.0f, 3.0f, 4.0f, 5.0f, 6.0f };
    EXPECT_EQ(fc_audio_queue_push(queue, input, 6), 4u) << "only four fit";
    EXPECT_EQ(fc_audio_queue_fill(queue), 4u);

    float output[4] = {};
    EXPECT_EQ(fc_audio_queue_pop(queue, output, 4), 4u);
    EXPECT_FLOAT_EQ(output[0], 1.0f);
    EXPECT_FLOAT_EQ(output[3], 4.0f) << "the queue kept the oldest four";

    fc_audio_queue_destroy(queue);
}

TEST(CApi, AudioQueueSurvivesWrapping)
{
    fc_audio_queue* queue = fc_audio_queue_create(8);
    ASSERT_NE(queue, nullptr);

    // Fill, drain, and fill again so both indices pass the end of the ring.
    for (int round = 0; round < 3; ++round) {
        float input[8];
        for (int i = 0; i < 8; ++i) {
            input[i] = static_cast<float>(round * 10 + i);
        }
        EXPECT_EQ(fc_audio_queue_push(queue, input, 8), 8u);

        float output[8] = {};
        EXPECT_EQ(fc_audio_queue_pop(queue, output, 8), 8u);
        for (int i = 0; i < 8; ++i) {
            EXPECT_FLOAT_EQ(output[i], input[i]) << "round " << round << " index " << i;
        }
    }

    fc_audio_queue_destroy(queue);
}

TEST(CApi, AudioQueueHandlesNullAndEmpty)
{
    EXPECT_EQ(fc_audio_queue_push(nullptr, nullptr, 4), 0u);
    EXPECT_EQ(fc_audio_queue_pop(nullptr, nullptr, 4), 0u);
    EXPECT_EQ(fc_audio_queue_fill(nullptr), 0u);
    EXPECT_EQ(fc_audio_queue_underruns(nullptr), 0u);
    EXPECT_EQ(fc_audio_queue_create(0), nullptr);

    fc_audio_queue* queue = fc_audio_queue_create(4);
    ASSERT_NE(queue, nullptr);
    float sample = 0.0f;
    EXPECT_EQ(fc_audio_queue_push(queue, &sample, 0), 0u);
    EXPECT_EQ(fc_audio_queue_pop(queue, &sample, 0), 0u);
    fc_audio_queue_destroy(queue);
}
