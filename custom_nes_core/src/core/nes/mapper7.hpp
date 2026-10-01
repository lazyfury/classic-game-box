#pragma once

// ---------------------------------------------------------------------------
// Mapper 7, the AxROM.
//
//     PRG ROM:  32KB banks, switched by a write to $8000-$FFFF
//     CHR:      8KB CHR RAM
//     + one bit of the register also chooses the nametable
//
// AxROM's trick is that it switches a whole 32KB at a time. That is a lot of
// code per bank switch, but it also means the game never has to think about
// where its code lives: the whole address space is one bank.
//
// The second trick is the mirroring. The register's bit 4 picks single-screen
// lower or single-screen upper, so the game can point the PPU's nametables at
// either half of the 2KB VRAM. Battletoads uses it to scroll the status bar;
// most games use it to get a second nametable for free.
//
// Marble Madness and (the US) Gauntlet are AxROM.
// ---------------------------------------------------------------------------

#include "core/nes/mapper.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper7 : public Mapper {
public:
    Mapper7(std::vector<u8> prg, std::vector<u8> chr)
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
        const std::size_t bank = static_cast<std::size_t>(bank_ & 0x07u) % banks;
        return prg_[bank * 0x8000u + static_cast<std::size_t>(address - 0x8000u)];
    }

    void write_prg(u16 /*address*/, u8 value) override
    {
        bank_ = static_cast<u8>(value & 0x07u);
        // Bit 4 is the nametable select, not a bank bit.
        mirroring_ = ((value & 0x10u) != 0) ? Mirroring::SingleScreenUpper
                                            : Mirroring::SingleScreenLower;
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
        const std::size_t count = prg_.size() / 0x8000u;
        return (count == 0) ? 1 : count;
    }

    std::vector<u8> prg_;
    std::vector<u8> chr_;
    Mirroring mirroring_ = Mirroring::SingleScreenLower;
    bool chr_ram_ = false;
    u8 bank_ = 0;

    // -- save states ---------------------------------------------------------
    //
    // AxROM is one register, and it selects both the 32KB bank and the single
    // screen mirroring. Forgetting it comes back on the wrong screen of the
    // wrong level.

public:
    void serialize(StateWriter& out) const override
    {
        out.put_u8(bank_);
        out.put_u8(static_cast<u8>(mirroring_));
        out.put_flag(chr_ram_);
        if (chr_ram_) {
            out.sized_bytes(chr_);
        }
    }

    bool deserialize(StateReader& in) override
    {
        in.get_u8(bank_);

        u8 mirroring = 0;
        in.get_u8(mirroring);
        mirroring_ = static_cast<Mirroring>(mirroring);

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
