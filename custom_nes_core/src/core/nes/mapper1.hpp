#pragma once

// ---------------------------------------------------------------------------
// Mapper 1, the MMC1.
//
//     PRG ROM:  up to 256KB, banked in 16KB or 32KB windows
//     CHR ROM:  up to 128KB, banked in 4KB or 8KB windows (or 8KB CHR RAM)
//     + a shift register, and switchable nametable mirroring
//
// The MMC1 is the first mapper that is interesting, because the CPU has only
// five wires to talk to it and it has four registers to fill.
//
// One serial port, five bits
// --------------------------
// A write to $8000-$FFFF does not select a bank directly. It shifts ONE bit
// into a five bit shift register:
//
//     write, bit 0 -> shift in, LSB first
//     after five writes the register holds a value
//     which register it lands in is chosen by the ADDRESS, not the value:
//
//         $8000-$9FFF  control     mirroring + PRG mode + CHR size
//         $A000-$BFFF  CHR bank 0
//         $C000-$DFFF  CHR bank 1
//         $E000-$FFFF  PRG bank
//
// Writing a value with bit 7 set resets the shift register instead, which is
// how a program recovers if it loses track. A reset also forces PRG mode 3.
//
// This is why every "write" in an MMC1 game looks like four or five writes in
// a row to the same address: the program is clocking bits in one at a time.
//
// Why four registers is enough
// ----------------------------
// The control register is the clever part. It decides how the other three are
// interpreted:
//
//     bits 0-1  mirroring:  0 one-screen lower, 1 one-screen upper,
//                           2 vertical, 3 horizontal
//     bits 2-3  PRG mode:   0/1  32KB at $8000 (low bank bit ignored)
//                           2    $8000 fixed first, $C000 switchable
//                           3    $8000 switchable, $C000 fixed last
//     bit 4     CHR mode:   0   8KB CHR banks, 1   4KB CHR banks
//
// So the same CHR bank register means different things depending on bit 4,
// and the same PRG bank register means different things depending on bits
// 2-3. Everything is a mode bit, which is exactly how the real board saves
// logic gates.
//
// Zelda II is the reason this file exists.
// ---------------------------------------------------------------------------

