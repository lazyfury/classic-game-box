#pragma once

// ---------------------------------------------------------------------------
// Save states.
//
// A save state is the answer to "what would it take to put this machine back
// where it is right now". Everything else in the emulator is either an input
// (the ROM, the buttons) or a derived output (the framebuffer), and neither
// belongs in a state file.
//
//   in the state            not in the state
//   ------------            ----------------
//   CPU registers           the framebuffer        -- pure output, redrawn
//   CPU cycle counter       the APU sample queue   -- pure output
//   console RAM             PRG ROM                -- came from the file
//   PPU timing and memory   CHR ROM                -- came from the file
//   APU channel phases      the disassembler       -- a view, not a state
//   mapper bank registers
//
// That "not" column is the whole design. Every byte left out is a byte that
// cannot disagree with what the machine would compute for itself. The
// framebuffer is 240KB and is redrawn within one frame; a state file that
// carried it would be twenty times larger and could be wrong.
//
// The one thing that is *not* derivable and is easy to forget: the mapper's
// bank registers. They are not memory, they are the wiring of a circuit board,
// and if they are missing the picture comes back showing the wrong part of the
// ROM. Cartridge::read_prg goes through them on the very next fetch, so a
// state without them survives about three instructions.
//
// The format
// ----------
// A magic number, a version, then fields in a fixed order. No keys, no
// self-describing structure, because the only reader is the build that wrote
// it and a version number is enough to say "this is not yours".
//
// Every multi byte value is written **little endian explicitly**, not by
// copying the host's bytes. That costs a shift and a mask and buys two things:
// the same state file means the same thing on arm64 and on wasm32, and the two
// builds' state files can be compared with `cmp`. The second is how the cross
// compilation parity test covers save states at all.
//
// On a version bump
// -----------------
// Old files stop loading, on purpose. Silently reading a v1 file as v2 would
// produce a machine that is subtly wrong rather than an error message, and a
// subtly wrong save state is indistinguishable from an emulator bug.
// ---------------------------------------------------------------------------

#include "core/types.hpp"

#include <array>
#include <cstddef>
#include <cstring>
#include <span>
#include <vector>

namespace fc {

/// The first four bytes of every state, so a file can be recognised.
inline constexpr std::array<u8, 4> kStateMagic = { 'F', 'C', 'S', 'T' };

/// Bumped whenever the meaning of any field below changes.
inline constexpr u32 kStateVersion = 1;

// ---------------------------------------------------------------------------
// Writing
//
// The methods are named put_/get_ rather than u8/u16/... on purpose. A member
// function called `u8` shadows the type alias `fc::u8` for the rest of the
// class body, so `static_cast<u16>(x)` inside another method resolves `u16` to
// the member function and stops compiling. Cheaper to avoid the collision than
// to qualify every type name for the life of the file.
// ---------------------------------------------------------------------------

class StateWriter {
public:
    /// Grow a vector as needed. What a save to disk uses.
    StateWriter() = default;

    /// Write into a buffer the caller already owns.
    ///
    /// Rewind saves thirty times a second. Thirty allocations a second is
    /// thirty garbage collections a minute, and in a frame loop that is not a
    /// cost, it is a stutter. With a fixed buffer there is nothing to allocate
    /// and nothing to collect.
    ///
    /// The price is that a state too large for the buffer is *refused* rather
    /// than accommodated, which is what overflowed() reports. A caller that
    /// sized its buffer from state_size() will never see it; one that guessed
    /// will be told.
    explicit StateWriter(std::span<u8> fixed) noexcept
        : fixed_(fixed)
        , fixed_mode_(true)
    {
    }

    void put_u8(u8 value) { write(&value, 1); }

    void put_u16(u16 value)
    {
        put_u8(static_cast<u8>(value & 0xFF));
        put_u8(static_cast<u8>((value >> 8) & 0xFF));
    }

    void put_u32(u32 value)
    {
        put_u16(static_cast<u16>(value & 0xFFFF));
        put_u16(static_cast<u16>((value >> 16) & 0xFFFF));
    }

