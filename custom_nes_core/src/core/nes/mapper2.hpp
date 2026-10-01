#pragma once

// ---------------------------------------------------------------------------
// Mapper 2, the UxROM.
//
//     PRG ROM:  16KB switchable at $8000, the LAST 16KB fixed at $C000
//     CHR:      8KB CHR RAM, no banking at all
//     + one bank register, written anywhere in $8000-$FFFF
//
// UxROM is the second simplest mapper after NROM and it is the first one that
// is actually useful, because it is where the trick that every later mapper
// copies shows up:
//
//     hold the fixed bank at the TOP, so the interrupt vectors never move,
//     and switch the code below it
//
// $FFFA-$FFFF has to contain the NMI, reset and IRQ vectors at all times. If
// the switchable bank covered that range, every bank switch would have to
// carry its own copy of the vectors. Pinning the last bank means the vectors
// only ever exist once, and the game gets 48KB of switchable code in a
// 16KB-at-a-time window.
//
// The CHR is RAM. Nothing is banked because there is nothing to bank: the
// game copies the tiles it needs into the 8KB of RAM and redraws them when it
// wants different ones. That is why UxROM games tend to have small, repeated
// tilesets.
//
// Mega Man, Castlevania, Contra and Duck Tales are all UxROM.
// ---------------------------------------------------------------------------

#include "core/nes/mapper.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper2 : public Mapper {
public:
    Mapper2(std::vector<u8> prg, std::vector<u8> chr, Mirroring mirroring)
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
        const std::size_t bank = (address < 0xC000u) ? (bank_ % banks) : (banks - 1u);
        return prg_[bank * 0x4000u + static_cast<std::size_t>(address & 0x3FFFu)];
    }

    void write_prg(u16 /*address*/, u8 value) override
    {
        // Anywhere in $8000-$FFFF. There are at most 16 banks on a real
        // UxROM, so the high bits are not connected.
        bank_ = static_cast<u8>(value & 0x0Fu);
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
    u8 bank_ = 0;

    // -- save states ---------------------------------------------------------
    //
    // UxROM has exactly one bank register, and it is the whole of the state.
    // CHR is ROM on every UxROM board, but the vector is written anyway when
    // the header asked for CHR RAM, because nothing stops a homebrew doing
    // that.

public:
    void serialize(StateWriter& out) const override
    {
        out.put_u8(bank_);
        out.put_flag(chr_ram_);
        if (chr_ram_) {
            out.sized_bytes(chr_);
        }
    }

    bool deserialize(StateReader& in) override
    {
        in.get_u8(bank_);
        in.get_flag(chr_ram_);
        if (chr_ram_) {
            in.sized_bytes(chr_);
        }
        return in.ok();
    }

    [[nodiscard]] bool saves_state() const noexcept override { return true; }

private:
};

} // namespace fc::nes
