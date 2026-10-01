#include "core/nes/apu.hpp"
#include "core/nes/bus.hpp"
#include "core/types.hpp"

#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <cstddef>
#include <vector>

using namespace fc;

namespace {

/// Enable one channel and give it a note that will not stop on its own.
/// Enable a channel and give it a note that will not stop on its own.
///
/// The order matters: $4015 must come first. Writing $4003 while the channel
/// is disabled does NOT load the length counter, so the note would be silent.
/// That is real hardware behaviour, not a quirk of this implementation, and
/// it is why every test below enables before it plays.
void play_pulse1(nes::Apu& apu, u8 duty = 2, u8 volume = 15, u16 timer = 0x100)
{
    apu.write(0x4015, 0x01);
    apu.write(0x4000, static_cast<u8>((duty << 6) | 0x20 | 0x10 | (volume & 0x0F)));
    apu.write(0x4002, static_cast<u8>(timer & 0xFF));
    apu.write(0x4003, static_cast<u8>(timer >> 8));
}

} // namespace

// ===========================================================================
// Registers
// ===========================================================================

TEST(Apu, RegistersReachTheRightChannel)
{
    nes::Apu apu;

    apu.write(0x4002, 0x34);   // pulse 1 timer low
    apu.write(0x4006, 0x56);   // pulse 2 timer low
    apu.write(0x400A, 0x78);   // triangle timer low

    EXPECT_EQ(apu.pulse1().timer(), 0x34u);
    EXPECT_EQ(apu.pulse2().timer(), 0x56u);
    EXPECT_EQ(apu.triangle().sequence_position(), 0u);
    EXPECT_NE(apu.pulse1().timer(), apu.pulse2().timer());
    (void)0;
}

TEST(Apu, WritesToUnknownRegistersDoNothing)
{
    nes::Apu apu;
    apu.write(0x4002, 0x11);
    for (u16 address = 0x4020; address < 0x4030; ++address) {
        apu.write(address, 0xFF);
    }
    EXPECT_EQ(apu.pulse1().timer(), 0x11u) << "nothing else touched it";
}

TEST(Apu, EnableRegisterTurnsChannelsOnAndOff)
{
    nes::Apu apu;

    apu.write(0x4015, 0x0F);
    EXPECT_EQ(apu.enabled_channels(), 0x0F);
    EXPECT_EQ(apu.channel_status() & 0x0F, 0x00) << "enabled is not the same as playing";

    play_pulse1(apu);
    EXPECT_EQ(apu.channel_status() & 0x01, 0x01) << "the length counter is running";

    apu.write(0x4015, 0x00);
    EXPECT_EQ(apu.channel_status() & 0x01, 0x00) << "disabling clears the length counter";
}

TEST(Apu, ReadReturnsTheStatusRegister)
{
    nes::Apu apu;
    play_pulse1(apu);
    EXPECT_EQ(apu.read(0x4015) & 0x01, 0x01);

    // Everything else is write only and returns zero for the bus to handle.
    EXPECT_EQ(apu.read(0x4000), 0x00);
    EXPECT_EQ(apu.read(0x4002), 0x00);
}

// ===========================================================================
// Pulse
// ===========================================================================

TEST(ApuPulse, OutputFollowsTheDutySequence)
{
    nes::Apu apu;

    // Duty 2 is 50%: 0 1 1 1 1 0 0 0. Constant volume 15, length halted.
    apu.write(0x4015, 0x01);   // enable first, or the length counter stays 0
    apu.write(0x4000, 0xBF);
    apu.write(0x4002, 0x08);   // timer low = 8, high enough to be audible
    apu.write(0x4003, 0x00);

    std::vector<u8> outputs;
    for (int step = 0; step < 8; ++step) {
        outputs.push_back(apu.pulse1().output());
        // One timer period is timer + 1 APU cycles.
        for (int i = 0; i < 9; ++i) {
            apu.tick();
        }
    }

    const std::vector<u8> expected = { 0, 15, 15, 15, 15, 0, 0, 0 };
    EXPECT_EQ(outputs, expected);
}

