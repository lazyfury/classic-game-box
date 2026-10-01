#pragma once

// ---------------------------------------------------------------------------
// Mapper 66, GxROM (also sold as GNROM).
//
//     $8000-$FFFF  write:  [PPPP CCCC]
//                          PPPP = 32KB PRG bank
//                          CCCC = 8KB CHR bank
//     Mirroring: fixed by the board
//
// The cheapest possible way to make a cartridge bigger: one latch, one
// register, and both ROMs move together. There is no fixed bank at the top -
// a bank switch moves the reset vectors too - so a game has to be written to
// survive it. Super Mario Bros. + Duck Hunt is the usual example.
//
// Only the low two bits of each half are connected to anything on a real
// board (4 x 32KB = 128KB of PRG, 4 x 8KB = 32KB of CHR), so the upper bits
// are dropped rather than stored.
// ---------------------------------------------------------------------------

#include "core/nes/mapper.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper66 : public Mapper {
public:
    Mapper66(std::vector<u8> prg, std::vector<u8> chr, Mirroring mirroring)
        : prg_(std::move(prg))
        , chr_(std::move(chr))
        , mirroring_(mirroring)
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
        const std::size_t bank = static_cast<std::size_t>(prg_bank_) % banks;
        return prg_[bank * 0x8000u + static_cast<std::size_t>(address & 0x7FFFu)];
    }

    void write_prg(u16 /*address*/, u8 value) override
    {
        prg_bank_ = static_cast<u8>((value >> 4) & 0x03u);
        chr_bank_ = static_cast<u8>(value & 0x03u);
    }

    [[nodiscard]] u8 read_chr(u16 address) override
    {
        return chr_[chr_offset(address)];
    }

    void write_chr(u16 address, u8 value) override
    {
        if (chr_ram_) {
            chr_[chr_offset(address)] = value;
        }
    }

    [[nodiscard]] Mirroring mirroring() const noexcept override { return mirroring_; }

    [[nodiscard]] u8 prg_bank() const noexcept { return prg_bank_; }
    [[nodiscard]] u8 chr_bank() const noexcept { return chr_bank_; }

private:
    [[nodiscard]] std::size_t prg_bank_count() const noexcept
    {
        const std::size_t count = prg_.size() / 0x8000u;
        return (count == 0) ? 1 : count;
    }

    [[nodiscard]] std::size_t chr_offset(u16 address) const noexcept
    {
        if (chr_.empty()) {
            return 0;
        }
        const std::size_t banks = chr_.size() / 0x2000u;
        const std::size_t bank = static_cast<std::size_t>(chr_bank_) % ((banks == 0) ? 1 : banks);
        return bank * 0x2000u + static_cast<std::size_t>(address & 0x1FFFu);
    }

    std::vector<u8> prg_;
    std::vector<u8> chr_;
    Mirroring mirroring_;
    bool chr_ram_ = false;
    u8 prg_bank_ = 0;
    u8 chr_bank_ = 0;
};

} // namespace fc::nes
