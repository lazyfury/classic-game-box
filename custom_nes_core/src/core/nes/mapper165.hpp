#pragma once

// ---------------------------------------------------------------------------
// Mapper 165, an MMC3 that borrows the MMC2's trick.
//
// The pattern side is divided into two 4KB halves, and each half can point at
// either a 4KB page of the pattern ROM or at one 4KB of pattern RAM:
//
//     $0000-$0FFF   register R0, or R1
//     $1000-$1FFF   register R2, or R4
//
// Which of the pair is in use is not chosen by a register. It is chosen by
// what the PPU is drawing: when it fetches tile $FD the first of the pair
// applies, when it fetches tile $FE the second does, and each half of the
// pattern tables remembers its own. That is the MMC2's latch, and it exists so
// that a game can change the tiles a sprite uses without touching the tiles
// the background is using in the same frame.
//
// A page number of zero means the half is RAM. That is how the board gets
// 4KB of rewritable tiles into a cartridge that is otherwise ROM.
//
// 火焰之纹章 暗黑龙与光之剑 (the game this was written for) is a 512KB/128KB
// cartridge with a battery.
// ---------------------------------------------------------------------------

#include "core/nes/mapper4.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper165 : public Mapper4 {
public:
    Mapper165(std::vector<u8> prg, std::vector<u8> chr, Mirroring default_mirroring)
        : Mapper4(std::move(prg), std::move(chr), default_mirroring)
    {
        make_chr_ram_window(0x1000);   // 4KB, one 4KB half
    }

    // -- the PPU's view -------------------------------------------------------

    void on_ppu_address(u16 address) override
    {
        // The A12 edge the MMC3's scanline counter runs on, first.
        Mapper4::on_ppu_address(address);

        // Then the MMC2 latch. `address & 0x2FF8` is the tile number and the
        // half of it being fetched; the pattern table is bit 12 and the half
        // of the tile is bit 3. Each pattern table latches separately.
        switch (address & 0x2FF8u) {
        case 0x0FD0u:
        case 0x0FE8u:
            latch_[(address >> 12) & 0x01u] = (address & 0x08u) == 0x08u;
            break;
        default:
            break;
        }
    }

    [[nodiscard]] u8 read_chr(u16 address) override
    {
        const std::size_t half = (address >> 12) & 0x01u;
        const std::size_t page = page_for_half(half);
        const std::size_t within = (address >> 10) & 0x03u;

        if (page == 0u) {
            if (chr_ram_window_.empty()) {
                return 0;
            }
            return chr_ram_window_[within * 0x400u
                                   + static_cast<std::size_t>(address & 0x3FFu)];
        }

        const std::size_t bank = (page >> 2) * 4u + within;
        const std::size_t banks = chr_.size() / 0x400u;
        if (banks == 0u) {
            return 0;
        }
        return chr_[(bank % banks) * 0x400u
                    + static_cast<std::size_t>(address & 0x3FFu)];
    }

    void write_chr(u16 address, u8 value) override
    {
        const std::size_t half = (address >> 12) & 0x01u;
        if (page_for_half(half) != 0u || chr_ram_window_.empty()) {
            return;
        }
        const std::size_t within = (address >> 10) & 0x03u;
        chr_ram_window_[within * 0x400u
                        + static_cast<std::size_t>(address & 0x3FFu)] = value;
    }

    void serialize(StateWriter& out) const override
    {
        Mapper4::serialize(out);
        out.put_flag(latch_[0]);
        out.put_flag(latch_[1]);
    }

    bool deserialize(StateReader& in) override
    {
        if (!Mapper4::deserialize(in)) {
            return false;
        }
        in.get_flag(latch_[0]);
        in.get_flag(latch_[1]);
        return in.ok();
    }

private:
    /// The register that feeds one 4KB half, chosen by that half's latch.
    [[nodiscard]] std::size_t page_for_half(std::size_t half) const noexcept
    {
        if (half == 0u) {
            return chr_reg_[latch_[0] ? 1u : 0u];
        }
        return chr_reg_[latch_[1] ? 4u : 2u];
    }

    bool latch_[2] = { false, false };
};

} // namespace fc::nes
