#pragma once

// ---------------------------------------------------------------------------
// Mapper 246, a Chinese multicart board.
//
//     $6000-$67FF  write:  [.... .RRR]
//                          RRR 0-3  -> 8KB PRG window RRR
//                          RRR 4-7  -> 2KB CHR window (RRR - 4)
//     PRG: 4 x 8KB windows, together 32KB
//     CHR: 4 x 2KB windows, together 8KB
//
// The board has no work RAM, so $6000-$7FFF belongs to the registers, which
// is why has_work_ram() is false. The top 8KB PRG window is pinned to bank
// $FF, which for a small ROM wraps around to the last bank - the multicart
// menu lives there.
// ---------------------------------------------------------------------------

#include "core/nes/mapper.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper246 : public Mapper {
public:
    Mapper246(std::vector<u8> prg, std::vector<u8> chr, Mirroring mirroring)
        : prg_(std::move(prg))
        , chr_(std::move(chr))
        , mirroring_(mirroring)
    {
        prg_bank_[3] = 0xFF;
    }

    void make_chr_ram(std::size_t size = 8192)
    {
        chr_.assign(size, 0);
        chr_ram_ = true;
    }

    [[nodiscard]] bool has_work_ram() const noexcept override { return false; }

    [[nodiscard]] u8 read_prg(u16 address) override
    {
        if (prg_.empty()) {
            return 0;
        }
        const std::size_t banks = prg_bank_count();
        const std::size_t slot = static_cast<std::size_t>(address >> 13) & 0x03u;
        const std::size_t bank = static_cast<std::size_t>(prg_bank_[slot]) % banks;
        return prg_[bank * 0x2000u + static_cast<std::size_t>(address & 0x1FFFu)];
    }

    void write_prg(u16 /*address*/, u8 /*value*/) override {}

    void write_expansion(u16 address, u8 value) override
    {
        if (address < 0x6000u || address > 0x67FFu) {
            return;
        }
        const u8 reg = static_cast<u8>(address & 0x07u);
        if (reg <= 0x03u) {
            prg_bank_[reg] = value;
        } else {
            chr_bank_[reg & 0x03u] = value;
        }
    }

    [[nodiscard]] u8 read_chr(u16 address) override
    {
        if (chr_.empty()) {
            return 0;
        }
        const std::size_t banks = chr_.size() / 0x800u;
        const std::size_t slot = static_cast<std::size_t>(address >> 11) & 0x03u;
        const std::size_t bank = static_cast<std::size_t>(chr_bank_[slot]) % ((banks == 0) ? 1 : banks);
        return chr_[bank * 0x800u + static_cast<std::size_t>(address & 0x7FFu)];
    }

    void write_chr(u16 address, u8 value) override
    {
        if (!chr_ram_) {
            return;
        }
        const std::size_t banks = chr_.size() / 0x800u;
        const std::size_t slot = static_cast<std::size_t>(address >> 11) & 0x03u;
        const std::size_t bank = static_cast<std::size_t>(chr_bank_[slot]) % ((banks == 0) ? 1 : banks);
        chr_[bank * 0x800u + static_cast<std::size_t>(address & 0x7FFu)] = value;
    }

    [[nodiscard]] Mirroring mirroring() const noexcept override { return mirroring_; }

    [[nodiscard]] u8 prg_bank(int index) const noexcept
    {
        return (index >= 0 && index < 4) ? prg_bank_[index] : 0;
    }

private:
    [[nodiscard]] std::size_t prg_bank_count() const noexcept
    {
        const std::size_t count = prg_.size() / 0x2000u;
        return (count == 0) ? 1 : count;
    }

    std::vector<u8> prg_;
    std::vector<u8> chr_;
    Mirroring mirroring_;
    bool chr_ram_ = false;
    u8 prg_bank_[4] = { 0, 0, 0, 0xFF };
    u8 chr_bank_[4] = { 0, 0, 0, 0 };
};

} // namespace fc::nes
