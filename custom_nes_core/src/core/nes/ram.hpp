#pragma once

// ---------------------------------------------------------------------------
// The NES's 2KB of work RAM.
//
// This class is also the explanation of memory mirroring.
//
// A 2KB chip needs 11 address lines: 2^11 = 2048. The 6502 has 16. So the
// top 5 address lines are simply NOT CONNECTED to the RAM chip.
//
//     CPU address lines   A15 A14 A13 A12 A11 | A10 ... A0
//                          |   |   |   |   |     \_______/
//                          |   |   |   |   |         |
//                          |   |   |   |   |     not wired
//                          \___|___|___|___/          |
//                                   |                 |
//                              chip select        RAM chip
//                              (decoder logic)    (11 lines)
//
// The consequence is mirroring, and it is not something we implement: it is
// what happens when you do not wire the bits up.
//
//     $0000  ---+
//     $0800  ---+-- all four are the same byte
//     $1000  ---+
//     $1800  ---+
//
// Masking with 0x07FF inside this class IS the missing wires. Putting the
// mask here rather than in the bus is the physically accurate choice: the
// bus does the chip select, the chip does the wrapping.
// ---------------------------------------------------------------------------

#include "core/types.hpp"

#include <array>

namespace fc::nes {

class Ram {
public:
    /// 2KB, so 11 address lines.
    static constexpr u16 kSize = 0x0800;
    static constexpr u16 kMask = kSize - 1;   // 0x07FF

    /// The mask is the point: only the low 11 bits of the address reach the
    /// chip, exactly as on the board.
    [[nodiscard]] u8 read(u16 address) const noexcept
    {
        return bytes_[address & kMask];
    }

    void write(u16 address, u8 value) noexcept
    {
        bytes_[address & kMask] = value;
    }

    void clear() noexcept
    {
        bytes_.fill(0);
    }

    /// Direct access, for tests and the future debugger. `index` is 0..$07FF.
    [[nodiscard]] const std::array<u8, kSize>& bytes() const noexcept { return bytes_; }

    /// The chip's bytes as a pointer, for a front end that persists or
    /// searches them. The mask above is still the only way the CPU reaches
    /// them; this is the view for everything that is not the CPU.
    [[nodiscard]] u8* data() noexcept { return bytes_.data(); }
    [[nodiscard]] const u8* data() const noexcept { return bytes_.data(); }

private:
    std::array<u8, kSize> bytes_{};
};

} // namespace fc::nes