TEST(ApuPulse, EachDutySettingHasItsOwnShape)
{
    // Duty 0 is 12.5%: mostly low. Duty 3 is the same shape inverted.
    nes::Apu apu;

    const auto shape = [&](u8 duty) {
        apu.reset();
        apu.write(0x4015, 0x01);
        apu.write(0x4000, static_cast<u8>((duty << 6) | 0x20 | 0x10 | 0x0F));
        apu.write(0x4002, 0x08);
        apu.write(0x4003, 0x00);

        std::vector<u8> out;
        for (int step = 0; step < 8; ++step) {
            out.push_back(apu.pulse1().output());
            for (int i = 0; i < 9; ++i) {
                apu.tick();
            }
        }
        return out;
    };

    const std::vector<u8> duty0 = { 0, 15, 0, 0, 0, 0, 0, 0 };
    const std::vector<u8> duty2 = { 0, 15, 15, 15, 15, 0, 0, 0 };
    const std::vector<u8> duty3 = { 15, 0, 0, 15, 15, 15, 15, 0 };

    EXPECT_EQ(shape(0), duty0);
    EXPECT_EQ(shape(2), duty2);
    EXPECT_EQ(shape(3), duty3);
}

TEST(ApuPulse, LengthCounterEventuallySilencesTheChannel)
{
    nes::Apu apu;

    // Length index 0 loads 10 from the table; halt is off so it counts down.
    apu.write(0x4015, 0x01);
    apu.write(0x4000, 0x1F);   // no halt, constant volume 15
    apu.write(0x4002, 0x08);
    apu.write(0x4003, 0x00);

    EXPECT_TRUE(apu.pulse1().length_active());

    // The half frame clock runs on frame steps 2 and 4, so ten ticks take
    // about 149000 APU cycles, which is nearly 300000 CPU cycles.
    apu.tick_cpu(400000);

    EXPECT_FALSE(apu.pulse1().length_active());
    EXPECT_EQ(apu.pulse1().output(), 0);
}

TEST(ApuPulse, HaltingTheLengthCounterKeepsTheNoteGoing)
{
    nes::Apu apu;
    apu.write(0x4015, 0x01);
    apu.write(0x4000, 0x3F);   // halt set
    apu.write(0x4002, 0x08);
    apu.write(0x4003, 0x00);

    apu.tick_cpu(400000);

    EXPECT_TRUE(apu.pulse1().length_active()) << "halted notes do not expire";
}

TEST(ApuPulse, TheEnvelopeDecaysWhenTheLengthCounterIsHalted)
{
    // The envelope's decay is stepped by the quarter frame clock, and with
    // the loop flag set it restarts from 15 rather than staying at zero. That
    // is the entire "instrument" the hardware provides.
    nes::Apu apu;
    apu.write(0x4015, 0x01);
    apu.write(0x4000, 0x20 | 0x00);   // halt/loop set, constant volume off,
                                      // envelope period 0 => fastest decay
    apu.write(0x4002, 0x08);
    apu.write(0x4003, 0x00);

    std::vector<u8> levels;
    for (int i = 0; i < 200; ++i) {
        levels.push_back(apu.pulse1().output());
        apu.tick_cpu(100);
    }

    const bool saw_low = std::any_of(levels.begin(), levels.end(),
                                     [](u8 v) { return v == 0; });
    const bool saw_high = std::any_of(levels.begin(), levels.end(),
                                      [](u8 v) { return v >= 14; });
    EXPECT_TRUE(saw_high) << "the envelope starts at 15";
    EXPECT_TRUE(saw_low) << "and decays to 0";
}

TEST(ApuPulse, SilencedWhenThePeriodIsTooShort)
{
    nes::Apu apu;
    apu.write(0x4015, 0x01);
    apu.write(0x4000, 0xBF);
    apu.write(0x4002, 0x04);   // timer 4 is below the audible minimum of 8
    apu.write(0x4003, 0x00);

    EXPECT_EQ(apu.pulse1().output(), 0) << "the hardware mutes periods under 8";
}

