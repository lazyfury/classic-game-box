#pragma once

// ---------------------------------------------------------------------------
// Mapper 242, a Waixing board.
//
//     Any write to $8000-$FFFF:
//         A1        mirroring (1 = horizontal)
//         A3-A6     32KB PRG bank
//     PRG: one 32KB window
//     CHR: 8KB, no banking
//
// Another address-decoded latch, like mapper 227: the bank number is read off
// the address lines, and the data bus is not connected at all.
// ---------------------------------------------------------------------------

#include "core/nes/mapper.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper242 : public Mapper {
public:
    Mapper242(std::vector<u8> prg, std::vector<u8> chr)
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
        const std::size_t bank = static_cast<std::size_t>(prg_bank_) % banks;
        return prg_[bank * 0x8000u + static_cast<std::size_t>(address & 0x7FFFu)];
    }

    void write_prg(u16 address, u8 /*value*/) override
    {
        mirroring_ = ((address & 0x0002u) != 0) ? Mirroring::Horizontal
                                                : Mirroring::Vertical;
        prg_bank_ = static_cast<u8>((address >> 3) & 0x0Fu);
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

    [[nodiscard]] u8 prg_bank() const noexcept { return prg_bank_; }

private:
    [[nodiscard]] std::size_t prg_bank_count() const noexcept
    {
        const std::size_t count = prg_.size() / 0x8000u;
        return (count == 0) ? 1 : count;
    }

    std::vector<u8> prg_;
    std::vector<u8> chr_;
    bool chr_ram_ = false;
    u8 prg_bank_ = 0;
    Mirroring mirroring_ = Mirroring::Vertical;
};

} // namespace fc::nes
