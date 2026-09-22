#pragma once

// ---------------------------------------------------------------------------
// Mapper 178, a Waixing board.
//
//     $4800-$4FFF  write:  four registers, one per address bit 0-1
//     PRG: 2 x 16KB windows
//     CHR: 8KB
//
// Waixing moved the bank registers out of the CPU's usual $8000-$FFFF window
// and down into the expansion area at $4800, on purpose: an unmodified
// cartridge cannot be swapped onto this board without its code writing there.
//
// The register meanings:
//
//     reg0 bit 0  mirroring (1 = horizontal)
//     reg0 bit 1  use a 16+16 split instead of a 32KB bank
//     reg0 bit 2  repeat the same 16KB in both halves
//     reg1 bits 0-2  low bank bits
//     reg2           high bank bits
//     reg3 bits 0-1  work RAM bank (this emulator has one 8KB page)
//
// `bank = (reg2 << 3) | (reg1 & 7)` is where the 16KB number comes from.
// ---------------------------------------------------------------------------

#include "core/nes/mapper.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper178 : public Mapper {
public:
    Mapper178(std::vector<u8> prg, std::vector<u8> chr, Mirroring mirroring)
        : prg_(std::move(prg))
        , chr_(std::move(chr))
        , mirroring_(mirroring)
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

    void write_prg(u16 /*address*/, u8 /*value*/) override {}

    void write_expansion(u16 address, u8 value) override
    {
        if (address < 0x4800u || address > 0x4FFFu) {
            return;
        }
        reg_[address & 0x03u] = value;
        update();
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

    void update() noexcept
    {
        const u8 small = static_cast<u8>(reg_[1] & 0x07u);
        const u8 big = reg_[2];
        const u8 bank = static_cast<u8>((big << 3) | small);

        if ((reg_[0] & 0x02u) != 0) {
            page_lo_ = bank;
            if ((reg_[0] & 0x04u) != 0) {
                page_hi_ = static_cast<u8>((big << 3) | 0x06u | (reg_[1] & 0x01u));
            } else {
                page_hi_ = static_cast<u8>((big << 3) | 0x07u);
            }
        } else if ((reg_[0] & 0x04u) != 0) {
            page_lo_ = bank;
            page_hi_ = bank;
        } else {
            page_lo_ = static_cast<u8>(bank & 0xFEu);
            page_hi_ = static_cast<u8>(page_lo_ + 1u);
        }

        mirroring_ = ((reg_[0] & 0x01u) != 0) ? Mirroring::Horizontal
                                              : Mirroring::Vertical;
    }

    std::vector<u8> prg_;
    std::vector<u8> chr_;
    Mirroring mirroring_;
    bool chr_ram_ = false;
    u8 reg_[4] = { 0, 0, 0, 0 };
    u8 page_lo_ = 0;
    u8 page_hi_ = 1;
};

} // namespace fc::nes
