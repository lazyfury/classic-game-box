#pragma once

// ---------------------------------------------------------------------------
// Mapper 68, the Sunsoft-4.
//
//     $8000-$B000  4 x 2KB CHR banks
//     $C000/$D000  two nametable banks taken from CHR ROM (rarely used)
//     $E000        [.... N.MM]  M = mirroring, N = use CHR for nametables
//     $F000        [.E.. RBBB]  B = 16KB PRG bank, R = enable work RAM,
//                               E = use an external ROM instead
//     PRG: $8000-$BFFF switchable, $C000-$FFFF bank 7
//
// A mid-80s Sunsoft board. The interesting feature is $C000/$D000: the board
// can point the nametables at four 1KB windows *inside the CHR ROM*, which is
// how After Burner gets a whole extra plane of scroll data without owning
// more VRAM. That part is not emulated yet, so those registers are accepted
// and ignored; every other register is.
// ---------------------------------------------------------------------------

#include "core/nes/mapper.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper68 : public Mapper {
public:
    Mapper68(std::vector<u8> prg, std::vector<u8> chr, Mirroring mirroring)
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
                                     : ((banks >= 8u) ? 7u : (banks - 1u));
        return prg_[bank * 0x4000u + static_cast<std::size_t>(address & 0x3FFFu)];
    }

    void write_prg(u16 address, u8 value) override
    {
        switch (address & 0xF000u) {
        case 0x8000: chr_reg_[0] = value; break;
        case 0x9000: chr_reg_[1] = value; break;
        case 0xA000: chr_reg_[2] = value; break;
        case 0xB000: chr_reg_[3] = value; break;
        case 0xC000: case 0xD000: break;   // CHR nametables, not emulated
        case 0xE000:
            switch (value & 0x03u) {
            case 0: mirroring_ = Mirroring::Vertical; break;
            case 1: mirroring_ = Mirroring::Horizontal; break;
            case 2: mirroring_ = Mirroring::SingleScreenLower; break;
            default: mirroring_ = Mirroring::SingleScreenUpper; break;
            }
            break;
        default:   // $F000
            prg_bank_ = static_cast<u8>(value & 0x07u);
            break;
        }
    }

    [[nodiscard]] u8 read_chr(u16 address) override
    {
        if (chr_.empty()) {
            return 0;
        }
        const std::size_t banks = chr_.size() / 0x800u;
        const std::size_t slot = static_cast<std::size_t>(address >> 11) & 0x03u;
        const std::size_t bank = static_cast<std::size_t>(chr_reg_[slot]) % ((banks == 0) ? 1 : banks);
        return chr_[bank * 0x800u + static_cast<std::size_t>(address & 0x7FFu)];
    }

    void write_chr(u16 /*address*/, u8 /*value*/) override {}

    [[nodiscard]] Mirroring mirroring() const noexcept override { return mirroring_; }

    [[nodiscard]] u8 prg_bank() const noexcept { return prg_bank_; }
    [[nodiscard]] u8 chr_register(int index) const noexcept
    {
        return (index >= 0 && index < 4) ? chr_reg_[index] : 0;
    }

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
    u8 chr_reg_[4] = { 0, 0, 0, 0 };
};

} // namespace fc::nes
