#pragma once

// ---------------------------------------------------------------------------
// The APU - Audio Processing Unit.
//
// The APU is not a separate chip. It is on the same die as the CPU, in the
// Ricoh 2A03, which is why it shares the CPU's clock and why $4000-$4017
// sits in the CPU's address space rather than on its own bus.
//
// Five channels, each a small piece of dedicated hardware:
//
//     Pulse 1     a square wave with a selectable duty cycle
//     Pulse 2     the same, and the two can be detuned against each other
//     Triangle    a triangle wave, and no volume control at all
//     Noise       a pseudo-random bit stream from a shift register
//     DMC         delta modulated samples read out of CPU memory
//
// What makes NES music sound like NES music is that these channels have
// almost no features. There is no filter, no mixer, no effects, no envelope
// shapes beyond a linear decay, and no way to play a sample except by
// streaming it through the DMC. Composers got the sound by writing their own
// software instruments on top of these five very dumb voices.
//
// Timing
// ------
// The APU runs at half the CPU clock: 1.789773 MHz / 2 = 894886 Hz.
//
// Everything inside is driven by two counters:
//
//     the channel's own timer      sets the pitch, counts APU cycles
//     the frame sequencer          drives envelopes and lengths
//
// The frame sequencer has two modes and walks either four or five steps per
// frame, about 240 Hz:
//
//     4-step:  quarter frames (envelopes) on steps 1,2,3,4
//              half frames    (length, sweep) on steps 2,4
//              the last step raises the frame IRQ
//
//     5-step:  quarter frames on 1,2,3,4,5
//              half frames    on 2,5
//              no IRQ
//
// The 5-step mode exists so that the half-frame clock runs at a slightly
// different rate, which is a pitch effect some soundtracks use.
// ---------------------------------------------------------------------------

#include "core/state_fwd.hpp"
#include "core/bus.hpp"
#include "core/nes/device.hpp"
#include "core/types.hpp"

#include <array>
#include <cstddef>
#include <vector>

namespace fc::nes {

// ---------------------------------------------------------------------------
// The channel's 32 entry length counter table.
//
// A length counter is just a down counter that silences the channel when it
// reaches zero, and the value it loads comes from this table rather than from
// the register directly. The table is not linear: it is the set of note
// lengths the hardware designer thought were useful, and it is the same for
// every channel.
// ---------------------------------------------------------------------------
inline constexpr u8 kLengthTable[32] = {
    10, 254, 20,  2, 40,  4, 80,  6,
    160,  8, 60, 10, 14, 12, 26, 14,
    12,  16, 24, 18, 48, 20, 96, 22,
    192, 24, 72, 26, 16, 28, 32, 30,
};

/// The eight steps of a pulse channel's duty cycle, for each of the four
/// settings. The shift register is reloaded from one of these every eight
/// timer expiries, which is what makes a square wave a square wave.
inline constexpr u8 kDutyTable[4][8] = {
    { 0, 1, 0, 0, 0, 0, 0, 0 },   // 12.5%
    { 0, 1, 1, 0, 0, 0, 0, 0 },   // 25%
    { 0, 1, 1, 1, 1, 0, 0, 0 },   // 50%
    { 1, 0, 0, 1, 1, 1, 1, 0 },   // 25%, negated
};

/// The triangle channel's waveform: 32 steps up and down.
inline constexpr u8 kTriangleSequence[32] = {
    15, 14, 13, 12, 11, 10,  9,  8,
     7,  6,  5,  4,  3,  2,  1,  0,
     0,  1,  2,  3,  4,  5,  6,  7,
     8,  9, 10, 11, 12, 13, 14, 15,
};

/// Noise timer periods, NTSC. Short periods are high pitched hisses, long
/// ones are the low rumble under an explosion.
inline constexpr u16 kNoisePeriodTable[16] = {
    4, 8, 16, 32, 64, 96, 128, 160,
    202, 254, 380, 508, 762, 1016, 2034, 4068,
};

/// DMC sample rates, NTSC, in APU cycles per output bit.
inline constexpr u16 kDmcRateTable[16] = {
    428, 380, 340, 320, 286, 254, 226, 214,
    190, 160, 142, 128, 106, 84, 72, 54,
};

// ---------------------------------------------------------------------------
// Pulse 1 and Pulse 2
// ---------------------------------------------------------------------------

class PulseChannel {
public:
    explicit PulseChannel(bool is_pulse1 = true) noexcept
        : is_pulse1_(is_pulse1)
    {
    }

    void reset() noexcept;
    void write(u8 index, u8 value) noexcept;   // index 0-3 within the channel

    void clock_timer() noexcept;      // every APU cycle
    void clock_envelope() noexcept;   // quarter frame
    void clock_length() noexcept;     // half frame
    void clock_sweep() noexcept;      // half frame

