#pragma once

// ---------------------------------------------------------------------------
// Mapper 33, the Taito TC0190.
//
//     $8000  [MBB BBBB]  B = 8KB PRG bank at $8000, M = mirroring
//     $8001  [.BB BBBB]  B = 8KB PRG bank at $A000
//     $8002  [..BB BBBB] 2 x 1KB CHR banks (0 and 1)
//     $8003  [..BB BBBB] 2 x 1KB CHR banks (2 and 3)
//     $A000-$A003        4 x 1KB CHR banks (4 to 7)
//
// Two 8KB PRG windows at the bottom, the usual two pinned at the top, and
// eight 1KB CHR windows. The address decode is the interesting part: only
// A13, A1 and A0 are wired, so the register is selected by `addr & 0xA003`.
//
// Taito used this on games like Insector X; its sibling mapper 48
// (TC0690) adds an IRQ on top.
// ---------------------------------------------------------------------------

#include "core/nes/mapper.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper33 : public Mapper {
public:
    Mapper33(std::vector<u8> prg, std::vector<u8> chr, Mirroring mirroring)
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
        switch (address & 0xA003u) {
        case 0x8000:
            prg_reg_[0] = static_cast<u8>(value & 0x3Fu);
            mirroring_ = ((value & 0x40u) != 0) ? Mirroring::Horizontal
                                                : Mirroring::Vertical;
            break;
        case 0x8001:
            prg_reg_[1] = static_cast<u8>(value & 0x3Fu);
            break;
        case 0x8002:
            chr_reg_[0] = static_cast<u8>(value * 2u);
            chr_reg_[1] = static_cast<u8>(value * 2u + 1u);
            break;
        case 0x8003:
            chr_reg_[2] = static_cast<u8>(value * 2u);
            chr_reg_[3] = static_cast<u8>(value * 2u + 1u);
            break;
        case 0xA000: case 0xA001: case 0xA002: case 0xA003:
            chr_reg_[4u + (address & 0x03u)] = value;
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
    [[nodiscard]] u8 chr_register(int index) const noexcept
    {
        return (index >= 0 && index < 8) ? chr_reg_[index] : 0;
    }

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
        switch (slot) {
        case 0: return prg_reg_[0];
        case 1: return prg_reg_[1];
        case 2: return second_last;
        default: return last;
        }
    }

    std::vector<u8> prg_;
    std::vector<u8> chr_;
    Mirroring mirroring_;
    u8 prg_reg_[2] = { 0, 0 };
    u8 chr_reg_[8] = { 0, 0, 0, 0, 0, 0, 0, 0 };
};

} // namespace fc::nes
