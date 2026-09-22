#pragma once

// ---------------------------------------------------------------------------
// Mapper 190, the "Magic Kid Goo Goo" board.
//
//     $8000-$9FFF  write:  PRG 16KB bank for $8000 (low three bits)
//     $A000-$BFFF  write:  2KB CHR bank (A0-A1 select one of four)
//     $C000-$DFFF  write:  PRG 16KB bank for $8000, plus 8
//     $E000-$FFFF  write:  2KB CHR bank
//     Mirroring: fixed vertical
//
// A tiny Chinese board. The only unusual part is that the same physical PRG
// register can be written in two places - once with a bank in the low half of
// the ROM and once with `bank | 8`, which is how the game reaches a bank
// above 128KB without a second register.
// ---------------------------------------------------------------------------

#include "core/nes/mapper.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper190 : public Mapper {
public:
    Mapper190(std::vector<u8> prg, std::vector<u8> chr)
        : prg_(std::move(prg))
        , chr_(std::move(chr))
    {
    }

    void make_chr_ram(std::size_t size = 8192)
    {
        chr_.assign(size, 0);
        chr_ram_ = true;
    }

    [[nodiscard]] u8 read_prg(u16 address) override
    {
        if (prg_.empty()) {
            return 0;
        }
        const std::size_t banks = prg_bank_count();
        // Only the low window is switchable on this board; the top 16KB
        // stays on bank 0, because the register has no second output wired
        // to it.
        const std::size_t bank = ((address < 0xC000u)
                                      ? static_cast<std::size_t>(prg_bank_)
                                      : 0u) % banks;
        return prg_[bank * 0x4000u + static_cast<std::size_t>(address & 0x3FFFu)];
    }

    void write_prg(u16 address, u8 value) override
    {
        if (address >= 0x8000u && address <= 0x9FFFu) {
            prg_bank_ = static_cast<u8>(value & 0x07u);
        } else if (address >= 0xC000u && address <= 0xDFFFu) {
            prg_bank_ = static_cast<u8>((value & 0x07u) | 0x08u);
        } else if ((address & 0xA000u) == 0xA000u) {
            chr_bank_[address & 0x03u] = value;
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

    /// Fixed, and not from the header.
    [[nodiscard]] Mirroring mirroring() const noexcept override { return Mirroring::Vertical; }

    [[nodiscard]] u8 prg_bank() const noexcept { return prg_bank_; }
    [[nodiscard]] u8 chr_bank(int index) const noexcept
    {
        return (index >= 0 && index < 4) ? chr_bank_[index] : 0;
    }

private:
    [[nodiscard]] std::size_t prg_bank_count() const noexcept
    {
        const std::size_t count = prg_.size() / 0x4000u;
        return (count == 0) ? 1 : count;
    }

    std::vector<u8> prg_;
    std::vector<u8> chr_;
    bool chr_ram_ = false;
    u8 prg_bank_ = 0;
    u8 chr_bank_[4] = { 0, 0, 0, 0 };
};

} // namespace fc::nes