TEST(ApuPulse, TheSweepUnitChangesThePeriod)
{
    nes::Apu apu;
    apu.write(0x4015, 0x01);
    apu.write(0x4000, 0xBF);
    apu.write(0x4001, 0x81);   // sweep on, period 0, shift 1, add
    apu.write(0x4002, 0x00);
    apu.write(0x4003, 0x01);   // timer = 0x0100

    const u16 before = apu.pulse1().timer();
    apu.tick_cpu(200000);
    const u16 after = apu.pulse1().timer();

    EXPECT_GT(after, before) << "a positive sweep should raise the period";
}

TEST(ApuPulse, PulseOneAndTwoNegateDifferently)
{
    // A real asymmetry in the silicon: pulse 1's negate subtracts one more
    // than pulse 2's. Setting both channels up the same way and running the
    // sweep shows it.
    nes::Apu apu;

    const auto run = [&](bool first) {
        apu.reset();
        const u16 base = first ? 0x4000 : 0x4004;
        apu.write(0x4015, first ? 0x01 : 0x02);
        apu.write(base + 0, 0xBF);
        apu.write(base + 1, 0x89);   // sweep on, period 0, negate, shift 1
        apu.write(base + 2, 0x00);
        apu.write(base + 3, 0x04);   // timer = 0x0400
        // Three half-frame clocks: 1024 -> 511 -> 255 -> 127 for pulse 1 and
        // 1024 -> 512 -> 256 -> 128 for pulse 2. Long enough to show the
        // difference, short enough that neither sweeps into the wrap that
        // disables the unit.
        apu.tick_cpu(45000);
        return first ? apu.pulse1().timer() : apu.pulse2().timer();
    };

    EXPECT_LT(run(true), run(false))
        << "pulse 1 sweeps down one further than pulse 2";
}

// ===========================================================================
// Triangle
// ===========================================================================

TEST(ApuTriangle, NeedsBothCountersToSound)
{
    nes::Apu apu;

    // Length counter only: the linear counter is still zero.
    apu.write(0x4015, 0x04);
    apu.write(0x4008, 0x00);   // control off, linear reload 0
    apu.write(0x400A, 0x08);
    apu.write(0x400B, 0x00);
    EXPECT_EQ(apu.triangle().output(), 0);

    // Give the linear counter a reload value and a control flag to hold it.
    apu.write(0x4008, 0xFF);   // control on, linear reload 127
    // The linear counter reloads on the quarter frame clock, which is one
    // frame step (7457 CPU cycles) away, so give it well over that.
    apu.tick_cpu(20000);
    EXPECT_GT(apu.triangle().output(), 0u)
        << "with both counters alive the triangle sounds";
}

TEST(ApuTriangle, StepsThroughItsSequence)
{
    nes::Apu apu;
    apu.write(0x4015, 0x04);
    apu.write(0x4008, 0xFF);
    apu.write(0x400A, 0x08);
    apu.write(0x400B, 0x00);
    apu.tick_cpu(20000);

    std::vector<u8> seen;
    for (int i = 0; i < 32; ++i) {
        seen.push_back(apu.triangle().output());
        for (int t = 0; t < 9; ++t) {
            apu.tick();
        }
    }

    // The sequence counts 15 down to 0 and back up. It must reach both ends.
    EXPECT_EQ(*std::max_element(seen.begin(), seen.end()), 15);
    EXPECT_EQ(*std::min_element(seen.begin(), seen.end()), 0);
}

TEST(ApuTriangle, TheLinearCounterDecaysWithoutTheControlFlag)
{
    nes::Apu apu;
    apu.write(0x4015, 0x04);
    apu.write(0x4008, 0x02);   // control off, linear reload 2
    apu.write(0x400A, 0x08);
    apu.write(0x400B, 0x00);

    apu.tick_cpu(400000);
    EXPECT_EQ(apu.triangle().output(), 0) << "the linear counter ran out";
}

// ===========================================================================
// Noise
// ===========================================================================

