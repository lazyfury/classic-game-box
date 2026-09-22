#pragma once

// ---------------------------------------------------------------------------
// Mapper 74, the Waixing MMC3.
//
// An MMC3 with a piece of pattern RAM whose place is chosen by the page number
// rather than by a mode bit, plus 4KB of work RAM below the MMC3's usual 8KB.
//
// Where the pattern RAM is
// ------------------------
// The header lies. These cartridges are dumped with a CHR page count for ROM
// that is not there, and the game writes its tiles into pages the header
// calls ROM. The writes are what say where the RAM is: 天使之翼2 writes CHR
// pages 0-3 thousands of times and touches no other page, so the lower 4KB of
// the pattern side is RAM and everything above it is ROM.
//
// It is worth saying that FCEUX documents this mapper as a 2KB window at pages
// 8 and 9. Either that is a different board of the same number or the header
// it was written against numbered its pages differently. The data decides:
// pages 0-3, and the rest of the ROM stays where the game expects it.
//
// Where the work RAM is
// ---------------------
// $5000-$5FFF is RAM, beside $6000-$7FFF. The games copy a short bank-switch
// routine down there and call the copy, so that the call survives the bank it
// switches away from, and they clear the range on the way through.
//
// 天使之翼2, 天神之剑 and 封神榜 are the cartridges this was written for.
// ---------------------------------------------------------------------------

#include "core/nes/mapper4.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper74 : public Mapper4 {
public:
    Mapper74(std::vector<u8> prg, std::vector<u8> chr, Mirroring default_mirroring)
        : Mapper4(std::move(prg), std::move(chr), default_mirroring)
        , exp_ram_(0x1000, 0)
    {
        make_chr_ram_window(0x1000);   // 4KB, CHR pages 0-3
    }

    /// 4KB of RAM at $5000-$5FFF, beside the MMC3's usual 8KB at $6000.
    [[nodiscard]] u8 read_expansion(u16 address) override
    {
        if (address >= 0x5000u && address <= 0x5FFFu) {
            return exp_ram_[static_cast<std::size_t>(address - 0x5000u)];
        }
        return 0;
    }

    void write_expansion(u16 address, u8 value) override
    {
        if (address >= 0x5000u && address <= 0x5FFFu) {
            exp_ram_[static_cast<std::size_t>(address - 0x5000u)] = value;
        }
    }

    void serialize(StateWriter& out) const override
    {
        Mapper4::serialize(out);
        out.sized_bytes(exp_ram_);
    }

    bool deserialize(StateReader& in) override
    {
        if (!Mapper4::deserialize(in)) {
            return false;
        }
        in.sized_bytes(exp_ram_);
        return in.ok();
    }

protected:
    [[nodiscard]] bool chr_page_is_ram(std::size_t page) const noexcept override
    {
        return page < 4u;
    }

private:
    std::vector<u8> exp_ram_;
};

} // namespace fc::nes
