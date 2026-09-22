#pragma once

// ---------------------------------------------------------------------------
// Mapper 32, the IREM G-101.
//
//     $8000  [.... ...P]  P  = mirroring (0 = vertical, 1 = horizontal)
//            [..BB BBBB]  B  = 8KB PRG bank for slot 0
//     $9000  [.... ..MM]  MM = PRG layout mode
//     $A000  [..BB BBBB]  B  = 8KB PRG bank for slot 1
//     $B000-$B007        8 x 1KB CHR banks
//
// Four 8KB PRG windows, and the last two are pinned to the top of the ROM:
//
//     mode 0:  [ reg0 ][ reg1 ][ -2 ][ -1 ]
//     mode 1:  [  -2  ][ reg1 ][ reg0 ][ -1 ]
//
// The second mode exists so a game can keep a small fixed kernel at $8000 and
// put data banks at $C000. The CHR side is eight 1KB banks, one per register
// in the $B000 page.
// ---------------------------------------------------------------------------

#include "core/nes/mapper.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper32 : public Mapper {
public:
    Mapper32(std::vector<u8> prg, std::vector<u8> chr, Mirroring mirroring)
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
        const std::size_t slot = static_cast<std::size_t>(address >> 13) & 0x03u;
        const std::size_t bank = prg_slot(slot, banks) % banks;
        return prg_[bank * 0x2000u + static_cast<std::size_t>(address & 0x1FFFu)];
    }

    void write_prg(u16 address, u8 value) override
    {
        switch (address & 0xF000u) {
        case 0x8000: prg_reg_[0] = static_cast<u8>(value & 0x1Fu); break;
        case 0x9000:
            prg_mode_ = static_cast<u8>((value >> 1) & 0x01u);
            mirroring_ = ((value & 0x01u) != 0) ? Mirroring::Horizontal
                                                : Mirroring::Vertical;
            break;
        case 0xA000: prg_reg_[1] = static_cast<u8>(value & 0x1Fu); break;
        case 0xB000: chr_reg_[address & 0x07u] = value; break;
        default: break;
        }
    }

    [[nodiscard]] u8 read_chr(u16 address) override
    {
        if (chr_.empty()) {
            return 0;
        }
        const std::size_t banks = chr_.size() / 0x400u;
        const std::size_t slot = static_cast<std::size_t>(address >> 10) & 0x07u;
        const std::size_t bank = static_cast<std::size_t>(chr_reg_[slot]) % ((banks == 0) ? 1 : banks);
        return chr_[bank * 0x400u + static_cast<std::size_t>(address & 0x3FFu)];
    }

    void write_chr(u16 /*address*/, u8 /*value*/) override {}

    [[nodiscard]] Mirroring mirroring() const noexcept override { return mirroring_; }

    [[nodiscard]] u8 prg_register(int index) const noexcept
    {
        return (index >= 0 && index < 2) ? prg_reg_[index] : 0;
    }
    [[nodiscard]] u8 prg_mode() const noexcept { return prg_mode_; }

private:
    [[nodiscard]] std::size_t prg_bank_count() const noexcept
    {
        const std::size_t count = prg_.size() / 0x2000u;
        return (count == 0) ? 1 : count;
    }

    [[nodiscard]] std::size_t prg_slot(std::size_t slot, std::size_t banks) const noexcept
    {
        const std::size_t second_last = (banks >= 2u) ? (banks - 2u) : 0u;
        const std::size_t last = banks - 1u;

        if (prg_mode_ == 0) {
            switch (slot) {
            case 0: return prg_reg_[0];
            case 1: return prg_reg_[1];
            case 2: return second_last;
            default: return last;
            }
        }
        switch (slot) {
        case 0: return second_last;
        case 1: return prg_reg_[1];
        case 2: return prg_reg_[0];
        default: return last;
        }
    }

    std::vector<u8> prg_;
    std::vector<u8> chr_;
    Mirroring mirroring_;
    u8 prg_reg_[2] = { 0, 0 };
    u8 chr_reg_[8] = { 0, 0, 0, 0, 0, 0, 0, 0 };
    u8 prg_mode_ = 0;
};

} // namespace fc::nes