    void put_u64(u64 value)
    {
        put_u32(static_cast<u32>(value & 0xFFFFFFFF));
        put_u32(static_cast<u32>((value >> 32) & 0xFFFFFFFF));
    }

    void put_s32(s32 value) { put_u32(static_cast<u32>(value)); }
    void put_flag(bool value) { put_u8(value ? 1 : 0); }

    void put_f32(f32 value) { put_u32(std::bit_cast<u32>(value)); }
    void put_f64(f64 value) { put_u64(std::bit_cast<u64>(value)); }

    void raw(const void* data, std::size_t size) { write(data, size); }

    /// A byte order that does not depend on the host: a std::array<u8, N> is
    /// exactly N bytes, with no length and no padding.
    template <std::size_t N>
    void blob(const std::array<u8, N>& values)
    {
        raw(values.data(), N);
    }

    /// A byte vector whose length is part of the state, so a cartridge with
    /// more or less RAM than expected is caught on read rather than overrunning.
    void sized_bytes(std::span<const u8> values)
    {
        put_u32(static_cast<u32>(values.size()));
        raw(values.data(), values.size());
    }

    /// True if the fixed buffer ran out. The state is then incomplete and must
    /// not be used.
    [[nodiscard]] bool overflowed() const noexcept { return overflowed_; }

    /// How many bytes were written.
    [[nodiscard]] std::size_t size() const noexcept
    {
        return fixed_mode_ ? written_ : bytes_.size();
    }

    /// Only valid in the growing mode. Empty in the fixed mode, where the
    /// caller already knows where the bytes are.
    [[nodiscard]] const std::vector<u8>& data() const noexcept { return bytes_; }

private:
    void write(const void* data, std::size_t size)
    {
        if (fixed_mode_) {
            if (fixed_.size() < size) {
                overflowed_ = true;
                return;
            }
            std::memcpy(fixed_.data(), data, size);
            fixed_ = fixed_.subspan(size);
            written_ += size;
            return;
        }

        const auto* first = static_cast<const u8*>(data);
        bytes_.insert(bytes_.end(), first, first + size);
    }

    std::vector<u8> bytes_;
    std::span<u8> fixed_{};
    std::size_t written_ = 0;
    bool fixed_mode_ = false;
    bool overflowed_ = false;
};

// ---------------------------------------------------------------------------
// Reading
//
// Every read either succeeds or latches a failure. Once failed, the reader
// stays failed and reports zeros, so a caller can read a whole state without
// checking anything and then ask once at the end. A truncated or corrupt file
// produces a false return, never a half loaded machine.
// ---------------------------------------------------------------------------

class StateReader {
public:
    explicit StateReader(std::span<const u8> data) : data_(data) {}

    bool get_u8(u8& value) { return take(&value, 1); }

    bool get_u16(u16& value)
    {
        u8 lo = 0;
        u8 hi = 0;
        if (!take(&lo, 1) || !take(&hi, 1)) {
            return false;
        }
        value = static_cast<u16>(lo | (static_cast<u16>(hi) << 8));
        return true;
    }

    bool get_u32(u32& value)
    {
        u16 lo = 0;
        u16 hi = 0;
        if (!get_u16(lo) || !get_u16(hi)) {
            return false;
        }
        value = static_cast<u32>(lo) | (static_cast<u32>(hi) << 16);
        return true;
    }

    bool get_u64(u64& value)
    {
        u32 lo = 0;
        u32 hi = 0;
        if (!get_u32(lo) || !get_u32(hi)) {
            return false;
        }
        value = static_cast<u64>(lo) | (static_cast<u64>(hi) << 32);
        return true;
    }

    bool get_s32(s32& value)
    {
        u32 bits = 0;
        if (!get_u32(bits)) {
            return false;
        }
        value = static_cast<s32>(bits);
        return true;
    }

