#include "core/nes/apu.hpp"

#include <algorithm>

namespace fc::nes {

namespace {

/// The frame sequencer steps every 7457 CPU cycles on an NTSC machine. That
/// is the number to remember: it is what makes the quarter frame clock run at
/// 240 Hz, the half frame clock at 120 Hz and the frame IRQ at 60 Hz.
///
/// Our APU is clocked at half the CPU rate (see tick_cpu), so one CPU cycle
/// is two ticks and the step is 3728.5 ticks. Rounding up drifts by half a
/// tick per step, which is a couple of CPU cycles per frame and inaudible.
///
/// This number being wrong by a factor of two is not subtle in the other
/// direction. The quarter frame clock drives the volume envelopes and the
/// length counters, so at half speed a snare decays for twice as long and
/// the drums smear into a constant hiss instead of crisp hits.
constexpr int kFrameStepCycles = 3729;

u8 decay_envelope_clock(u8& divider, u8& decay, bool& start, u8 period,
                        bool loop) noexcept
{
    if (start) {
        start = false;
        decay = 15;
        divider = period;
        return decay;
    }

    if (divider == 0) {
        divider = period;
        if (decay > 0) {
            --decay;
        } else if (loop) {
            decay = 15;
        }
    } else {
        --divider;
    }
    return decay;
}

} // namespace

// ===========================================================================
// Pulse
// ===========================================================================

void PulseChannel::reset() noexcept
{
    duty_ = 0;
    length_halt_ = false;
    constant_volume_ = false;
    volume_ = 0;
    envelope_divider_ = 0;
    envelope_decay_ = 0;
    envelope_start_ = false;

    sweep_enable_ = false;
    sweep_period_ = 0;
    sweep_negate_ = false;
    sweep_shift_ = 0;
    sweep_reload_ = false;
    sweep_counter_ = 0;

    timer_ = 0;
    timer_counter_ = 0;
    duty_pos_ = 0;
    length_ = 0;
    enabled_ = false;
}

void PulseChannel::write(u8 index, u8 value) noexcept
{
    switch (index & 0x03u) {
    case 0:
        duty_ = static_cast<u8>((value >> 6) & 0x03u);
        length_halt_ = (value & 0x20u) != 0;
        constant_volume_ = (value & 0x10u) != 0;
        volume_ = static_cast<u8>(value & 0x0Fu);
        break;

    case 1:
        sweep_enable_ = (value & 0x80u) != 0;
        sweep_period_ = static_cast<u8>((value >> 4) & 0x07u);
        sweep_negate_ = (value & 0x08u) != 0;
        sweep_shift_ = static_cast<u8>(value & 0x07u);
        sweep_reload_ = true;
        break;

    case 2:
        timer_ = static_cast<u16>((timer_ & 0x0700u) | value);
        break;

    case 3:
        timer_ = static_cast<u16>((timer_ & 0x00FFu) | ((value & 0x07u) << 8));
        if (enabled_) {
            length_ = kLengthTable[value >> 3];
        }
        envelope_start_ = true;
        duty_pos_ = 0;
        break;

    default:
        break;
    }
}

int PulseChannel::sweep_target() const noexcept
{
    const int change = static_cast<int>(timer_) >> sweep_shift_;

    if (sweep_negate_) {
        // Pulse 1 negates by inverting the period and subtracting, which
        // ends up one lower than a plain subtraction. Pulse 2 does not.
        // This asymmetry is in the silicon and some soundtracks depend on it.
        return static_cast<int>(timer_) - change - (is_pulse1_ ? 1 : 0);
    }
    return static_cast<int>(timer_) + change;
}

bool PulseChannel::silenced() const noexcept
{
    if (length_ == 0) {
        return true;
    }
    // Below 8 the channel is inaudible, and the sweep unit is disabled too.
    if (timer_ < 8) {
        return true;
    }
    if (sweep_target() > 0x7FF) {
        return true;
    }
    return false;
}

void PulseChannel::clock_timer() noexcept
{
    if (timer_counter_ == 0) {
        timer_counter_ = timer_;
        duty_pos_ = static_cast<u8>((duty_pos_ + 1) & 7u);
    } else {
        --timer_counter_;
    }
}

void PulseChannel::clock_envelope() noexcept
{
    (void)decay_envelope_clock(envelope_divider_, envelope_decay_,
                               envelope_start_, volume_, length_halt_);
}

void PulseChannel::clock_length() noexcept
{
    if (length_halt_) {
        return;
    }
    if (length_ > 0) {
        --length_;
    }
}

void PulseChannel::clock_sweep() noexcept
{
    if (sweep_counter_ == 0 && sweep_enable_ && sweep_shift_ > 0) {
        const int target = sweep_target();
        if (target <= 0x7FF) {
            timer_ = static_cast<u16>(target);
        }
    }

    if (sweep_counter_ == 0 || sweep_reload_) {
        sweep_counter_ = sweep_period_;
        sweep_reload_ = false;
    } else {
        --sweep_counter_;
    }
}

void PulseChannel::set_enabled(bool on) noexcept
{
    enabled_ = on;
    if (!on) {
        length_ = 0;
    }
}

u8 PulseChannel::output() const noexcept
{
    if (silenced()) {
        return 0;
    }
    if (kDutyTable[duty_][duty_pos_] == 0) {
        return 0;
    }
    return constant_volume_ ? volume_ : envelope_decay_;
}

// ===========================================================================
// Triangle
// ===========================================================================

void TriangleChannel::reset() noexcept
{
    control_ = false;
    linear_reload_ = 0;
    linear_reload_flag_ = false;
    linear_counter_ = 0;
    timer_ = 0;
    timer_counter_ = 0;
    sequence_pos_ = 0;
    length_ = 0;
    enabled_ = false;
}

void TriangleChannel::write(u8 index, u8 value) noexcept
{
    switch (index & 0x03u) {
    case 0:
        control_ = (value & 0x80u) != 0;
        linear_reload_ = static_cast<u8>(value & 0x7Fu);
        break;

    case 2:
        timer_ = static_cast<u16>((timer_ & 0x0700u) | value);
        break;

    case 3:
        timer_ = static_cast<u16>((timer_ & 0x00FFu) | ((value & 0x07u) << 8));
        if (enabled_) {
            length_ = kLengthTable[value >> 3];
        }
        linear_reload_flag_ = true;
        break;

    default:
        break;   // $4009 is not connected to anything
    }
}

void TriangleChannel::clock_timer() noexcept
{
    if (timer_counter_ != 0) {
        --timer_counter_;
        return;
    }

    timer_counter_ = timer_;

    // The sequence only advances while the channel is actually sounding, so
    // a note that stops and restarts carries on from where it left off
    // instead of clicking back to the start of the wave.
    if (length_ > 0 && linear_counter_ > 0 && timer_ >= 2) {
        sequence_pos_ = static_cast<u8>((sequence_pos_ + 1) & 31u);
    }
}

void TriangleChannel::clock_linear() noexcept
{
    if (linear_reload_flag_) {
        linear_counter_ = linear_reload_;
    } else if (linear_counter_ > 0) {
        --linear_counter_;
    }

    // The control flag doubles as "hold the linear counter at its reload
    // value", which is how a triangle note is sustained.
    if (!control_) {
        linear_reload_flag_ = false;
    }
}

void TriangleChannel::clock_length() noexcept
{
    if (control_) {
        return;
    }
    if (length_ > 0) {
        --length_;
    }
}

void TriangleChannel::set_enabled(bool on) noexcept
{
    enabled_ = on;
    if (!on) {
        length_ = 0;
    }
}

u8 TriangleChannel::output() const noexcept
{
    if (length_ == 0 || linear_counter_ == 0) {
        return 0;
    }
    if (timer_ < 2) {
        return 0;   // too fast to be audible; the hardware silences it
    }
    return kTriangleSequence[sequence_pos_];
}

// ===========================================================================
// Noise
// ===========================================================================

void NoiseChannel::reset() noexcept
{
    length_halt_ = false;
    constant_volume_ = false;
    volume_ = 0;
    envelope_divider_ = 0;
    envelope_decay_ = 0;
    envelope_start_ = false;

    mode_ = false;
    period_index_ = 0;
    timer_counter_ = 0;
    lfsr_ = 1;
    length_ = 0;
    enabled_ = false;
}

void NoiseChannel::write(u8 index, u8 value) noexcept
{
    switch (index & 0x03u) {
    case 0:
        length_halt_ = (value & 0x20u) != 0;
        constant_volume_ = (value & 0x10u) != 0;
        volume_ = static_cast<u8>(value & 0x0Fu);
        break;

    case 2:
        mode_ = (value & 0x80u) != 0;
        period_index_ = static_cast<u8>(value & 0x0Fu);
        break;

    case 3:
        if (enabled_) {
            length_ = kLengthTable[value >> 3];
        }
        envelope_start_ = true;
        break;

    default:
        break;
    }
}

void NoiseChannel::clock_timer() noexcept
{
    if (timer_counter_ != 0) {
        --timer_counter_;
        return;
    }

    // kNoisePeriodTable is the LFSR clock period in CPU cycles, exactly (the
    // formula is CPU / period with no + 1). Our APU tick is half a CPU cycle,
    // and the reload below costs one extra tick, so the reload is period/2 - 1
    // ticks. Using the table unchanged makes the shift register run an octave
    // slow: the difference between a snare's "tss" and a continuous "shhh".
    timer_counter_ = static_cast<u16>(kNoisePeriodTable[period_index_] / 2u - 1u);

    // A linear feedback shift register. Bit 0 is fed back into bit 14,
    // XORed with either bit 1 (15 bit, the hissing noise) or bit 6 (6 bit,
    // the metallic ringing one). Nothing here is random: it is a completely
    // deterministic sequence that just sounds like it.
    const u16 tap = mode_ ? 6 : 1;
    const u16 feedback = static_cast<u16>((lfsr_ & 1u) ^ ((lfsr_ >> tap) & 1u));
    lfsr_ = static_cast<u16>(lfsr_ >> 1);
    lfsr_ = static_cast<u16>(lfsr_ | (feedback << 14));
}

void NoiseChannel::clock_envelope() noexcept
{
    (void)decay_envelope_clock(envelope_divider_, envelope_decay_,
                               envelope_start_, volume_, length_halt_);
}

void NoiseChannel::clock_length() noexcept
{
    if (length_halt_) {
        return;
    }
    if (length_ > 0) {
        --length_;
    }
}

void NoiseChannel::set_enabled(bool on) noexcept
{
    enabled_ = on;
    if (!on) {
        length_ = 0;
    }
}

u8 NoiseChannel::output() const noexcept
{
    if (length_ == 0) {
        return 0;
    }
    if ((lfsr_ & 1u) != 0) {
        return 0;
    }
    return constant_volume_ ? volume_ : envelope_decay_;
}

// ===========================================================================
// DMC
// ===========================================================================

void DmcChannel::reset() noexcept
{
    irq_enable_ = false;
    loop_ = false;
    rate_index_ = 0;
    level_ = 0;
    address_ = 0xC000;
    length_ = 0;
    current_address_ = 0xC000;
    bytes_remaining_ = 0;
    timer_counter_ = 0;
    sample_buffer_ = 0;
    sample_buffer_empty_ = true;
    bits_remaining_ = 8;
    enabled_ = false;
    irq_pending_ = false;
}

void DmcChannel::write(u8 index, u8 value) noexcept
{
    switch (index & 0x03u) {
    case 0:
        irq_enable_ = (value & 0x80u) != 0;
        if (!irq_enable_) {
            irq_pending_ = false;
        }
        loop_ = (value & 0x40u) != 0;
        rate_index_ = static_cast<u8>(value & 0x0Fu);
        break;

    case 1:
        // The only way to write the DAC directly. This is what a game uses
        // to make a click or a low buzz without any sample data.
        level_ = static_cast<u8>(value & 0x7Fu);
        break;

    case 2:
        address_ = static_cast<u16>(0xC000u + (static_cast<u16>(value) * 64u));
        break;

    case 3:
        length_ = static_cast<u16>((static_cast<u16>(value) * 16u) + 1u);
        break;

    default:
        break;
    }
}

void DmcChannel::restart() noexcept
{
    current_address_ = address_;
    bytes_remaining_ = length_;
}

void DmcChannel::set_enabled(bool on) noexcept
{
    enabled_ = on;
    if (!on) {
        bytes_remaining_ = 0;
        sample_buffer_empty_ = true;
    } else if (bytes_remaining_ == 0) {
        restart();
    }
}

void DmcChannel::clock_timer() noexcept
{
    if (timer_counter_ != 0) {
        --timer_counter_;
        return;
    }

    // Like the noise table, kDmcRateTable is the bit period in CPU cycles, so
    // the reload in half-CPU ticks is rate/2 - 1 (see the noise comment).
    timer_counter_ = static_cast<u16>(kDmcRateTable[rate_index_] / 2u - 1u);

    // Refill the one byte buffer when it runs dry. On real hardware this
    // takes four cycles and the CPU is stalled if it tries to use the bus;
    // here it just happens.
    if (sample_buffer_empty_ && bytes_remaining_ > 0) {
        sample_buffer_ = (bus_ != nullptr) ? bus_->read(current_address_) : 0x00;
        sample_buffer_empty_ = false;
        bits_remaining_ = 8;

        current_address_ = (current_address_ == 0xFFFFu)
                               ? 0x8000u
                               : static_cast<u16>(current_address_ + 1u);

        --bytes_remaining_;
        if (bytes_remaining_ == 0) {
            if (loop_) {
                restart();
            } else if (irq_enable_) {
                irq_pending_ = true;
            }
        }
    }

    if (sample_buffer_empty_) {
        return;
    }

    // Each bit nudges the 7 bit DAC up or down by two. That is the whole
    // encoding: delta modulation stores the *change* in level, not the level.
    if ((sample_buffer_ & 1u) != 0) {
        level_ = (level_ <= 125u) ? static_cast<u8>(level_ + 2u) : 127u;
    } else {
        level_ = (level_ >= 2u) ? static_cast<u8>(level_ - 2u) : 0u;
    }

    sample_buffer_ = static_cast<u8>(sample_buffer_ >> 1);
    if (--bits_remaining_ == 0) {
        sample_buffer_empty_ = true;
    }
}

// ===========================================================================
// The APU
// ===========================================================================

Apu::Apu()
{
    reset();
}

void Apu::reset() noexcept
{
    pulse1_.reset();
    pulse2_.reset();
    triangle_.reset();
    noise_.reset();
    dmc_.reset();

    enabled_ = 0;
    frame_step_ = 0;
    frame_counter_ = 0;
    five_step_ = false;
    irq_inhibit_ = false;
    frame_irq_ = false;

    cpu_remainder_ = 0;
    sample_accumulator_ = 0.0;
    mix_accumulator_ = 0.0;
    mix_counter_ = 0;
    samples_.clear();
    last_output_ = 0.0f;
    cycles_ = 0;
}

// ---------------------------------------------------------------------------
// The CPU's side
// ---------------------------------------------------------------------------

u8 Apu::read(u16 address)
{
    switch (address) {
    case 0x4015:
        return channel_status();
    default:
        // Every other APU register is write only. Reading one gives whatever
        // is on the bus, and the bus handles that.
        return 0;
    }
}

void Apu::write(u16 address, u8 value)
{
    switch (address) {
    case 0x4000: pulse1_.write(0, value); break;
    case 0x4001: pulse1_.write(1, value); break;
    case 0x4002: pulse1_.write(2, value); break;
    case 0x4003: pulse1_.write(3, value); break;

    case 0x4004: pulse2_.write(0, value); break;
    case 0x4005: pulse2_.write(1, value); break;
    case 0x4006: pulse2_.write(2, value); break;
    case 0x4007: pulse2_.write(3, value); break;

    case 0x4008: triangle_.write(0, value); break;
    case 0x4009: triangle_.write(1, value); break;
    case 0x400A: triangle_.write(2, value); break;
    case 0x400B: triangle_.write(3, value); break;

    case 0x400C: noise_.write(0, value); break;
    case 0x400D: noise_.write(1, value); break;
    case 0x400E: noise_.write(2, value); break;
    case 0x400F: noise_.write(3, value); break;

    case 0x4010: dmc_.write(0, value); break;
    case 0x4011: dmc_.write(1, value); break;
    case 0x4012: dmc_.write(2, value); break;
    case 0x4013: dmc_.write(3, value); break;

    case 0x4015:
        enabled_ = static_cast<u8>(value & 0x1Fu);
        pulse1_.set_enabled((value & 0x01u) != 0);
        pulse2_.set_enabled((value & 0x02u) != 0);
        triangle_.set_enabled((value & 0x04u) != 0);
        noise_.set_enabled((value & 0x08u) != 0);
        dmc_.set_enabled((value & 0x10u) != 0);
        dmc_.clear_irq();
        break;

    case 0x4017:
        five_step_ = (value & 0x80u) != 0;
        irq_inhibit_ = (value & 0x40u) != 0;
        if (irq_inhibit_) {
            frame_irq_ = false;
        }

        frame_step_ = 0;
        frame_counter_ = 0;

        // Writing the frame counter with the IRQ inhibited also resets the
        // divider and, in four step mode, clocks the quarter and half frame
        // units straight away. Programs use this to line their music up with
        // a known point in the frame.
        if (irq_inhibit_ && !five_step_) {
            clock_quarter_frame();
            clock_half_frame();
        }
        break;

    default:
        break;
    }
}

// ---------------------------------------------------------------------------
// The frame sequencer
// ---------------------------------------------------------------------------

void Apu::clock_quarter_frame() noexcept
{
    pulse1_.clock_envelope();
    pulse2_.clock_envelope();
    triangle_.clock_linear();
    noise_.clock_envelope();
}

void Apu::clock_half_frame() noexcept
{
    pulse1_.clock_length();
    pulse1_.clock_sweep();
    pulse2_.clock_length();
    pulse2_.clock_sweep();
    triangle_.clock_length();
    noise_.clock_length();
}

void Apu::tick() noexcept
{
    ++cycles_;

    // Every channel has its own timer, and they all count APU cycles. There
    // is no shared clock: the four channels are completely independent
    // oscillators that happen to share an output wire.
    pulse1_.clock_timer();
    pulse2_.clock_timer();

    // The triangle timer runs at the CPU rate, not the APU rate, which is why
    // its frequency formula divides by 32 where the pulse's divides by 16:
    // same timer value, twice the rate, four times the steps. tick() is half
    // a CPU cycle, so the triangle gets two timer clocks per tick.
    triangle_.clock_timer();
    triangle_.clock_timer();

    noise_.clock_timer();
    dmc_.clock_timer();

    if (++frame_counter_ >= kFrameStepCycles) {
        frame_counter_ = 0;

        if (five_step_) {
            if (++frame_step_ > 5) {
                frame_step_ = 1;
            }
            if (frame_step_ != 4) {
                clock_quarter_frame();
            }
            if (frame_step_ == 2 || frame_step_ == 5) {
                clock_half_frame();
            }
        } else {
            if (++frame_step_ > 4) {
                frame_step_ = 1;
            }
            clock_quarter_frame();
            if (frame_step_ == 2 || frame_step_ == 4) {
                clock_half_frame();
            }
            if (frame_step_ == 4 && !irq_inhibit_) {
                frame_irq_ = true;
            }
        }
    }

    // Resample from 894886 Hz down to 44100. The accumulator keeps the
    // fractional part, so the rate is right on average rather than drifting.
    //
    // The average below is the anti-aliasing filter. Picking every 20th
    // sample instead would fold everything above 22 kHz back down, and the
    // noise channel runs far above that: the result is the sizzling "hiss"
    // that point-sampled NES audio is famous for. Averaging the ~20 APU
    // cycles that make up one output sample is a box filter whose first null
    // sits exactly at the output rate, so the worst of the fold-back is gone
    // for twenty additions per sample.
    mix_accumulator_ += static_cast<f64>(mix());
    ++mix_counter_;

    sample_accumulator_ += static_cast<f64>(kSampleRate);
    if (sample_accumulator_ >= kApuClock) {
        sample_accumulator_ -= kApuClock;
        emit_sample();
    }
}

void Apu::tick_cpu(int cpu_cycles) noexcept
{
    cpu_remainder_ += cpu_cycles;
    while (cpu_remainder_ >= 2) {
        cpu_remainder_ -= 2;
        tick();
    }
}

// ---------------------------------------------------------------------------
// Mixing
// ---------------------------------------------------------------------------

f32 Apu::mix() const noexcept
{
    // The 2A03's mixer is not a linear sum. These two formulae are the
    // standard approximation of its measured response, and they matter: a
    // linear mix makes the pulse channels far too loud against the triangle
    // and the noise.
    const int pulse_sum = static_cast<int>(pulse1_.output()) + static_cast<int>(pulse2_.output());

    f32 pulse_out = 0.0f;
    if (pulse_sum > 0) {
        pulse_out = 95.88f / ((8128.0f / static_cast<f32>(pulse_sum)) + 100.0f);
    }

    const f32 tnd = (static_cast<f32>(triangle_.output()) / 8227.0f)
                  + (static_cast<f32>(noise_.output()) / 12241.0f)
                  + (static_cast<f32>(dmc_.output()) / 22638.0f);

    f32 tnd_out = 0.0f;
    if (tnd > 0.0f) {
        tnd_out = 159.79f / ((1.0f / tnd) + 100.0f);
    }

    return pulse_out + tnd_out;
}

void Apu::emit_sample() noexcept
{
    // The average of every APU cycle since the last output sample, which is
    // both the decimation and the anti-aliasing filter.
    last_output_ = (mix_counter_ > 0)
        ? static_cast<f32>(mix_accumulator_ / static_cast<f64>(mix_counter_))
        : mix();
    mix_accumulator_ = 0.0;
    mix_counter_ = 0;

    if (samples_.size() < kMaxPendingSamples) {
        samples_.push_back(last_output_);
    }
}

std::vector<f32> Apu::take_samples()
{
    std::vector<f32> out;
    out.swap(samples_);
    return out;
}

std::size_t Apu::drain(f32* out, std::size_t max_samples) noexcept
{
    if (out == nullptr || max_samples == 0) {
        return 0;
    }

    const std::size_t count = std::min(max_samples, samples_.size());
    std::copy_n(samples_.begin(), count, out);

    // Erasing from the front is O(n). That is fine here: the vector holds at
    // most one frame of audio, and the alternative ring buffer would add
    // index arithmetic to the one function that has to be obviously correct.
    samples_.erase(samples_.begin(),
                   samples_.begin() + static_cast<std::ptrdiff_t>(count));
    return count;
}

u8 Apu::channel_status() const noexcept
{
    u8 status = 0;
    if (pulse1_.length_active())   status |= 0x01;
    if (pulse2_.length_active())   status |= 0x02;
    if (triangle_.length_active()) status |= 0x04;
    if (noise_.length_active())    status |= 0x08;
    if (dmc_.bytes_remaining() > 0) status |= 0x10;
    if (frame_irq_)                status |= 0x40;
    if (dmc_.irq_pending())        status |= 0x80;
    return status;
}

f32 Apu::output() const noexcept
{
    return mix();
}

} // namespace fc::nes