    void set_enabled(bool on) noexcept;
    [[nodiscard]] u8 output() const noexcept;          // 0-15
    [[nodiscard]] bool length_active() const noexcept { return length_ > 0; }
    [[nodiscard]] u16 timer() const noexcept { return timer_; }
    [[nodiscard]] u8 duty_position() const noexcept { return duty_pos_; }

private:
    [[nodiscard]] int sweep_target() const noexcept;
    [[nodiscard]] bool silenced() const noexcept;

    bool is_pulse1_ = true;

    u8 duty_ = 0;
    bool length_halt_ = false;      // also the envelope loop flag
    bool constant_volume_ = false;
    u8 volume_ = 0;
    u8 envelope_divider_ = 0;
    u8 envelope_decay_ = 0;
    bool envelope_start_ = false;

    bool sweep_enable_ = false;
    u8 sweep_period_ = 0;
    bool sweep_negate_ = false;
    u8 sweep_shift_ = 0;
    bool sweep_reload_ = false;
    u8 sweep_counter_ = 0;

    u16 timer_ = 0;
    u16 timer_counter_ = 0;
    u8 duty_pos_ = 0;
    u8 length_ = 0;
    bool enabled_ = false;

    friend struct fc::StateAccess;
};

// ---------------------------------------------------------------------------
// Triangle
// ---------------------------------------------------------------------------

class TriangleChannel {
public:
    void reset() noexcept;
    void write(u8 index, u8 value) noexcept;

    void clock_timer() noexcept;
    void clock_linear() noexcept;   // quarter frame
    void clock_length() noexcept;   // half frame

    void set_enabled(bool on) noexcept;
    [[nodiscard]] u8 output() const noexcept;          // 0-15
    [[nodiscard]] bool length_active() const noexcept { return length_ > 0; }
    [[nodiscard]] u8 sequence_position() const noexcept { return sequence_pos_; }

private:
    bool control_ = false;          // length halt and linear counter control
    u8 linear_reload_ = 0;
    bool linear_reload_flag_ = false;
    u8 linear_counter_ = 0;

    u16 timer_ = 0;
    u16 timer_counter_ = 0;
    u8 sequence_pos_ = 0;
    u8 length_ = 0;
    bool enabled_ = false;

    friend struct fc::StateAccess;
};

// ---------------------------------------------------------------------------
// Noise
// ---------------------------------------------------------------------------

class NoiseChannel {
public:
    void reset() noexcept;
    void write(u8 index, u8 value) noexcept;

    void clock_timer() noexcept;
    void clock_envelope() noexcept;
    void clock_length() noexcept;

    void set_enabled(bool on) noexcept;
    [[nodiscard]] u8 output() const noexcept;
    [[nodiscard]] bool length_active() const noexcept { return length_ > 0; }
    [[nodiscard]] u16 shift_register() const noexcept { return lfsr_; }

    /// Inspection, for tests and diagnostics.
    [[nodiscard]] u8 period_index() const noexcept { return period_index_; }
    [[nodiscard]] bool short_mode() const noexcept { return mode_; }
    [[nodiscard]] bool constant_volume() const noexcept { return constant_volume_; }
    [[nodiscard]] u8 volume() const noexcept { return volume_; }
    [[nodiscard]] u8 envelope_level() const noexcept { return envelope_decay_; }
    [[nodiscard]] bool length_halted() const noexcept { return length_halt_; }

private:
    bool length_halt_ = false;
    bool constant_volume_ = false;
    u8 volume_ = 0;
    u8 envelope_divider_ = 0;
    u8 envelope_decay_ = 0;
    bool envelope_start_ = false;

    bool mode_ = false;             // false: 15 bit, true: 6 bit (metallic)
    u8 period_index_ = 0;
    u16 timer_counter_ = 0;

    u16 lfsr_ = 1;
    u8 length_ = 0;
    bool enabled_ = false;

    friend struct fc::StateAccess;
};

// ---------------------------------------------------------------------------
// DMC - Delta Modulation Channel
// ---------------------------------------------------------------------------

class DmcChannel {
public:
    void reset() noexcept;
    void write(u8 index, u8 value) noexcept;

    void clock_timer() noexcept;

    void set_enabled(bool on) noexcept;
    void set_memory_reader(Bus* bus) noexcept { bus_ = bus; }

    [[nodiscard]] u8 output() const noexcept { return level_; }   // 0-127
    [[nodiscard]] u8 rate_index() const noexcept { return rate_index_; }
    [[nodiscard]] bool looping() const noexcept { return loop_; }
    [[nodiscard]] bool irq_pending() const noexcept { return irq_pending_; }
    void clear_irq() noexcept { irq_pending_ = false; }
    [[nodiscard]] u16 sample_address() const noexcept { return address_; }
    [[nodiscard]] u16 current_address() const noexcept { return current_address_; }
    [[nodiscard]] u16 bytes_remaining() const noexcept { return bytes_remaining_; }

private:
    void restart() noexcept;

    bool irq_enable_ = false;
    bool loop_ = false;
    u8 rate_index_ = 0;

    u8 level_ = 0;                  // the 7 bit DAC
    u16 address_ = 0xC000;          // where the sample starts, as configured
    u16 length_ = 0;                // how many bytes, as configured
    u16 current_address_ = 0xC000;  // where the read head is now
    u16 bytes_remaining_ = 0;

