#pragma once

// ---------------------------------------------------------------------------
// Mapper 10, the MMC4.
//
// Same CHR latches as the MMC2, different PRG arrangement:
//
//     PRG ROM:  16KB banks. $8000-$BFFF fixed to the second-to-last bank,
//               $C000-$FFFF switchable. One register.
//     CHR ROM:  the MMC2 latch pair, unchanged.
//
// MMC2 and MMC4 exist as two numbers because the board designer changed how
// much PRG a game could reach, not because the CHR trick changed. Fire Emblem
// and Famicom Wars are MMC4.
//
// The small differences from MMC2 are:
//   * 16KB PRG granularity instead of 8KB
//   * the fixed bank is the LOW half, not the second and last of four
//   * the register addresses shift down by one ($A000 = PRG, $B000-$E000 =
//     the four CHR banks)
// ---------------------------------------------------------------------------

#include "core/nes/mapper9.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper10 : public Mapper9 {
public:
    Mapper10(std::vector<u8> prg, std::vector<u8> chr, Mirroring mirroring)
        : Mapper9(std::move(prg), std::move(chr), mirroring)
    {
    }

    [[nodiscard]] u8 read_prg(u16 address) override
    {
        if (prg_.empty()) {
            return 0;
        }
        const std::size_t banks = prg_.size() / 0x4000u;
        const std::size_t second_last = (banks >= 2u) ? (banks - 2u) : 0u;

        const std::size_t bank = (address < 0xC000u)
                                     ? (second_last % banks)
                                     : (static_cast<std::size_t>(prg_reg_[0]) % banks);
        return prg_[bank * 0x4000u + static_cast<std::size_t>(address & 0x3FFFu)];
    }

    void write_prg(u16 address, u8 value) override
    {
        switch (address & 0xF000u) {
        case 0xA000u: prg_reg_[0] = static_cast<u8>(value & 0x0Fu); break;
        case 0xB000u: chr_reg_[0] = static_cast<u8>(value & 0x1Fu); break;
        case 0xC000u: chr_reg_[1] = static_cast<u8>(value & 0x1Fu); break;
        case 0xD000u: chr_reg_[2] = static_cast<u8>(value & 0x1Fu); break;
        case 0xE000u: chr_reg_[3] = static_cast<u8>(value & 0x1Fu); break;
        default: break;
        }
    }
};

} // namespace fc::nes