    bool get_flag(bool& value)
    {
        u8 byte = 0;
        if (!get_u8(byte)) {
            return false;
        }
        value = byte != 0;
        return true;
    }

    bool get_f32(f32& value)
    {
        u32 bits = 0;
        if (!get_u32(bits)) {
            return false;
        }
        value = std::bit_cast<f32>(bits);
        return true;
    }

    bool get_f64(f64& value)
    {
        u64 bits = 0;
        if (!get_u64(bits)) {
            return false;
        }
        value = std::bit_cast<f64>(bits);
        return true;
    }

    bool raw(void* destination, std::size_t size) { return take(destination, size); }

    template <std::size_t N>
    bool blob(std::array<u8, N>& values)
    {
        return raw(values.data(), N);
    }

    bool sized_bytes(std::span<u8> destination)
    {
        u32 size = 0;
        if (!get_u32(size)) {
            return false;
        }
        // The length in the file has to match the length expected, or the two
        // builds disagree about the shape of the machine and nothing after
        // this point means anything.
        if (size != destination.size()) {
            return fail();
        }
        return raw(destination.data(), destination.size());
    }

    /// False if anything at all went wrong. Check this once, at the end.
    [[nodiscard]] bool ok() const noexcept { return ok_; }

    /// True only if everything was consumed and nothing failed. A state with
    /// trailing bytes was written by something that knows more than this
    /// reader does, which is a reason to refuse it.
    [[nodiscard]] bool finished() const noexcept { return ok_ && at_ == data_.size(); }

    [[nodiscard]] std::size_t consumed() const noexcept { return at_; }

private:
    bool fail()
    {
        ok_ = false;
        return false;
    }

    bool take(void* destination, std::size_t size)
    {
        if (!ok_ || at_ + size > data_.size()) {
            if (destination != nullptr && size > 0) {
                std::memset(destination, 0, size);
            }
            return fail();
        }
        std::memcpy(destination, data_.data() + at_, size);
        at_ += size;
        return true;
    }

    std::span<const u8> data_;
    std::size_t at_ = 0;
    bool ok_ = true;
};

// ---------------------------------------------------------------------------
// The machine's state, in one place.
//
// Everything below is a private member of some class that has no other reason
// to expose it. Rather than give each of those classes a pair of serialize
// methods and scatter the definition of "a NES state" across six files, this
// one struct is a friend of all of them and the whole format is readable in
// src/core/state.cpp, top to bottom.
//
// The section functions are private members rather than free functions in the
// .cpp because friendship is not transitive: a free function in that
// translation unit has no more access than anybody else.
// ---------------------------------------------------------------------------

namespace nes {
class Apu;
class Cartridge;
class Machine;
class NesBus;
class Ppu;
}

class Cpu;

struct StateAccess {
    /// Write every byte of the machine's state. Never fails.
    static void save(const nes::Machine& machine, StateWriter& out);

    /// Read it back into a machine that already has the same cartridge loaded.
    ///
    /// Returns false on a bad magic number, a version it does not know, a
    /// different cartridge, or a truncated file. Everything that can be
    /// checked is checked *before* the machine is touched, so a false return
    /// leaves a running machine exactly as it was.
    static bool load(nes::Machine& machine, StateReader& in);

private:
    static void write_cpu(const Cpu& cpu, StateWriter& out);
    static bool read_cpu(Cpu& cpu, StateReader& in);

    static void write_bus(const nes::NesBus& bus, StateWriter& out);
    static bool read_bus(nes::NesBus& bus, StateReader& in);

    static void write_ppu(const nes::Ppu& ppu, StateWriter& out);
    static bool read_ppu(nes::Ppu& ppu, StateReader& in);

    static void write_apu(const nes::Apu& apu, StateWriter& out);
    static bool read_apu(nes::Apu& apu, StateReader& in);

    static void write_cartridge(const nes::Cartridge& cartridge, StateWriter& out);
    static bool read_cartridge(nes::Cartridge& cartridge, StateReader& in);
};

} // namespace fc