TEST(ApuNoise, TheShiftRegisterIsDeterministic)
{
    // Nothing here is random. Two machines set up the same way produce the
    // same "noise" forever, which is what makes recordings reproducible.
    const auto run = [] {
        nes::Apu apu;
        apu.write(0x4015, 0x08);
        apu.write(0x400C, 0x3F);
        apu.write(0x400E, 0x04);
        apu.write(0x400F, 0x00);
        apu.tick_cpu(5000);
        return apu.noise().shift_register();
    };

    EXPECT_EQ(run(), run());
}

TEST(ApuNoise, TheTwoModesUseDifferentTaps)
{
    const auto run = [](u8 mode) {
        nes::Apu apu;
        apu.write(0x4015, 0x08);
        apu.write(0x400C, 0x3F);
        apu.write(0x400E, static_cast<u8>(mode | 0x04));
        apu.write(0x400F, 0x00);
        apu.tick_cpu(5000);
        return apu.noise().shift_register();
    };

    // Mode 0 is the 15 bit sequence, mode 1 the 6 bit one. They must diverge.
    EXPECT_NE(run(0x00), run(0x80));
}

TEST(ApuNoise, ProducesBothZeroesAndOnes)
{
    nes::Apu apu;
    apu.write(0x4015, 0x08);
    apu.write(0x400C, 0x3F);
    apu.write(0x400E, 0x04);
    apu.write(0x400F, 0x00);

    bool saw_loud = false;
    bool saw_quiet = false;
    for (int i = 0; i < 400; ++i) {
        if (apu.noise().output() > 0) {
            saw_loud = true;
        } else {
            saw_quiet = true;
        }
        apu.tick_cpu(20);
    }

    EXPECT_TRUE(saw_loud);
    EXPECT_TRUE(saw_quiet) << "noise has to alternate or it is not noise";
}

// ===========================================================================
// DMC
// ===========================================================================

TEST(ApuDmc, TheDirectLoadWritesTheDac)
{
    nes::Apu apu;
    apu.write(0x4011, 0x7F);
    EXPECT_EQ(apu.dmc().output(), 0x7F);

    apu.write(0x4011, 0x00);
    EXPECT_EQ(apu.dmc().output(), 0x00);

    apu.write(0x4011, 0x40);
    EXPECT_EQ(apu.dmc().output(), 0x40) << "seven bits, not eight";
}

TEST(ApuDmc, SampleAddressAndLengthAreScaled)
{
    nes::Apu apu;

    apu.write(0x4012, 0x00);
    EXPECT_EQ(apu.dmc().sample_address(), 0xC000u);

    apu.write(0x4012, 0x01);
    EXPECT_EQ(apu.dmc().sample_address(), 0xC040u) << "value * 64, offset from $C000";

    apu.write(0x4013, 0x00);
    EXPECT_EQ(apu.dmc().bytes_remaining(), 0u) << "not started yet";

    apu.write(0x4013, 0x01);
    apu.write(0x4015, 0x10);
    EXPECT_EQ(apu.dmc().bytes_remaining(), 17u) << "value * 16 + 1";
}

TEST(ApuDmc, ReadsSampleBytesFromCpuMemory)
{
    // A bus that hands out a known ramp, so the DAC's movement is predictable.
    struct RampBus : nes::NesBus {
        [[nodiscard]] u8 read(u16 address) override
        {
            return static_cast<u8>(address & 0xFF);
        }
    };

    RampBus bus;
    nes::Apu apu;
    apu.set_memory_reader(&bus);

    apu.write(0x4010, 0x0F);   // fastest rate, no loop, no IRQ
    apu.write(0x4012, 0x00);   // $C000
    apu.write(0x4013, 0x00);   // one byte
    apu.write(0x4011, 0x40);   // start from the middle
    apu.write(0x4015, 0x10);

    const u8 before = apu.dmc().output();
    apu.tick_cpu(2000);

    EXPECT_NE(apu.dmc().output(), before) << "the DAC moved";
    EXPECT_NE(apu.dmc().current_address(), 0xC000u) << "the read head advanced";
}

