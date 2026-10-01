#pragma once

// ---------------------------------------------------------------------------
// Mapper 162, a Waixing board.
//
//     $5000-$5FFF  write:  four registers (A8-A9 select one)
//     PRG: one 32KB window
//     CHR: 8KB
//
// The PRG bank is assembled out of reg0's high bits, reg1 and reg2, and reg3
// decides *how* those bits are glued together - so the same four registers
// mean four different banks depending on reg3:
//
//     reg3 & 5 == 0:  (reg0 & 0C) | (reg1 & 02) | (reg2 << 4)
//     reg3 & 5 == 1:  (reg0 & 0C)            | (reg2 << 4)
//     reg3 & 5 == 4:  (reg0 & 0E) | ((reg1 >> 1) & 1) | (reg2 << 4)
//     reg3 & 5 == 5:  (reg0 & 0F)            | (reg2 << 4)
//
// Reset leaves reg0 = 3, reg1 = 0, reg2 = 0, reg3 = 7, which is the fifth
// row: bank 3, the usual place a bank-switched cart boots from.
// ---------------------------------------------------------------------------

#include "core/nes/mapper.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper162 : public Mapper {
public:
    Mapper162(std::vector<u8> prg, std::vector<u8> chr, Mirroring mirroring)
        : prg_(std::move(prg))
        , chr_(std::move(chr))
        , mirroring_(mirroring)
    {
        reg_[0] = 3;
        reg_[1] = 0;
        reg_[2] = 0;
        reg_[3] = 7;
        update();
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
        if (address < 0x5000u || address > 0x5FFFu) {
            return;
        }
        reg_[(address >> 8) & 0x03u] = value;
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

    [[nodiscard]] u8 prg_bank() const noexcept { return prg_bank_; }

private:
    [[nodiscard]] std::size_t prg_bank_count() const noexcept
    {
        const std::size_t count = prg_.size() / 0x8000u;
        return (count == 0) ? 1 : count;
    }

    void update() noexcept
    {
        switch (reg_[3] & 0x05u) {
        case 0:
            prg_bank_ = static_cast<u8>((reg_[0] & 0x0Cu) | (reg_[1] & 0x02u) |
                                        ((reg_[2] & 0x0Fu) << 4));
            break;
        case 1:
            prg_bank_ = static_cast<u8>((reg_[0] & 0x0Cu) | ((reg_[2] & 0x0Fu) << 4));
            break;
        case 4:
            prg_bank_ = static_cast<u8>((reg_[0] & 0x0Eu) | ((reg_[1] >> 1) & 0x01u) |
                                        ((reg_[2] & 0x0Fu) << 4));
            break;
        default:   // 5
            prg_bank_ = static_cast<u8>((reg_[0] & 0x0Fu) | ((reg_[2] & 0x0Fu) << 4));
            break;
        }
    }

    std::vector<u8> prg_;
    std::vector<u8> chr_;
    Mirroring mirroring_;
    bool chr_ram_ = false;
    u8 reg_[4] = { 0, 0, 0, 0 };
    u8 prg_bank_ = 3;
};

} // namespace fc::nes
