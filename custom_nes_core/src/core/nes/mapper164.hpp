#pragma once

// ---------------------------------------------------------------------------
// Mapper 164, a Waixing board.
//
//     $5000  write:  [.... LLLL]  low four bits of the 32KB PRG bank
//     $5100  write:  [.... HHHH]  high four bits
//     PRG: one 32KB window
//     CHR: 8KB, no banking
//
// The simplest of the Waixing boards: the bank number is just two nibbles
// written to two addresses in the expansion area. Reset leaves the register
// at $0F, so the cart boots from bank 15 - the last bank of a 512KB ROM.
// ---------------------------------------------------------------------------

#include "core/nes/mapper.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper164 : public Mapper {
public:
    Mapper164(std::vector<u8> prg, std::vector<u8> chr, Mirroring mirroring)
        : prg_(std::move(prg))
        , chr_(std::move(chr))
        , mirroring_(mirroring)
        , prg_bank_(0x0F)
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

    void write_prg(u16 /*address*/, u8 /*value*/) override {}

    void write_expansion(u16 address, u8 value) override
    {
        switch (address & 0x7300u) {
        case 0x5000:
            prg_bank_ = static_cast<u8>((prg_bank_ & 0xF0u) | (value & 0x0Fu));
            break;
        case 0x5100:
            prg_bank_ = static_cast<u8>((prg_bank_ & 0x0Fu) | ((value & 0x0Fu) << 4));
            break;
        default:
            break;
        }
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
    Mirroring mirroring_;
    bool chr_ram_ = false;
    u8 prg_bank_ = 0x0F;
};

} // namespace fc::nes
