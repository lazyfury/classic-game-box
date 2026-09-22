#pragma once

// ---------------------------------------------------------------------------
// Mapper 163, the Nanjing FC-001 board.
//
//     PRG ROM:  32KB switchable bank covering the whole $8000-$FFFF window
//     PRG RAM:  8KB at $6000, battery backed
//     CHR:      8KB CHR RAM, whose top half can be switched by the PPU
//     Mirroring: hard wired on the board
//
// This board was used by almost every game from Nanjing (南晶). It is a
// Chinese unlicensed chip, and it has two things a licensed board does not:
// a 32KB window rather than 16KB halves, and a small "feedback" register
// that exists only so a cartridge can prove it is not a copy.
//
// The bank number
// ---------------
// The register is split over three addresses, and one bit of the number is
// deliberately not where you would expect it:
//
//     $5000  C... PPPP   bits 0-3 = PRG A15-A18 (bank bits 0-3)
//                        bit 7    = automatic 4KB CHR-RAM switch
//     $5200  .... ..PP   bits 0-1 = PRG A19-A20 (bank bits 4-5)
//     $5300  .... .A?B   bit 0    = swap D0/D1 on writes to $5000-$5200
//                        bit 2    = 0: force PRG A15/A16 = 11
//                                   1: take A15/A16 from $5000
//
// On reset every register is zero, so A15/A16 are forced to 11 and the CPU
// starts in 32KB bank 3. That is not a detail: the reset vector of a Nanjing
// cartridge is written on the assumption that bank 3 is the boot bank, and
// a game that changed it before the first instruction would read a
// different reset vector and die.
//
// The copy protection
// -------------------
// $5100 and $5101 are a two-bit shift register that a game can poke and
// read back. The board presents it as a security check: the game latches a
// value, then writes a strobe to $5101 and reads the inverted `F` bit back
// from $5500. A copied cartridge with a plain ROM chip cannot produce the
// inverted bit, so the game knows. This is why the register is emulated in
// hardware rather than by patching the ROM.
//
//     D2  F    the bit that gets read back (inverted)
//     D0  E    the strobe that flips F on a 1 -> 0 edge
//
// The automatic CHR switch
// ------------------------
// With $5000 bit 7 set the board ignores the PPU's own A12 and drives CHR
// A12 from PPU A9, latched when A13 rises. In plain terms: the top half of
// the nametable uses the left 4KB pattern table and the bottom half uses
// the right one, no matter where the screen is scrolled. It is a "3D wall"
// trick. The real chip watches the address bus per tile; this emulator
// only has the beam position, so the switch is made on scanlines 127 and
// 239, which is the same approximation Nestopia and Mesen use.
// ---------------------------------------------------------------------------

#include "core/nes/mapper.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper163 : public Mapper {
public:
    Mapper163(std::vector<u8> prg, std::vector<u8> chr, Mirroring mirroring)
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

    // -- the CPU's view, $8000-$FFFF -----------------------------------------

    [[nodiscard]] u8 read_prg(u16 address) override
    {
        if (prg_.empty()) {
            return 0;
        }
        const std::size_t banks = prg_bank_count();
        const std::size_t bank = static_cast<std::size_t>(prg_bank_) % banks;
        return prg_[bank * 0x8000u + static_cast<std::size_t>(address & 0x7FFFu)];
    }

    void write_prg(u16 /*address*/, u8 /*value*/) override
    {
        // Nothing is written to $8000-$FFFF on this board: every register
        // lives down in the expansion area, at $5000.
    }

    // -- the expansion area, $5000-$5FFF -------------------------------------

    void write_expansion(u16 address, u8 value) override
    {
        if (address < 0x5000u || address > 0x5FFFu) {
            return;
        }

        // The eight address lines that matter are A8 and A9 (the register
        // number, so the registers mirror every $400) and A0 (which half of
        // the feedback port). Everything else is not decoded.
        const u8 index = static_cast<u8>((address >> 8) & 0x03u);

        // The mode register's bit 0 swaps data bits 0 and 1 on every write
        // to $5000-$5200. The mode register itself is exempt: a swap cannot
        // depend on the value it is about to be told.
        if ((mode_ & 0x01u) != 0 && index <= 2u) {
            value = swap_bits_0_and_1(value);
        }

        if (index == 1u) {
            write_feedback(address, value);
            return;
        }

        switch (index) {
        case 0: prg_low_ = value; break;
        case 2: prg_high_ = value; break;
        default: mode_ = value; break;   // index 3, $5300
        }
        update_banks();
    }

    [[nodiscard]] u8 read_expansion(u16 address) override
    {
        if (address < 0x5000u || address > 0x5FFFu) {
            return 0;
        }
        // The wiki defines the feedback read at $5500, but the FCEUX
        // implementation answers it across the whole window, and that is
        // what the games were tested against. Either way the only bit a
        // game looks at is D2, the inverted F bit.
        return static_cast<u8>((feedback_ ^ 1u) << 2u);
    }

    // -- the PPU's view, $0000-$1FFF -----------------------------------------

    [[nodiscard]] u8 read_chr(u16 address) override
    {
        if (chr_.empty()) {
            return 0;
        }
        return chr_[chr_offset(address) % chr_.size()];
    }

    void write_chr(u16 address, u8 value) override
    {
        if (chr_ram_) {
            chr_[chr_offset(address) % chr_.size()] = value;
        }
    }

    [[nodiscard]] Mirroring mirroring() const noexcept override { return mirroring_; }

    /// The board's automatic CHR switch, which the real chip ties to PPU
    /// A13/A9. The closest thing this PPU exposes is the scanline, so the
    /// switch happens where the picture crosses the middle.
    void on_scanline(int scanline) noexcept override
    {
        if (!auto_switch_) {
            return;
        }
        if (scanline == 127) {
            auto_page_ = 1u;
        } else if (scanline == 239) {
            auto_page_ = 0u;
        }
    }

    // -- inspection, for tests ----------------------------------------------

    [[nodiscard]] u8 prg_bank() const noexcept { return prg_bank_; }
    [[nodiscard]] u8 mode() const noexcept { return mode_; }
    [[nodiscard]] bool auto_switch() const noexcept { return auto_switch_; }
    [[nodiscard]] u8 feedback_bit() const noexcept { return feedback_; }

