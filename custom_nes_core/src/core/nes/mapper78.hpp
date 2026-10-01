#pragma once

// ---------------------------------------------------------------------------
// Mapper 78, the Jaleco JF-16.
//
//     $8000-$FFFF  write:  [CCCC MBBB]
//                          CCCC = 8KB CHR bank
//                          M    = mirroring (board dependent)
//                          BBB  = 16KB PRG bank
//     PRG: $8000-$BFFF switchable, $C000-$FFFF fixed last 16KB
//     CHR: 8KB banks
//
// A tidy little board - one register, two ROMs. The mirroring bit is where
// the boards differ: Holy Diver wires it to horizontal/vertical, while the
// others wire it to the two single-screen halves. With only an iNES header
// there is no way to tell them apart, so the single-screen wiring is used,
// which is what Mesen and FCEUX default to. Holy Diver's status bar is the
// one visible casualty.
// ---------------------------------------------------------------------------

#include "core/nes/mapper.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper78 : public Mapper {
public:
    Mapper78(std::vector<u8> prg, std::vector<u8> chr, Mirroring mirroring)
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
        const std::size_t bank = (address < 0xC000u)
                                     ? (static_cast<std::size_t>(prg_bank_) % banks)
                                     : (banks - 1u);
        return prg_[bank * 0x4000u + static_cast<std::size_t>(address & 0x3FFFu)];
    }

    void write_prg(u16 /*address*/, u8 value) override
    {
        prg_bank_ = static_cast<u8>(value & 0x07u);
        chr_bank_ = static_cast<u8>((value >> 4) & 0x0Fu);
        mirroring_ = ((value & 0x08u) != 0) ? Mirroring::SingleScreenUpper
                                            : Mirroring::SingleScreenLower;
    }

    [[nodiscard]] u8 read_chr(u16 address) override
    {
        if (chr_.empty()) {
            return 0;
        }
        const std::size_t banks = chr_.size() / 0x2000u;
        const std::size_t bank = static_cast<std::size_t>(chr_bank_) % ((banks == 0) ? 1 : banks);
        return chr_[bank * 0x2000u + static_cast<std::size_t>(address & 0x1FFFu)];
    }

    void write_chr(u16 /*address*/, u8 /*value*/) override {}

    [[nodiscard]] Mirroring mirroring() const noexcept override { return mirroring_; }

    [[nodiscard]] u8 prg_bank() const noexcept { return prg_bank_; }
    [[nodiscard]] u8 chr_bank() const noexcept { return chr_bank_; }

private:
    [[nodiscard]] std::size_t prg_bank_count() const noexcept
    {
        const std::size_t count = prg_.size() / 0x4000u;
        return (count == 0) ? 1 : count;
    }

    std::vector<u8> prg_;
    std::vector<u8> chr_;
    Mirroring mirroring_;
    u8 prg_bank_ = 0;
    u8 chr_bank_ = 0;
};

} // namespace fc::nes
