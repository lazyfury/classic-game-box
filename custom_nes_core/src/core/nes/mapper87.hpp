#pragma once

// ---------------------------------------------------------------------------
// Mapper 87, the Jaleco JF-13.
//
//     $6000-$7FFF  write:  [..CP PPPP]  (masked to $6000 and $7000)
//                          $6000: PPPP = 32KB PRG bank, C = high CHR bit
//                          $7000: expansion audio (not emulated)
//     PRG: one 32KB bank, no fixed half
//     CHR: one 8KB bank, three bits wide
//
// This board has no work RAM: $6000-$7FFF is the register, which is why
// has_work_ram() is false and the Cartridge forwards those cycles to
// write_expansion() instead of to a RAM chip.
//
// The CHR number is split across two nibbles on purpose - the board only had
// so many pins - so the 8KB bank is `(D0-D1) | (D4 << 2)`.
// ---------------------------------------------------------------------------

#include "core/nes/mapper.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper87 : public Mapper {
public:
    Mapper87(std::vector<u8> prg, std::vector<u8> chr, Mirroring mirroring)
        : prg_(std::move(prg))
        , chr_(std::move(chr))
        , mirroring_(mirroring)
    {
    }

    [[nodiscard]] bool has_work_ram() const noexcept override { return false; }

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
        if (address < 0x6000u || address > 0x7FFFu) {
            return;
        }
        if ((address & 0x7000u) != 0x6000u) {
            return;   // $7000 is the (unemulated) audio register
        }
        prg_bank_ = static_cast<u8>((value & 0x30u) >> 4);
        chr_bank_ = static_cast<u8>((value & 0x03u) | ((value >> 4) & 0x04u));
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
        const std::size_t count = prg_.size() / 0x8000u;
        return (count == 0) ? 1 : count;
    }

    std::vector<u8> prg_;
    std::vector<u8> chr_;
    Mirroring mirroring_;
    u8 prg_bank_ = 0;
    u8 chr_bank_ = 0;
};

} // namespace fc::nes