private:
    [[nodiscard]] std::size_t prg_bank_count() const noexcept
    {
        const std::size_t count = prg_.size() / 0x8000u;
        return (count == 0) ? 1 : count;
    }

    [[nodiscard]] static u8 swap_bits_0_and_1(u8 value) noexcept
    {
        return static_cast<u8>((value & 0xFCu) |
                               ((value >> 1) & 0x01u) |
                               ((value << 1) & 0x02u));
    }

    /// $5100/$5101: latch the feedback bit, and flip it on a falling edge
    /// of E. D2 is F, D0 is E.
    void write_feedback(u16 address, u8 value) noexcept
    {
        const bool e = (value & 0x01u) != 0;
        const bool f = (value & 0x04u) != 0;

        if ((address & 0x0001u) != 0) {
            // $5101: the strobe. E is latched so the edge can be seen; F is
            // ignored and only flips.
            if (e_ && !e) {
                feedback_ ^= 1u;
            }
            e_ = e;
            return;
        }

        // $5100: latch both bits. Value 6 is a quirk two other emulators
        // special case - it jumps to bank 3 - so it is kept here too.
        e_ = e;
        feedback_ = f ? 1u : 0u;
        if (value == 0x06u) {
            prg_bank_ = 3u;
        }
    }

    /// Recompute the 32KB bank from the three registers.
    void update_banks() noexcept
    {
        auto_switch_ = (prg_low_ & 0x80u) != 0;

        u8 bank = static_cast<u8>((prg_low_ & 0x0Fu) | ((prg_high_ & 0x03u) << 4));

        // Mode bit 2 clear forces A15/A16 to 11, which is why power-on lands
        // in bank 3 rather than bank 0.
        if ((mode_ & 0x04u) == 0) {
            bank = static_cast<u8>((bank & 0xFCu) | 0x03u);
        }
        prg_bank_ = bank;
    }

    /// When the automatic switch is on, both 4KB pattern tables are read
    /// out of the same half of the CHR RAM.
    [[nodiscard]] std::size_t chr_offset(u16 address) const noexcept
    {
        if (!auto_switch_) {
            return static_cast<std::size_t>(address & 0x1FFFu);
        }
        return static_cast<std::size_t>((address & 0x0FFFu) | (auto_page_ << 12));
    }

    std::vector<u8> prg_;
    std::vector<u8> chr_;
    Mirroring mirroring_;
    bool chr_ram_ = false;

    u8 prg_low_ = 0;
    u8 prg_high_ = 0;
    u8 mode_ = 0;
    u8 prg_bank_ = 3;       // the reset state: A15/A16 forced high
    bool auto_switch_ = false;
    u8 auto_page_ = 0;

    u8 feedback_ = 0;
    bool e_ = false;

    // -- save states ---------------------------------------------------------
    //
    // The Nanjing board is unusual in that its state includes things a save
    // would not normally have to think about: the anti-piracy feedback latch
    // at $5100/$5101, which the game polls and which therefore changes what
    // happens next; and `auto_page_`, the 4KB window the automatic CHR
    // switch is currently on, which the mapper keeps rather than recomputing.

public:
    void serialize(StateWriter& out) const override
    {
        out.put_u8(prg_low_);
        out.put_u8(prg_high_);
        out.put_u8(mode_);
        out.put_u8(prg_bank_);
        out.put_flag(auto_switch_);
        out.put_u8(auto_page_);
        out.put_u8(feedback_);
        out.put_flag(e_);
        out.put_u8(static_cast<u8>(mirroring_));
        out.put_flag(chr_ram_);
        if (chr_ram_) {
            out.sized_bytes(chr_);
        }
    }

    bool deserialize(StateReader& in) override
    {
        in.get_u8(prg_low_);
        in.get_u8(prg_high_);
        in.get_u8(mode_);
        in.get_u8(prg_bank_);
        in.get_flag(auto_switch_);
        in.get_u8(auto_page_);
        in.get_u8(feedback_);
        in.get_flag(e_);

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