TEST(ApuDmc, TheIrqFiresWhenASampleFinishes)
{
    struct SilentBus : nes::NesBus {
        [[nodiscard]] u8 read(u16) override { return 0x00; }
    };

    SilentBus bus;
    nes::Apu apu;
    apu.set_memory_reader(&bus);

    apu.write(0x4010, 0x8F);   // IRQ enable, fastest rate
    apu.write(0x4012, 0x00);
    apu.write(0x4013, 0x00);   // one byte
    apu.write(0x4015, 0x10);

    EXPECT_FALSE(apu.dmc().irq_pending());
    apu.tick_cpu(2000);
    EXPECT_TRUE(apu.dmc().irq_pending());
    EXPECT_EQ(apu.read(0x4015) & 0x80, 0x80) << "and it shows in the status register";

    apu.write(0x4015, 0x10);   // writing the enable register clears it
    EXPECT_FALSE(apu.dmc().irq_pending());
}

// ===========================================================================
// The frame sequencer
// ===========================================================================

TEST(ApuFrames, FourStepModeWalksOneToFour)
{
    nes::Apu apu;
    apu.write(0x4017, 0x40);   // four step, IRQ inhibited

    std::vector<int> steps;
    for (int i = 0; i < 4; ++i) {
        // One frame step is 7457 CPU cycles, and the APU tick is half a CPU
        // cycle, so 7458 CPU cycles is the smallest whole number of ticks
        // that reaches the 3728.5 tick boundary.
        apu.tick_cpu(7458);
        steps.push_back(apu.frame_step());
    }

    EXPECT_EQ(steps, (std::vector<int>{ 1, 2, 3, 4 }));
}

TEST(ApuFrames, FiveStepModeWalksOneToFive)
{
    nes::Apu apu;
    apu.write(0x4017, 0xC0);   // five step, IRQ inhibited

    std::vector<int> steps;
    for (int i = 0; i < 5; ++i) {
        apu.tick_cpu(7458);
        steps.push_back(apu.frame_step());
    }

    EXPECT_EQ(steps, (std::vector<int>{ 1, 2, 3, 4, 5 }));
    EXPECT_TRUE(apu.five_step_mode());
}

TEST(ApuFrames, FourStepModeRaisesTheFrameIrq)
{
    nes::Apu apu;
    apu.write(0x4017, 0x00);   // four step, IRQ allowed

    apu.tick_cpu(7458 * 4);
    EXPECT_TRUE(apu.frame_irq_pending());
    EXPECT_EQ(apu.read(0x4015) & 0x40, 0x40);
}

TEST(ApuFrames, FiveStepModeNeverRaisesIt)
{
    nes::Apu apu;
    apu.write(0x4017, 0x80);   // five step, IRQ allowed

    apu.tick_cpu(7458 * 5);
    EXPECT_FALSE(apu.frame_irq_pending()) << "the fifth step exists to avoid the IRQ";
}

TEST(ApuFrames, InhibitingTheIrqClearsAPendingOne)
{
    nes::Apu apu;
    apu.write(0x4017, 0x00);
    apu.tick_cpu(7458 * 4);
    ASSERT_TRUE(apu.frame_irq_pending());

    apu.write(0x4017, 0x40);
    EXPECT_FALSE(apu.frame_irq_pending());
}

TEST(ApuFrames, WritingWithTheInhibitBitClocksImmediately)
{
    // Programs use this to line their music up with a known point: the write
    // both resets the divider and gives the envelopes a kick.
    nes::Apu apu;
    apu.write(0x4000, 0x08);   // envelope period 8, no constant volume
    apu.write(0x4002, 0x08);
    apu.write(0x4003, 0x00);
    apu.write(0x4015, 0x01);

    apu.write(0x4017, 0x40);
    EXPECT_EQ(apu.frame_step(), 0) << "the divider was reset";
}

// ===========================================================================
// Clock rates
//
// The APU tick is half a CPU cycle, but the datasheet tables are written in
// CPU cycles and the signal formulas divide by (t + 1) for the register based
// channels. Treating both the same is an easy mistake and its symptom is a
// pitch that is wrong by exactly an octave - which sounds plausible enough
// that it can survive a long time. These tests pin the rates down.
// ===========================================================================

