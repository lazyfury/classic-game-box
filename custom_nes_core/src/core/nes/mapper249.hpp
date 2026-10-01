#pragma once

// ---------------------------------------------------------------------------
// Mapper 249, an MMC3 clone with its bank lines crossed by a Waixing T9552.
//
//     Everything MMC3, plus:
//     $5000  write:  [.... .pPP]  P = PRG scramble, p = high CHR scramble bit
//
// A chip on the board permutes the MMC3's bank output before it reaches the
// ROM, so the console and the chip disagree about what "bank 5" means. The
// $5000 register selects which permutation is active.
//
// A mapper 249 ROM file is stored in the bank order the board produces when
// $5000=00, and every known game immediately writes $02, which is the order
// that looks unscrambled on the chip. The net effect, from the MMC3's output
// to this file, is therefore the pattern-0 column of the wiki's table:
//
//     PRG:  A14<->A16, A15<->A17        (an involution)
//     CHR:  a fixed six-bit shuffle
//
// Those permutations are applied to the bank numbers the MMC3's registers
// select. The two windows the MMC3 pins to the top of the ROM are left alone:
// they have to stay at the end, because that is where the reset vector lives.
//
// The wavetable sound of the chip the board is named after is not emulated.
// ---------------------------------------------------------------------------

#include "core/nes/mapper4.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper249 : public Mapper4 {
public:
    Mapper249(std::vector<u8> prg, std::vector<u8> chr, Mirroring mirroring)
        : Mapper4(std::move(prg), std::move(chr), mirroring)
    {
    }

    void write_expansion(u16 address, u8 value) override
    {
        if (address == 0x5000u) {
            scramble_ = value;
        }
    }

    [[nodiscard]] u8 scramble_register() const noexcept { return scramble_; }

protected:
    // Mapper 249 ROMs are stored in the bank order the board produces when
    // $5000=00 (that is what the iNES mapper number means). The T9552 then
    // permutes the MMC3's address lines; for the $5000 values the games
    // actually use, the net mapping from the MMC3's output to this file is
    // the pattern-0 column of the wiki table:
    //
    //     MMC3 A14 -> ROM A16     MMC3 A16 -> ROM A15
    //     MMC3 A15 -> ROM A17     MMC3 A17 -> ROM A14
    //
    // (8KB PRG bank bits: bit1=A14, bit2=A15, bit3=A16, bit4=A17.)
    [[nodiscard]] std::size_t map_prg_bank(std::size_t bank) const noexcept override
    {
        const unsigned p = static_cast<unsigned>(bank);
        return (p & ~0x1Eu) |
               (((p >> 4) & 0x01u) << 1) |
               (((p >> 3) & 0x01u) << 2) |
               (((p >> 1) & 0x01u) << 3) |
               (((p >> 2) & 0x01u) << 4);
    }

    // The matching CHR column (1KB bank bits: bit2=A12 .. bit7=A17):
    //     MMC3 A12 -> ROM A15     MMC3 A15 -> ROM A17
    //     MMC3 A13 -> ROM A12     MMC3 A16 -> ROM A14
    //     MMC3 A14 -> ROM A16     MMC3 A17 -> ROM A13
    [[nodiscard]] std::size_t map_chr_bank(std::size_t /*slot*/,
                                           std::size_t bank) const noexcept override
    {
        const unsigned p = static_cast<unsigned>(bank);
        return (p & ~0xFCu) |
               (((p >> 3) & 0x01u) << 2) |
               (((p >> 7) & 0x01u) << 3) |
               (((p >> 6) & 0x01u) << 4) |
               (((p >> 2) & 0x01u) << 5) |
               (((p >> 4) & 0x01u) << 6) |
               (((p >> 5) & 0x01u) << 7);
    }

private:
    u8 scramble_ = 0;
};

} // namespace fc::nes
