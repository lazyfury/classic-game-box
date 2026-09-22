#pragma once

// ---------------------------------------------------------------------------
// Mapper 9, the MMC2.
//
//     PRG ROM:  8KB banks, two switchable, two pinned
//     CHR ROM:  4KB banks, switched by the PPU itself
//
// The MMC2's idea is that a CHR bank switch can happen *while the picture is
// being drawn*, triggered by the PPU fetching a particular tile. There is no
// CPU involved at all.
//
// The mechanism
// -------------
// Two latches, one per 4KB half of the pattern tables. Each latch picks one
// of two registers:
//
//     $0000-$0FFF  latch0 ? R1 : R0
//     $1000-$1FFF  latch1 ? R3 : R2
//
// The latch flips when the PPU fetches one of two marker tiles:
//
//     tile $FD  ->  latch = 1
//     tile $FE  ->  latch = 0
//
// So a game draws a big object, puts tile $FD at the edge of its left half
// and tile $FE at the edge of its right half, and the pattern table changes
// bank exactly where the object crosses over. Punch-Out!! uses it for the
// boxers; the effect is a character using more than 256 tiles without ever
// touching the CHR register from code.
//
// This is the same PPU hook MMC3 uses, watching a different pattern.
// ---------------------------------------------------------------------------

#include "core/nes/mapper.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper9 : public Mapper {
public:
    Mapper9(std::vector<u8> prg, std::vector<u8> chr, Mirroring mirroring)
        : prg_(std::move(prg))
        , chr_(std::move(chr))
        , mirroring_(mirroring)
    {
    }

    [[nodiscard]] u8 read_prg(u16 address) override
    {
        if (prg_.empty()) {
            return 0;
        }
        const std::size_t banks = prg_bank_count();
        const std::size_t second_last = (banks >= 2u) ? (banks - 2u) : 0u;
        const std::size_t last = banks - 1u;

        std::size_t bank = 0;
        if (address < 0xA000u) {
            bank = prg_reg_[0];
        } else if (address < 0xC000u) {
            bank = second_last;
        } else if (address < 0xE000u) {
            bank = prg_reg_[1];
        } else {
            bank = last;
        }

        bank %= banks;
        return prg_[bank * 0x2000u + static_cast<std::size_t>(address & 0x1FFFu)];
    }

    void write_prg(u16 address, u8 value) override
    {
        switch (address & 0xF000u) {
        case 0xA000u: prg_reg_[0] = static_cast<u8>(value & 0x0Fu); break;
        case 0xB000u: chr_reg_[0] = static_cast<u8>(value & 0x1Fu); break;
        case 0xC000u: prg_reg_[1] = static_cast<u8>(value & 0x0Fu); break;
        case 0xD000u: chr_reg_[1] = static_cast<u8>(value & 0x1Fu); break;
        case 0xE000u: chr_reg_[2] = static_cast<u8>(value & 0x1Fu); break;
        case 0xF000u: chr_reg_[3] = static_cast<u8>(value & 0x1Fu); break;
        default: break;
        }
    }

    [[nodiscard]] u8 read_chr(u16 address) override
    {
        const std::size_t banks = chr_.size() / 0x1000u;
        if (banks == 0) {
            return 0;
        }
        const std::size_t reg = (address < 0x1000u)
                                    ? (latch_[0] ? 1u : 0u)
                                    : (latch_[1] ? 3u : 2u);
        const std::size_t bank = static_cast<std::size_t>(chr_reg_[reg]) % banks;
        return chr_[bank * 0x1000u + static_cast<std::size_t>(address & 0x0FFFu)];
    }

    void write_chr(u16 /*address*/, u8 /*value*/) override
    {
        // CHR ROM.
    }

    [[nodiscard]] Mirroring mirroring() const noexcept override { return mirroring_; }

    void on_ppu_address(u16 address) override
    {
        switch (address & 0x1FF0u) {
        case 0x0FD0u: latch_[0] = true; break;
        case 0x0FE0u: latch_[0] = false; break;
        case 0x1FD0u: latch_[1] = true; break;
        case 0x1FE0u: latch_[1] = false; break;
        default: break;
        }
    }

    // -- inspection ----------------------------------------------------------

    [[nodiscard]] bool latch(int half) const noexcept
    {
        return latch_[(half != 0) ? 1 : 0];
    }

protected:
    std::vector<u8> prg_;
    std::vector<u8> chr_;
    Mirroring mirroring_;
    u8 prg_reg_[2] = { 0, 0 };
    u8 chr_reg_[4] = { 0, 0, 0, 0 };
    bool latch_[2] = { false, false };

private:
    [[nodiscard]] std::size_t prg_bank_count() const noexcept
    {
        const std::size_t count = prg_.size() / 0x2000u;
        return (count == 0) ? 1 : count;
    }
};

} // namespace fc::nes