TEST(ApuRates, TheNoiseShiftRegisterRunsAtTheDatasheetRate)
{
    // Period index 3 is 32 CPU cycles, so the LFSR clocks 1789773/32 = 55930
    // times a second. At half that (the old bug) the hi-hat is a dull hiss
    // instead of a crisp hit.
    nes::Apu apu;
    apu.write(0x4015, 0x08);
    apu.write(0x400C, 0x1F);   // constant volume, so the register keeps moving
    apu.write(0x400E, 0x03);   // period index 3
    apu.write(0x400F, 0x08);   // length load

    u16 previous = apu.noise().shift_register();
    int changes = 0;
    for (int i = 0; i < 894886; ++i) {   // one second of APU ticks
        apu.tick();
        const u16 now = apu.noise().shift_register();
        if (now != previous) {
            ++changes;
            previous = now;
        }
    }

    EXPECT_NEAR(changes, 55930, 20) << "the noise is an octave off";
}

TEST(ApuRates, TheTriangleTimerRunsAtTheCpuRate)
{
    // f = CPU / (32 * (t + 1)). With t = 0x1FF that is 1789773 / 16384 =
    // 109.2 Hz, so the 32 step sequence advances 3494 times a second. This is
    // the bass line: an octave of error is a wrong note, not a timbre change.
    nes::Apu apu;
    apu.write(0x4015, 0x04);
    apu.write(0x4008, 0xFF);
    apu.write(0x400A, 0xFF);
    apu.write(0x400B, 0x01);   // timer = 0x1FF

    u8 previous = apu.triangle().sequence_position();
    int steps = 0;
    for (int i = 0; i < 894886; ++i) {
        apu.tick();
        const u8 now = apu.triangle().sequence_position();
        if (now != previous) {
            ++steps;
            previous = now;
        }
    }

    EXPECT_NEAR(steps, 3494, 20) << "the triangle is an octave off";
}

TEST(ApuRates, TheDmcBitRateMatchesTheDatasheet)
{
    // Rate index 15 is 54 CPU cycles per output bit, which is 27 APU ticks.
    // One byte is eight bits, so the read head advances once every 8 * 27 =
    // 216 ticks. The first fetch happens on the very first tick; measure the
    // gap to the second.
    nes::Apu apu;
    apu.write(0x4010, 0x0F);   // rate index 15, no loop, no IRQ
    apu.write(0x4012, 0x00);   // sample at $C000
    apu.write(0x4013, 0xFF);   // long enough that we stop first
    apu.write(0x4015, 0x10);

    int ticks = 0;
    u16 previous = apu.dmc().current_address();
    int first_change = -1;
    int second_change = -1;
    for (int i = 0; i < 2000; ++i) {
        apu.tick();
        ++ticks;
        const u16 now = apu.dmc().current_address();
        if (now != previous) {
            if (first_change < 0) {
                first_change = ticks;
            } else {
                second_change = ticks;
                break;
            }
            previous = now;
        }
    }

    ASSERT_GE(first_change, 0);
    ASSERT_GE(second_change, 0);
    EXPECT_NEAR(second_change - first_change, 216, 5)
        << "eight bits at 27 ticks each is 216 ticks a byte";
}

// ===========================================================================
// Mixing and output
// ===========================================================================

TEST(ApuMix, SilenceIsZero)
{
    nes::Apu apu;
    apu.write(0x4015, 0x00);

    apu.tick_cpu(10000);
    EXPECT_FLOAT_EQ(apu.output(), 0.0f);

    const auto samples = apu.take_samples();
    ASSERT_GT(samples.size(), 0u);
    for (f32 sample : samples) {
        EXPECT_FLOAT_EQ(sample, 0.0f);
    }
}

TEST(ApuMix, APlayingChannelProducesSound)
{
    nes::Apu apu;
    play_pulse1(apu);

    apu.tick_cpu(10000);
    EXPECT_GT(apu.output(), 0.0f);
}

