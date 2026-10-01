#pragma once

// ---------------------------------------------------------------------------
// Mapper 71, the Codemasters BF909x.
//
//     $8000-$BFFF  write:  [..M. BBBB]  M = mirroring, BBBB = PRG bank
//     $C000-$FFFF  write:  [.... BBBB]  BBBB = PRG bank
//     PRG: $8000-$BFFF switchable 16KB, $C000-$FFFF fixed last 16KB
//     CHR: 8KB RAM
//
// Codemasters built this out of a single 74 series latch, so the register is
// just the address bus: the value on the data bus is the bank, and one
// address line is wired to the mirroring pin.
//
// The mirroring bit only exists on the BF9097 board (Firehawk). Mesen and
// FCEUX detect it from the fact that the game writes to $9000 at all; the
// same trick is used here, so a plain BF9093 game keeps the header's
// mirroring.
// ---------------------------------------------------------------------------

#include "core/nes/mapper.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper71 : public Mapper {
public:
    Mapper71(std::vector<u8> prg, std::vector<u8> chr, Mirroring mirroring)
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
        const std::size_t bank = (address < 0xC000u)
                                     ? (static_cast<std::size_t>(bank_) % banks)
                                     : (banks - 1u);
        return prg_[bank * 0x4000u + static_cast<std::size_t>(address & 0x3FFFu)];
    }

    void write_prg(u16 address, u8 value) override
    {
        if (address == 0x9000u) {
            bf9097_ = true;
        }

        if (address >= 0xC000u || !bf9097_) {
            bank_ = static_cast<u8>(value & 0x0Fu);
        } else {
            mirroring_ = ((value & 0x10u) != 0) ? Mirroring::SingleScreenLower
                                                : Mirroring::SingleScreenUpper;
        }
    }

    [[nodiscard]] u8 read_chr(u16 address) override
    {
        if (chr_.empty()) {
            return 0;
        }
        return chr_[static_cast<std::size_t>(address) % chr_.size()];
    }

    void write_chr(u16 address, u8 value) override
    {
        if (chr_ram_) {
            chr_[static_cast<std::size_t>(address) % chr_.size()] = value;
        }
    }

    [[nodiscard]] Mirroring mirroring() const noexcept override { return mirroring_; }

    [[nodiscard]] u8 prg_bank() const noexcept { return bank_; }
    [[nodiscard]] bool is_bf9097() const noexcept { return bf9097_; }

private:
    [[nodiscard]] std::size_t prg_bank_count() const noexcept
    {
        const std::size_t count = prg_.size() / 0x4000u;
        return (count == 0) ? 1 : count;
    }

    std::vector<u8> prg_;
    std::vector<u8> chr_;
    Mirroring mirroring_;
    bool chr_ram_ = false;
    bool bf9097_ = false;
    u8 bank_ = 0;
};

} // namespace fc::nes