    u16 timer_counter_ = 0;
    u8 sample_buffer_ = 0;
    bool sample_buffer_empty_ = true;
    u8 bits_remaining_ = 8;
    bool enabled_ = false;
    bool irq_pending_ = false;

    /// The bus is how DMC fetches samples. Not state: it is the same object
    /// every time the machine is built.
    Bus* bus_ = nullptr;

    friend struct fc::StateAccess;
};

// ---------------------------------------------------------------------------
// The APU
// ---------------------------------------------------------------------------

class Apu : public Device {
public:
    /// The CPU's clock, in Hz. The APU runs at half of this.
    static constexpr f64 kCpuClock = 1789772.7272727273;
    static constexpr f64 kApuClock = kCpuClock / 2.0;

    /// What we resample down to.
    static constexpr int kSampleRate = 44100;

    /// How many samples we will hold before dropping them. The caller is
    /// expected to drain with take_samples(); this is only a guard against an
    /// unbounded buffer if nobody does.
    static constexpr std::size_t kMaxPendingSamples = 1u << 20;

    Apu();

    void reset() noexcept;

    /// The DMC reads sample data straight out of CPU memory, so the bus has
    /// to be handed to it. The machine wires this up after construction; the
    /// APU never owns the bus, it only borrows it.
    void set_memory_reader(Bus* bus) noexcept { dmc_.set_memory_reader(bus); }

    // -- Device: $4000-$4017 ------------------------------------------------

    [[nodiscard]] u8 read(u16 address) override;
    void write(u16 address, u8 value) override;

    // -- timing --------------------------------------------------------------

    /// Advance by a number of CPU cycles. The APU gets half that many.
    void tick_cpu(int cpu_cycles) noexcept;

    /// Advance by one APU cycle. Mainly for tests.
    void tick() noexcept;

    // -- output --------------------------------------------------------------

    /// Everything generated since the last call, at 44100 Hz, in 0..1.
    /// The NES's output is unipolar and AC coupled by a capacitor on the
    /// board, so a constant offset here is not a mistake.
    [[nodiscard]] std::vector<f32> take_samples();

    /// Copy out at most `max_samples` and drop them, without allocating.
    /// This is the one fc_take_samples() calls, because an audio callback
    /// must never allocate or block.
    std::size_t drain(f32* out, std::size_t max_samples) noexcept;

    /// Throw away everything queued, for when playback is paused.
    void clear_samples() noexcept { samples_.clear(); }

    [[nodiscard]] std::size_t samples_pending() const noexcept { return samples_.size(); }

    /// The mixer's current value without going through the sample buffer.
    [[nodiscard]] f32 output() const noexcept;

    // -- inspection ----------------------------------------------------------

    /// $4015's value: bit 0 pulse 1, 1 pulse 2, 2 triangle, 3 noise, 4 DMC.
    [[nodiscard]] u8 channel_status() const noexcept;
    [[nodiscard]] u8 enabled_channels() const noexcept { return enabled_; }
    [[nodiscard]] bool frame_irq_pending() const noexcept { return frame_irq_; }
    void clear_frame_irq() noexcept { frame_irq_ = false; }
    [[nodiscard]] int frame_step() const noexcept { return frame_step_; }
    [[nodiscard]] bool five_step_mode() const noexcept { return five_step_; }
    [[nodiscard]] u64 apu_cycles() const noexcept { return cycles_; }

    [[nodiscard]] PulseChannel& pulse1() noexcept { return pulse1_; }
    [[nodiscard]] PulseChannel& pulse2() noexcept { return pulse2_; }
    [[nodiscard]] TriangleChannel& triangle() noexcept { return triangle_; }
    [[nodiscard]] NoiseChannel& noise() noexcept { return noise_; }
    [[nodiscard]] DmcChannel& dmc() noexcept { return dmc_; }

private:
    void clock_quarter_frame() noexcept;
    void clock_half_frame() noexcept;
    [[nodiscard]] f32 mix() const noexcept;
    void emit_sample() noexcept;

    PulseChannel pulse1_{ true };
    PulseChannel pulse2_{ false };
    TriangleChannel triangle_{};
    NoiseChannel noise_{};
    DmcChannel dmc_{};

    u8 enabled_ = 0;

    int frame_step_ = 0;
    int frame_counter_ = 0;
    bool five_step_ = false;
    bool irq_inhibit_ = false;
    bool frame_irq_ = false;

    int cpu_remainder_ = 0;
    f64 sample_accumulator_ = 0.0;

    /// The running sum and count behind one output sample. Between two
    /// 44100 Hz samples about twenty APU cycles pass, and averaging them is
    /// the anti-aliasing filter (see emit_sample).
    f64 mix_accumulator_ = 0.0;
    int mix_counter_ = 0;

    std::vector<f32> samples_;
    f32 last_output_ = 0.0f;

    u64 cycles_ = 0;

    friend struct fc::StateAccess;
};

} // namespace fc::nes