TEST(ApuMix, MoreChannelsIsLouder)
{
    // Turn on exactly `channel_count` of the four square wave channels, all
    // with the same note, and see how loud the mixer gets.
    const auto peak_with = [](int channel_count) {
        nes::Apu apu;
        apu.write(0x4015, 0x0F);

        for (int channel = 0; channel < channel_count; ++channel) {
            const u16 base = static_cast<u16>(0x4000 + channel * 4);
            apu.write(base + 0, 0xBF);
            apu.write(base + 2, 0x08);
            apu.write(base + 3, 0x00);
        }

        f32 peak = 0.0f;
        for (int i = 0; i < 4000; ++i) {
            apu.tick();
            peak = std::max(peak, apu.output());
        }
        return peak;
    };

    // The mixer is not linear, so this is not exactly a factor of two, but
    // more channels must be louder than fewer.
    const f32 one = peak_with(1);
    const f32 two = peak_with(2);
    const f32 four = peak_with(4);

    EXPECT_GT(two, one) << "two channels are louder than one";
    EXPECT_GT(four, two) << "and four are louder than two";
    EXPECT_GT(one, 0.0f);
}

TEST(ApuMix, OutputStaysInRange)
{
    nes::Apu apu;
    apu.write(0x4015, 0x0F);
    for (int channel = 0; channel < 4; ++channel) {
        const u16 base = static_cast<u16>(0x4000 + channel * 4);
        apu.write(base + 0, 0xBF);
        apu.write(base + 2, 0x08);
        apu.write(base + 3, 0x00);
    }

    for (int i = 0; i < 20000; ++i) {
        apu.tick();
        const f32 value = apu.output();
        EXPECT_GE(value, 0.0f);
        EXPECT_LE(value, 1.0f);
    }
}

TEST(ApuMix, SamplesComeOutAtFortyFourOneHundredHertz)
{
    nes::Apu apu;

    // One second of CPU cycles should give about one second of samples.
    apu.tick_cpu(1789773);
    const auto samples = apu.take_samples();

    EXPECT_NEAR(static_cast<double>(samples.size()), 44100.0, 5.0);
}

TEST(ApuMix, TheResamplerAveragesInsteadOfPointSampling)
{
    nes::Apu apu;

    // A note above the 44100 Hz Nyquist rate. Sampling it point by point
    // would alias the harmonics straight back down into the audible band as
    // hiss; averaging the ~20 APU cycles behind each output sample filters
    // them out. With constant volume the pulse is only ever 0 or 15, so
    // point sampling can produce just a handful of distinct values while the
    // average produces the in-between ones.
    apu.write(0x4015, 0x01);
    apu.write(0x4000, 0xBF);   // duty 2, constant volume 15
    apu.write(0x4002, 0x10);   // timer low
    apu.write(0x4003, 0x08);   // timer high + a length load that keeps it on

    apu.tick_cpu(1789773 / 4);
    const auto samples = apu.take_samples();
    ASSERT_GT(samples.size(), 1000u);

    std::vector<f32> distinct;
    for (f32 sample : samples) {
        const bool seen = std::any_of(distinct.begin(), distinct.end(), [sample](f32 value) {
            return std::fabs(value - sample) < 1e-7f;
        });
        if (!seen) {
            distinct.push_back(sample);
        }
    }

    EXPECT_GT(distinct.size(), 4u)
        << "only " << distinct.size() << " distinct levels: the samples are raw, not averaged";
}

TEST(ApuMix, TakingSamplesDrainsTheBuffer)
{
    nes::Apu apu;
    apu.tick_cpu(100000);
    EXPECT_GT(apu.samples_pending(), 0u);

    const auto first = apu.take_samples();
    EXPECT_EQ(apu.samples_pending(), 0u);
    EXPECT_GT(first.size(), 0u);

    apu.tick_cpu(1000);
    const auto second = apu.take_samples();
    EXPECT_LT(second.size(), first.size());
}

TEST(ApuMix, TheApuRunsAtHalfTheCpuClock)
{
    nes::Apu apu;
    apu.tick_cpu(100);
    EXPECT_EQ(apu.apu_cycles(), 50u);

    apu.tick_cpu(1);   // an odd cycle is carried, not dropped
    EXPECT_EQ(apu.apu_cycles(), 50u);
    apu.tick_cpu(1);
    EXPECT_EQ(apu.apu_cycles(), 51u);
}
