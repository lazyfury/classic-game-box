#pragma once

// ---------------------------------------------------------------------------
// Mapper 121, an MMC3 with a small protection latch.
//
// The board is an MMC3 plus a handful of registers of its own, and a game that
// has to be talked to before it will run. What makes it a mapper of its own is
// the protection, and the protection is a state machine:
//
//   $5000-$5FFF  write: a 2-bit index into a table of four bytes, which a read
//                       back at the same range returns
//   $8001        the MMC3's bank data, but the value is kept, bit-reversed, as
//                a number for the state machine
//   $8003        the state machine's command; depending on which of a short
//                list it is, the reversed number becomes a PRG bank for one of
//                the top three windows
//
// Once the machine has been fed a command it recognises, three of the four
// PRG windows stop being the MMC3's and become the board's, and a game that
// never gets that far sees a cartridge that looks broken.
//
// 幽游白书5 (the game this was written for) is a 128KB cartridge, so several
// of these registers do nothing once masked to the size of the ROM -- but they
// still have to be there, because the game writes them and expects them to
// answer.
// ---------------------------------------------------------------------------

#include "core/nes/mapper4.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper121 : public Mapper4 {
public:
    Mapper121(std::vector<u8> prg, std::vector<u8> chr, Mirroring default_mirroring)
        : Mapper4(std::move(prg), std::move(chr), default_mirroring)
    {
        ex_regs_[3] = 0x80;
    }

    // -- the CPU's view, $8000-$FFFF -----------------------------------------

    [[nodiscard]] u8 read_prg(u16 address) override
    {
        // While the protection is armed, three windows are the board's.
        if ((ex_regs_[5] & 0x3Fu) != 0u) {
            const std::size_t high = static_cast<std::size_t>(ex_regs_[3] & 0x80u) >> 2;
            switch (address & 0xE000u) {
            case 0xA000u:
                return read_prg_bank(ex_regs_[2] | high, address);
            case 0xC000u:
                return read_prg_bank(ex_regs_[1] | high, address);
            case 0xE000u:
                return read_prg_bank(ex_regs_[0] | high, address);
            default:
                break;
            }
        }
        return Mapper4::read_prg(address);
    }

    void write_prg(u16 address, u8 value) override
    {
        switch (address & 0xE003u) {
        case 0x8000u:
            Mapper4::write_prg(0x8000, value);
            return;

        case 0x8001u:
            ex_regs_[6] = reverse_six_bits(value);
            if (ex_regs_[7] == 0) {
                update_ex_regs();
            }
            Mapper4::write_prg(0x8001, value);
            return;

        case 0x8003u:
            ex_regs_[5] = value;
            update_ex_regs();
            Mapper4::write_prg(0x8000, value);
            return;

        default:
            Mapper4::write_prg(address, value);
            return;
        }
    }

    // -- the expansion area, $5000-$5FFF -------------------------------------

    [[nodiscard]] u8 read_expansion(u16 address) override
    {
        if (address >= 0x5000u && address <= 0x5FFFu) {
            return ex_regs_[4];
        }
        return 0;
    }

    void write_expansion(u16 address, u8 value) override
    {
        if (address < 0x5000u || address > 0x5FFFu) {
            return;
        }

        // Four bytes, selected by the low two bits of the value.
        static constexpr u8 kProtection[4] = { 0x83, 0x83, 0x42, 0x00 };
        ex_regs_[4] = kProtection[value & 0x03u];

        // The A9713 multicart's extension, which shares the range.
        if ((address & 0x5180u) == 0x5180u) {
            ex_regs_[3] = value;
        }
    }

    // -- save states ---------------------------------------------------------

    void serialize(StateWriter& out) const override
    {
        Mapper4::serialize(out);
        for (const u8 value : ex_regs_) {
            out.put_u8(value);
        }
    }

    bool deserialize(StateReader& in) override
    {
        if (!Mapper4::deserialize(in)) {
            return false;
        }
        for (u8& value : ex_regs_) {
            in.get_u8(value);
        }
        return in.ok();
    }

protected:
    [[nodiscard]] std::size_t map_prg_bank(std::size_t bank) const noexcept override
    {
        return bank | (static_cast<std::size_t>(ex_regs_[3] & 0x80u) >> 2);
    }

    [[nodiscard]] std::size_t map_chr_bank(std::size_t slot,
                                           std::size_t bank) const noexcept override
    {
        // A 3-in-1 board wires PRG and CHR from the same ROM chip, and says so
        // by their sizes matching.
        if (prg_.size() == chr_.size()) {
            return bank | (static_cast<std::size_t>(ex_regs_[3] & 0x80u) << 1);
        }
        if ((slot < 4u && !chr_mode_) || (slot >= 4u && chr_mode_)) {
            return bank | 0x100u;
        }
        return bank;
    }

private:
    [[nodiscard]] static u8 reverse_six_bits(u8 value) noexcept
    {
        return static_cast<u8>(((value & 0x01u) << 5) | ((value & 0x02u) << 3)
                               | ((value & 0x04u) << 1) | ((value & 0x08u) >> 1)
                               | ((value & 0x10u) >> 3) | ((value & 0x20u) >> 5));
    }

    void update_ex_regs() noexcept
    {
        switch (ex_regs_[5] & 0x3Fu) {
        case 0x20: case 0x29: case 0x2B: case 0x3C: case 0x3F:
            ex_regs_[7] = 1;
            ex_regs_[0] = ex_regs_[6];
            break;
        case 0x26:
            ex_regs_[7] = 0;
            ex_regs_[0] = ex_regs_[6];
            break;
        case 0x2C:
            ex_regs_[7] = 1;
            if (ex_regs_[6] != 0u) {
                ex_regs_[0] = ex_regs_[6];
            }
            break;
        case 0x28:
            ex_regs_[7] = 0;
            ex_regs_[1] = ex_regs_[6];
            break;
        case 0x2A:
            ex_regs_[7] = 0;
            ex_regs_[2] = ex_regs_[6];
            break;
        case 0x2F:
            break;
        default:
            ex_regs_[5] = 0;
            break;
        }
    }

    u8 ex_regs_[8] = { 0, 0, 0, 0, 0, 0, 0, 0 };
};

} // namespace fc::nes
