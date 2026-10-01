#pragma once

// ---------------------------------------------------------------------------
// Mapper 227, a multicart board (1200-in-1 and friends).
//
//     Any write to $8000-$FFFF selects a bank from the *address*, not the
//     data: the menu's address decode is the register.
//
//     A11-A8 (A10-A8 via `addr >> 2`, A8 via `addr & 0x100`)
//     A0   small/large flag
//     A1   mirroring
//     A7   PRG mode (32KB pair vs. two banks)
//     A9   "large" flag, only meaningful when A0 is set
//
// This is a pirate board built out of a 74 series decoder rather than a
// latch, so the value written is ignored entirely. It is the same idea as
// mapper 15 and 226 - a cheap way to page a big ROM into 32KB - with the
// wiring spread over the address lines.
// ---------------------------------------------------------------------------

#include "core/nes/mapper.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper227 : public Mapper {
public:
    Mapper227(std::vector<u8> prg, std::vector<u8> chr)
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
        const std::size_t page = (address < 0xC000u) ? page_lo_ : page_hi_;
        return prg_[(page % banks) * 0x4000u + static_cast<std::size_t>(address & 0x3FFFu)];
    }

    void write_prg(u16 address, u8 /*value*/) override
    {
        const u16 bank = static_cast<u16>(((address >> 2) & 0x1Fu) |
                                          ((address & 0x0100u) >> 3));
        const bool small = (address & 0x0001u) != 0;
        const bool large = ((address >> 9) & 0x0001u) != 0;
        const bool prg_mode = ((address >> 7) & 0x0001u) != 0;

        if (prg_mode) {
            if (small) {
                page_lo_ = static_cast<u8>(bank & 0xFEu);
                page_hi_ = static_cast<u8>(page_lo_ + 1u);
            } else {
                page_lo_ = static_cast<u8>(bank);
                page_hi_ = static_cast<u8>(bank);
            }
        } else if (small) {
            page_lo_ = static_cast<u8>(bank & 0x3Eu);
            page_hi_ = large ? static_cast<u8>(bank | 0x07u)
                             : static_cast<u8>(bank & 0x38u);
        } else {
            page_lo_ = static_cast<u8>(bank);
            page_hi_ = large ? static_cast<u8>(bank | 0x07u)
                             : static_cast<u8>(bank & 0x38u);
        }

        mirroring_ = ((address & 0x0002u) != 0) ? Mirroring::Horizontal
                                                : Mirroring::Vertical;
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

    [[nodiscard]] u8 page_low() const noexcept { return page_lo_; }
    [[nodiscard]] u8 page_high() const noexcept { return page_hi_; }

private:
    [[nodiscard]] std::size_t prg_bank_count() const noexcept
    {
        const std::size_t count = prg_.size() / 0x4000u;
        return (count == 0) ? 1 : count;
    }

    std::vector<u8> prg_;
    std::vector<u8> chr_;
    bool chr_ram_ = false;
    u8 page_lo_ = 0;
    u8 page_hi_ = 1;
    Mirroring mirroring_ = Mirroring::Vertical;
};

} // namespace fc::nes
