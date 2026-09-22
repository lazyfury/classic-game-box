#pragma once

// ---------------------------------------------------------------------------
// Mapper 226, the "76-in-1" multicart.
//
//     PRG ROM:  either a 32KB window (mode 0) or two identical 16KB halves
//               (mode 1), chosen by one register bit
//     CHR:      8KB CHR RAM, no banking
//     Mirroring: one bit of the same register
//
// This is a pirate multicart board. It is the same story as mapper 15 (the
// 100-in-1): the menu and the games are all packed into one ROM chip, and a
// tiny latch chooses which part of the chip the CPU sees. What is different
// here is the shape of the window:
//
//     $8000  [PMOP PPPP]   bit 0-4  low 5 bits of the PRG bank
//                          bit 5    PRG mode (0 = 32KB, 1 = 16KB)
//                          bit 6    mirroring (0 = horizontal, 1 = vertical)
//                          bit 7    the sixth PRG bank bit
//     $8001  [.... ...H]   bit 0    the seventh PRG bank bit
//
// So the bank number is seven bits wide, which is 128 x 16KB = 2MB: exactly
// the size of "76合1.NES". The chip has only sixteen address pins, so the
// board splits the number across two registers - one bit has to live
// somewhere else because bit 6 of $8000 is already the mirroring bit.
//
// The two modes are worth reading twice:
//
//     mode 0   $8000-$FFFF is ONE 32KB bank, number `reg & 0xFE`
//     mode 1   $8000-$BFFF and $C000-$FFFF both show 16KB bank `reg`
//
// In mode 0 the low bit of the bank number is dropped, because a 32KB bank
// starts on an even 16KB boundary. In mode 1 the same 16KB appears twice, so
// a small program can run anywhere in the address space without the menu
// having to relocate it.
//
// The 1.5MB carts ("Super 42-in-1") wire the top two bank bits through a
// different table - { 0, 0, 1, 2 } - which is why the two branches below do
// not agree with each other. It is a wiring quirk, not a second mapper.
// ---------------------------------------------------------------------------

#include "core/nes/mapper.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper226 : public Mapper {
public:
    Mapper226(std::vector<u8> prg, std::vector<u8> chr)
        : prg_(std::move(prg))
        , chr_(std::move(chr))
    {
        // 1536KB carts number their 16KB banks in a scrambled order.
        reorder_banks_ = (prg_.size() == 1536u * 1024u);
    }

    void make_chr_ram(std::size_t size = 8192)
    {
        chr_.assign(size, 0);
        chr_ram_ = true;
    }

    // -- the CPU's view, $8000-$FFFF -----------------------------------------

    [[nodiscard]] u8 read_prg(u16 address) override
    {
        if (prg_.empty()) {
            return 0;
        }
        const std::size_t banks = prg_bank_count();
        const std::size_t page = (address < 0xC000u) ? page_lo_ : page_hi_;
        return prg_[(page % banks) * 0x4000u + static_cast<std::size_t>(address & 0x3FFFu)];
    }

    void write_prg(u16 address, u8 value) override
    {
        // $8000 and $8001 are two halves of one register: address bit 0
        // picks which half. Every other address line is ignored.
        if ((address & 0x0001u) == 0) {
            reg_[0] = value;
        } else {
            reg_[1] = value;
        }
        update_banks();
    }

    // -- the PPU's view, $0000-$1FFF -----------------------------------------

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

    // -- inspection, for tests ----------------------------------------------

    [[nodiscard]] u8 prg_page() const noexcept { return prg_page_; }
    [[nodiscard]] u8 page_low() const noexcept { return page_lo_; }
    [[nodiscard]] u8 page_high() const noexcept { return page_hi_; }

private:
    [[nodiscard]] std::size_t prg_bank_count() const noexcept
    {
        const std::size_t count = prg_.size() / 0x4000u;
        return (count == 0) ? 1 : count;
    }

    /// Recompute what $8000-$FFFF points at, and how the nametables are wired.
    ///
    /// The seven bit bank number is assembled from three places, which is
    /// the whole reason this function exists:
    ///
    ///     bit 0-4   $8000 bits 0-4
    ///     bit 5     $8000 bit 7   (shifted down two places)
    ///     bit 6     $8001 bit 0
    void update_banks() noexcept
    {
        u8 base = static_cast<u8>(((reg_[0] & 0x80u) >> 7) | ((reg_[1] & 0x01u) << 1));

        if (reorder_banks_) {
            base = kReorder1536[base & 0x03u];
        }

        const u8 bank = static_cast<u8>((base << 5) | (reg_[0] & 0x1Fu));
        prg_page_ = bank;

        if ((reg_[0] & 0x20u) != 0) {
            // Mode 1: the same 16KB bank answers in both halves.
            page_lo_ = bank;
            page_hi_ = bank;
        } else {
            // Mode 0: one 32KB bank, so the low bit of the number is not
            // connected - it is dropped rather than masked off.
            page_lo_ = static_cast<u8>(bank & 0xFEu);
            page_hi_ = static_cast<u8>(page_lo_ + 1u);
        }

        mirroring_ = ((reg_[0] & 0x40u) != 0) ? Mirroring::Vertical
                                              : Mirroring::Horizontal;
    }

    /// The 1.5MB bank order: raw bits { 0,1,2,3 } mean banks { 0,0,1,2 }.
    static constexpr u8 kReorder1536[4] = { 0, 0, 1, 2 };

    std::vector<u8> prg_;
    std::vector<u8> chr_;
    bool chr_ram_ = false;
    bool reorder_banks_ = false;

    u8 reg_[2] = { 0, 0 };
    u8 prg_page_ = 0;
    u8 page_lo_ = 0;
    u8 page_hi_ = 1;
    Mirroring mirroring_ = Mirroring::Horizontal;
};

} // namespace fc::nes
