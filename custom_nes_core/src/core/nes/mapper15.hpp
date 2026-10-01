#pragma once

// ---------------------------------------------------------------------------
// Mapper 15, the "100-in-1" multicart.
//
//     PRG ROM:  16KB switchable at $8000-$BFFF, the LAST 16KB fixed at
//               $C000-$FFFF
//     CHR:      8KB CHR RAM
//     + one bit of the register also picks single-screen mirroring
//
// This is not a licensed board, it is the cheapest way to put a menu and a
// pile of small games on one cartridge. The register is written anywhere in
// $8000-$FFFF: the low six bits are the PRG bank, bit 6 picks which half of
// the 2KB VRAM the nametables answer, and that is the whole chip.
//
// Why the top bank is pinned
// --------------------------
// The reset, NMI and IRQ vectors live at $FFFA-$FFFF. If those moved with
// the bank, the multicart's menu would have no fixed place to put its own
// interrupt handlers, and every bank would need its own copy. Pinning the
// last 16KB means the menu's code and vectors always answer, while the
// switchable half below holds the selected program.
//
// Boot sequence, verified against "100合1.NES":
//   1. Power on with the register at 0, so $C000-$FFFF is the last bank.
//   2. The CPU reads the reset vector from the fixed bank: $C001.
//   3. The menu runs from the fixed bank, draws its list from the switchable
//      bank, and writes the register to load a game.
//
// Getting the granularity wrong (32KB) makes the machine read its reset
// vector out of bank 0 instead, which is a different program and hangs. That
// is exactly what happened before this comment was written.
// ---------------------------------------------------------------------------

#include "core/nes/mapper.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper15 : public Mapper {
public:
    Mapper15(std::vector<u8> prg, std::vector<u8> chr)
        : prg_(std::move(prg))
        , chr_(std::move(chr))
    {
    }

    void make_chr_ram(std::size_t size = 8192)
    {
        chr_.assign(size, 0);
        chr_ram_ = true;
    }

    [[nodiscard]] u8 read_prg(u16 address) override
    {
        if (prg_.empty()) {
            return 0;
        }
        const std::size_t banks = prg_bank_count();

        // $8000-$BFFF switches; $C000-$FFFF is pinned to the last 16KB so the
        // reset and interrupt vectors never move. That is the same trick as
        // UxROM, and it is what lets the menu live in the fixed bank while a
        // game sits in the switchable one.
        if (address < 0xC000u) {
            const std::size_t bank =
                static_cast<std::size_t>(bank_ & 0x3Fu) % banks;
            return prg_[bank * 0x4000u + static_cast<std::size_t>(address & 0x3FFFu)];
        }
        return prg_[(banks - 1u) * 0x4000u + static_cast<std::size_t>(address & 0x3FFFu)];
    }

    void write_prg(u16 /*address*/, u8 value) override
    {
        bank_ = static_cast<u8>(value & 0x3Fu);
        mirroring_ = ((value & 0x40u) != 0) ? Mirroring::SingleScreenUpper
                                            : Mirroring::SingleScreenLower;
    }

    [[nodiscard]] u8 read_chr(u16 address) override
    {
        if (chr_.empty()) {
            return 0;
        }
        return chr_[static_cast<std::size_t>(address) % chr_.size()];
    }

    void write_chr(u16 address, u8 value) override
    {
        if (chr_ram_) {
            chr_[static_cast<std::size_t>(address) % chr_.size()] = value;
        }
    }

    [[nodiscard]] Mirroring mirroring() const noexcept override { return mirroring_; }

    [[nodiscard]] u8 prg_bank() const noexcept { return bank_; }

private:
    [[nodiscard]] std::size_t prg_bank_count() const noexcept
    {
        const std::size_t count = prg_.size() / 0x4000u;
        return (count == 0) ? 1 : count;
    }

    std::vector<u8> prg_;
    std::vector<u8> chr_;
    Mirroring mirroring_ = Mirroring::SingleScreenLower;
    bool chr_ram_ = false;
    u8 bank_ = 0;
};

} // namespace fc::nes