#include "core/nes/mapper.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper1 : public Mapper {
public:
    Mapper1(std::vector<u8> prg, std::vector<u8> chr)
        : prg_(std::move(prg))
        , chr_(std::move(chr))
    {
    }

    /// Turn this into 8KB of CHR RAM. Used when the header says zero CHR
    /// pages, which is common for MMC1 boards.
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
        return prg_[prg_offset(address) % prg_.size()];
    }

    void write_prg(u16 address, u8 value) override
    {
        // Bit 7 is not a data bit: it is the reset line for the shift
        // register. A program that has lost count sets it to start over.
        if ((value & 0x80u) != 0) {
            shift_ = 0x10u;
            control_ = static_cast<u8>(control_ | 0x0Cu);   // force PRG mode 3
            return;
        }

        // `complete` is sampled before the shift: after four writes the
        // sentinel bit in the register has reached bit 0, and the fifth write
        // is the one that completes the value.
        const bool complete = (shift_ & 0x01u) != 0;
        shift_ = static_cast<u8>((shift_ >> 1) | ((value & 0x01u) << 4));

        if (!complete) {
            return;
        }

        const u8 data = static_cast<u8>(shift_ & 0x1Fu);

        // The ADDRESS picks the register. The value was just bits arriving.
        switch ((address >> 13) & 0x03u) {
        case 0: control_ = data; break;
        case 1: chr_bank0_ = data; break;
        case 2: chr_bank1_ = data; break;
        default: prg_bank_ = data; break;
        }

        shift_ = 0x10u;
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

    // -- the cartridge decides how the nametables are wired ------------------

    [[nodiscard]] Mirroring mirroring() const noexcept override
    {
        switch (control_ & 0x03u) {
        case 0: return Mirroring::SingleScreenLower;
        case 1: return Mirroring::SingleScreenUpper;
        case 2: return Mirroring::Vertical;
        default: return Mirroring::Horizontal;
        }
    }

    // -- inspection, for tests and tools -------------------------------------

    [[nodiscard]] u8 control() const noexcept { return control_; }
    [[nodiscard]] u8 chr_bank0() const noexcept { return chr_bank0_; }
    [[nodiscard]] u8 chr_bank1() const noexcept { return chr_bank1_; }
    [[nodiscard]] u8 prg_bank() const noexcept { return prg_bank_; }
    [[nodiscard]] u8 shift_register() const noexcept { return shift_; }

private:
    [[nodiscard]] std::size_t prg_bank_count() const noexcept
    {
        const std::size_t count = prg_.size() / 0x4000u;
        return (count == 0) ? 1 : count;
    }

    /// Which 16KB PRG bank answers `address`. The control register's PRG mode
    /// is what makes this a four-way decision instead of a simple lookup.
    [[nodiscard]] std::size_t prg_offset(u16 address) const noexcept
    {
        const std::size_t banks = prg_bank_count();
        const std::size_t mode = (control_ >> 2) & 0x03u;
        const std::size_t high_half = (address >= 0xC000u) ? 1u : 0u;

        std::size_t bank = 0;
        if (mode == 0 || mode == 1) {
            // One 32KB bank at $8000. Bit 0 of the bank number is ignored:
            // there is no 16KB wire to carry it.
            bank = static_cast<std::size_t>((prg_bank_ & 0x1Eu)) + high_half;
        } else if (mode == 2) {
            // Fixed first bank low, switchable high.
            bank = (high_half == 0) ? 0u : (static_cast<std::size_t>(prg_bank_));
        } else {
            // Switchable low, fixed last bank high.
            bank = (high_half == 0) ? static_cast<std::size_t>(prg_bank_)
                                    : (banks - 1u);
        }

        // A bank number larger than the ROM simply wraps; the high address
        // lines are not connected. This also keeps a bad value from reading
        // past the end of the vector.
        bank %= banks;
        return bank * 0x4000u + static_cast<std::size_t>(address & 0x3FFFu);
    }

    [[nodiscard]] std::size_t chr_offset(u16 address) const noexcept
    {
        if (chr_.empty()) {
            return 0;
        }

        if ((control_ & 0x10u) != 0) {
            // 4KB mode: the two registers select the two halves separately.
            const std::size_t bank = (address < 0x1000u) ? chr_bank0_ : chr_bank1_;
            const std::size_t count = chr_.size() / 0x1000u;
            return ((count == 0 ? 0u : bank % count) * 0x1000u)
                 + static_cast<std::size_t>(address & 0x0FFFu);
        }

        // 8KB mode: the bank number is CHR bank 0 shifted right by one, and
        // CHR bank 1 is not connected at all. Bit 0 of CHR bank 0 selects the
        // 4KB half inside the bank and is ignored because the PPU's A12 does
        // that job here.
        //
        // Getting this wrong is subtle: the register value is already the
        // bank number doubled, so a game writing $02 wants bank 1 and a game
        // writing $10 wants bank 8. Treating the value as the bank number
        // directly puts every tile two banks away from where it belongs,
        // which is exactly what Zelda II was showing.
        const std::size_t bank = static_cast<std::size_t>(chr_bank0_ >> 1);
        const std::size_t count = chr_.size() / 0x2000u;
        return ((count == 0 ? 0u : bank % count) * 0x2000u)
             + static_cast<std::size_t>(address & 0x1FFFu);
    }

    std::vector<u8> prg_;
    std::vector<u8> chr_;
    bool chr_ram_ = false;

    // Power-on state. The shift register's $10 is a sentinel: when it reaches
    // bit 0, five bits have arrived. Control resets to PRG mode 3.
    u8 shift_ = 0x10u;
    u8 control_ = 0x0Cu;
    u8 chr_bank0_ = 0;
    u8 chr_bank1_ = 0;
    u8 prg_bank_ = 0;

    // -- save states ---------------------------------------------------------
    //
    // MMC1 is the mapper where the shift register itself is state. A write
    // lands in `shift_` and is only committed to a real register on the fifth
    // write, so a save taken mid serial transfer has to remember how far the
    // transfer had got. Losing that is a bank switch that silently does not
    // happen, several frames later.

public:
    void serialize(StateWriter& out) const override
    {
        out.put_u8(shift_);
        out.put_u8(control_);
        out.put_u8(chr_bank0_);
        out.put_u8(chr_bank1_);
        out.put_u8(prg_bank_);
        out.put_flag(chr_ram_);
        if (chr_ram_) {
            out.sized_bytes(chr_);
        }
    }

    bool deserialize(StateReader& in) override
    {
        in.get_u8(shift_);
        in.get_u8(control_);
        in.get_u8(chr_bank0_);
        in.get_u8(chr_bank1_);
        in.get_u8(prg_bank_);
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
