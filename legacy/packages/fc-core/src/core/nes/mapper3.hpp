#pragma once

// ---------------------------------------------------------------------------
// Mapper 3, the CNROM.
//
//     PRG ROM:  fixed, 16KB or 32KB, no banking
//     CHR ROM:  8KB banks, switched by a write to $8000-$FFFF
//
// This is the mirror image of UxROM. Where UxROM switches code and leaves the
// graphics alone, CNROM switches graphics and leaves the code alone.
//
// It is enough for a game whose problem is "more tiles", which was a lot of
// early games: Excitebike, Arkanoid, Gradius and Paperboy are all CNROM.
//
// Bus conflicts
// -------------
// A real CNROM board does not decode the write, so the ROM is still driving
// the data bus at the same time as the CPU. The two fight, and the value that
// lands in the register is `written AND rom[address]`. Games live with it by
// choosing a write address whose ROM byte has all the bits they need set.
//
// We do not model that fight. It only matters for a game that writes a value
// the ROM disagrees with, which would be a game that does not work on real
// hardware either.
// ---------------------------------------------------------------------------

#include "core/nes/mapper.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper3 : public Mapper {
public:
    Mapper3(std::vector<u8> prg, std::vector<u8> chr, Mirroring mirroring)
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
        // 16KB PRG mirrors into both halves, exactly like NROM.
        return prg_[static_cast<std::size_t>(address - 0x8000u) % prg_.size()];
    }

    void write_prg(u16 /*address*/, u8 value) override
    {
        chr_bank_ = static_cast<u8>(value & 0x03u);
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

    void write_chr(u16 /*address*/, u8 /*value*/) override
    {
        // CHR ROM: writes go nowhere.
    }

    [[nodiscard]] Mirroring mirroring() const noexcept override { return mirroring_; }

    [[nodiscard]] u8 chr_bank() const noexcept { return chr_bank_; }

private:
    std::vector<u8> prg_;
    std::vector<u8> chr_;
    Mirroring mirroring_;
    u8 chr_bank_ = 0;

    // -- save states ---------------------------------------------------------
    //
    // CNROM has one register and no RAM of any kind: the board is a ROM and a
    // latch. The whole of its state is four bits.

public:
    void serialize(StateWriter& out) const override
    {
        out.put_u8(chr_bank_);
    }

    bool deserialize(StateReader& in) override
    {
        in.get_u8(chr_bank_);
        return in.ok();
    }

    [[nodiscard]] bool saves_state() const noexcept override { return true; }

private:
};

} // namespace fc::nes
