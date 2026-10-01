#pragma once

// ---------------------------------------------------------------------------
// Mapper 199, an MMC3 that keeps four registers of its own.
//
// The board is an MMC3 with 256KB of pattern ROM and 8KB of pattern RAM, and
// it rewires the MMC3's lower half:
//
//     $0000-$03FF   CHR register 0
//     $0400-$07FF   the board's own register 2
//     $0800-$0BFF   CHR register 1
//     $0C00-$0FFF   the board's own register 3
//     $1000-$1FFF   the MMC3's usual upper slots
//
// and the two top PRG windows:
//
//     $C000-$FFFF   the board's own registers 0 and 1
//
// So four writes to $8001 that the MMC3 would have used for banks are
// captured by the board instead, whenever bank select bit 3 is set. CHR page
// numbers below 8 come from the RAM and 8 and up from the ROM: the page
// number is the selector, exactly as on mapper 74.
//
// It also has the four-way mirroring an MMC3 does not: vertical, horizontal,
// and one screen each way. This is the mapper behind Dragon Ball Z 2, San Guo
// Zhi 2 and 大盗伍佑卫门 天下宝藏.
// ---------------------------------------------------------------------------

#include "core/nes/mapper4.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper199 : public Mapper4 {
public:
    Mapper199(std::vector<u8> prg, std::vector<u8> chr, Mirroring default_mirroring)
        : Mapper4(std::move(prg), std::move(chr), default_mirroring)
    {
        // Power-on values. Register 0 is the second-to-last 8KB of PRG, which
        // is where an MMC3's reset vector expects to find itself, and register
        // 1 is the last.
        ex_regs_[0] = 0xFE;
        ex_regs_[1] = 0xFF;
        ex_regs_[2] = 1;
        ex_regs_[3] = 3;
        make_chr_ram_window(0x2000);
    }

    [[nodiscard]] u8 read_prg(u16 address) override
    {
        switch (address & 0xE000u) {
        case 0xC000u:
            return read_prg_bank(ex_regs_[0], address);
        case 0xE000u:
            return read_prg_bank(ex_regs_[1], address);
        default:
            return Mapper4::read_prg(address);
        }
    }

    void write_prg(u16 address, u8 value) override
    {
        // The board's own four registers, taken from the MMC3's bank data
        // write whenever bank select bit 3 is set.
        if (address == 0x8001u && (bank_select_ & 0x08u) != 0u) {
            ex_regs_[bank_select_ & 0x03u] = value;
            return;
        }

        // Four-way mirroring instead of the MMC3's two.
        if ((address & 0xE000u) == 0xA000u && (address & 0x0001u) == 0u) {
            switch (value & 0x03u) {
            case 0: mirroring_ = Mirroring::Vertical; break;
            case 1: mirroring_ = Mirroring::Horizontal; break;
            case 2: mirroring_ = Mirroring::SingleScreenLower; break;
            default: mirroring_ = Mirroring::SingleScreenUpper; break;
            }
            return;
        }

        Mapper4::write_prg(address, value);
    }

    void serialize(StateWriter& out) const override
    {
        Mapper4::serialize(out);
        for (const u8 value : ex_regs_) {
            out.put_u8(value);
        }
    }

    bool deserialize(StateReader& in) override
    {
        if (!Mapper4::deserialize(in)) {
            return false;
        }
        for (u8& value : ex_regs_) {
            in.get_u8(value);
        }
        return in.ok();
    }

protected:
    [[nodiscard]] bool chr_page_is_ram(std::size_t page) const noexcept override
    {
        return page < 8u;
    }

    /// The lower four 1KB slots do not follow the MMC3's mode bits at all:
    /// they are two MMC3 registers interleaved with two of the board's own.
    [[nodiscard]] std::size_t map_chr_bank(std::size_t slot,
                                           std::size_t bank) const noexcept override
    {
        switch (slot) {
        case 0: return chr_reg_[0];
        case 1: return ex_regs_[2];
        case 2: return chr_reg_[1];
        case 3: return ex_regs_[3];
        default: return bank;
        }
    }

private:
    u8 ex_regs_[4] = { 0, 0, 0, 0 };
};

} // namespace fc::nes
