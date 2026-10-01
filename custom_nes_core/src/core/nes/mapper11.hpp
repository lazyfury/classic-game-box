#pragma once

// ---------------------------------------------------------------------------
// Mapper 11, the Color Dreams board.
//
//     PRG ROM:  fixed, 32KB (or 16KB mirrored)
//     CHR ROM:  8KB banks, switched by a write to $8000-$FFFF
//
// This is CNROM again, with one difference that made Color Dreams' unlicensed
// cartridges work on a console that had no way to stop them: the value written
// selects the bank in its low bits, but the high bits are a "lock". Some
// revisions ignore every write after the first, which is how they stopped a
// game from being copied by a simpler board.
//
// The unlicensed games are the library: Bible Adventures, Crystal Mines, the
// Wisdom Tree titles. There is no bus conflict to model because the board was
// built not to have one.
// ---------------------------------------------------------------------------

#include "core/nes/mapper.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper11 : public Mapper {
public:
    Mapper11(std::vector<u8> prg, std::vector<u8> chr, Mirroring mirroring)
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
        return prg_[static_cast<std::size_t>(address - 0x8000u) % prg_.size()];
    }

    void write_prg(u16 /*address*/, u8 value) override
    {
        chr_bank_ = static_cast<u8>(value & 0x0Fu);
    }

    [[nodiscard]] u8 read_chr(u16 address) override
    {
        const std::size_t banks = chr_.size() / 0x2000u;
        if (banks == 0) {
            return 0;
        }
        const std::size_t bank = static_cast<std::size_t>(chr_bank_) % banks;
        return chr_[bank * 0x2000u + static_cast<std::size_t>(address & 0x1FFFu)];
    }

    void write_chr(u16 /*address*/, u8 /*value*/) override {}

    [[nodiscard]] Mirroring mirroring() const noexcept override { return mirroring_; }

    [[nodiscard]] u8 chr_bank() const noexcept { return chr_bank_; }

private:
    std::vector<u8> prg_;
    std::vector<u8> chr_;
    Mirroring mirroring_;
    u8 chr_bank_ = 0;
};

} // namespace fc::nes
