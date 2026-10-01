#pragma once

// ---------------------------------------------------------------------------
// Mapper 13, the CPROM.
//
//     PRG ROM:  fixed, 32KB
//     CHR:      16KB CHR RAM, one 4KB bank switchable
//
// CPROM is the smallest mapper that still has a register. It exists for one
// game, Videomation, whose whole job is drawing: the program rewrites the
// 16KB of CHR RAM as the canvas and uses the register to choose which 4KB
// bank the picture is looking at.
//
// The bank register is written anywhere in $8000-$FFFF and takes the low two
// bits. The top 4KB is pinned to the last bank; only the bottom 4KB moves.
//
// This is the only mapper here whose CHR is RAM with a *size other than the
// iNES default*, which is why the factory hands it 16KB instead of 8KB.
// ---------------------------------------------------------------------------

#include "core/nes/mapper.hpp"

#include <cstddef>
#include <cstdint>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper13 : public Mapper {
public:
    Mapper13(std::vector<u8> prg, Mirroring mirroring)
        : prg_(std::move(prg))
        , mirroring_(mirroring)
    {
        chr_.assign(kChrSize, 0);
    }

    [[nodiscard]] u8 read_prg(u16 address) override
    {
        if (prg_.empty()) {
            return 0;
        }
        return prg_[static_cast<std::size_t>(address - 0x8000u) % prg_.size()];
    }

    void write_prg(u16 /*address*/, u8 value) override
    {
        chr_bank_ = static_cast<u8>(value & 0x03u);
    }

    [[nodiscard]] u8 read_chr(u16 address) override
    {
        return chr_[chr_address(address)];
    }

    void write_chr(u16 address, u8 value) override
    {
        chr_[chr_address(address)] = value;
    }

    [[nodiscard]] Mirroring mirroring() const noexcept override { return mirroring_; }

    [[nodiscard]] u8 chr_bank() const noexcept { return chr_bank_; }
    [[nodiscard]] const std::vector<u8>& chr_ram() const noexcept { return chr_; }

private:
    static constexpr std::size_t kChrSize = 0x4000u;   // 16KB

    [[nodiscard]] std::size_t chr_address(u16 address) const noexcept
    {
        // The top half is pinned to the last 4KB bank.
        const std::size_t bank = (address < 0x1000u) ? chr_bank_ : 3u;
        return bank * 0x1000u + static_cast<std::size_t>(address & 0x0FFFu);
    }

    std::vector<u8> prg_;
    std::vector<u8> chr_;
    Mirroring mirroring_;
    u8 chr_bank_ = 0;
};

} // namespace fc::nes
