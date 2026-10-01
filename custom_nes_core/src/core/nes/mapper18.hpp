#pragma once

// ---------------------------------------------------------------------------
// Mapper 18, the Jaleco SS88006.
//
//     PRG: 4 x 8KB windows; $8000/$A000/$C000 switchable, $E000 fixed last
//     CHR: 8 x 1KB windows
//     Mirroring: runtime
//     + a down counter clocked by the CPU that can raise /IRQ
//
// This is the first board in the project whose IRQ is *not* derived from the
// PPU. MMC3 gets a free clock from A12; here the counter is wired straight to
// the CPU clock, which is why Mapper grew clocks_on_cpu_cycles() and
// on_cpu_cycle() for it.
//
// Each 8KB PRG bank number and each 1KB CHR bank number is spread over two
// registers - an even address holds the low nibble, the odd one the high
// nibble - because the latch on the board only has four data bits wired to
// the ROM. That is what `addr & 0xF003` is decoding:
//
//     $8000/1  PRG bank 0     $A000..$A003  CHR 0,1
//     $8002/3  PRG bank 1     $B000..$B003  CHR 2,3
//     $9000/1  PRG bank 2     $C000..$C003  CHR 4,5
//                             $D000..$D003  CHR 6,7
//     $E000..$E003  IRQ reload value, one nibble each
//     $F000  clear + reload the counter
//     $F001  enable, and pick the counter width (16/12/8/4 bits)
//     $F002  mirroring
//     $F003  expansion audio (not emulated)
// ---------------------------------------------------------------------------

#include "core/nes/mapper.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper18 : public Mapper {
public:
    Mapper18(std::vector<u8> prg, std::vector<u8> chr, Mirroring mirroring)
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
        const std::size_t banks = prg_bank_count();
        const std::size_t slot = static_cast<std::size_t>(address >> 13) & 0x03u;
        const std::size_t bank = (slot < 3u)
                                     ? (static_cast<std::size_t>(prg_bank_[slot]) % banks)
                                     : (banks - 1u);
        return prg_[bank * 0x2000u + static_cast<std::size_t>(address & 0x1FFFu)];
    }

    void write_prg(u16 address, u8 value) override
    {
        const bool upper = (address & 0x0001u) != 0;
        const u8 nibble = static_cast<u8>(value & 0x0Fu);

        switch (address & 0xF003u) {
        case 0x8000: case 0x8001: update_prg(0, nibble, upper); break;
        case 0x8002: case 0x8003: update_prg(1, nibble, upper); break;
        case 0x9000: case 0x9001: update_prg(2, nibble, upper); break;

        case 0xA000: case 0xA001: update_chr(0, nibble, upper); break;
        case 0xA002: case 0xA003: update_chr(1, nibble, upper); break;
        case 0xB000: case 0xB001: update_chr(2, nibble, upper); break;
        case 0xB002: case 0xB003: update_chr(3, nibble, upper); break;
        case 0xC000: case 0xC001: update_chr(4, nibble, upper); break;
        case 0xC002: case 0xC003: update_chr(5, nibble, upper); break;
        case 0xD000: case 0xD001: update_chr(6, nibble, upper); break;
        case 0xD002: case 0xD003: update_chr(7, nibble, upper); break;

        case 0xE000: case 0xE001: case 0xE002: case 0xE003:
            irq_reload_[address & 0x03u] = nibble;
            break;

        case 0xF000:
            irq_pending_ = false;
            irq_counter_ = static_cast<u16>(irq_reload_[0] | (irq_reload_[1] << 4) |
                                            (irq_reload_[2] << 8) | (irq_reload_[3] << 12));
            break;

        case 0xF001:
            irq_pending_ = false;
            irq_enabled_ = (nibble & 0x01u) != 0;
            if ((nibble & 0x08u) != 0) {
                irq_size_ = 3;   // 4-bit counter
            } else if ((nibble & 0x04u) != 0) {
                irq_size_ = 2;   // 8-bit
            } else if ((nibble & 0x02u) != 0) {
                irq_size_ = 1;   // 12-bit
            } else {
                irq_size_ = 0;   // 16-bit
            }
            break;

        case 0xF002:
            switch (nibble & 0x03u) {
            case 0: mirroring_ = Mirroring::Horizontal; break;
            case 1: mirroring_ = Mirroring::Vertical; break;
            case 2: mirroring_ = Mirroring::SingleScreenLower; break;
            default: mirroring_ = Mirroring::SingleScreenUpper; break;
            }
            break;

        default:   // $F003: expansion audio
            break;
        }
    }

    [[nodiscard]] u8 read_chr(u16 address) override
    {
        if (chr_.empty()) {
            return 0;
        }
        const std::size_t banks = chr_.size() / 0x400u;
        const std::size_t slot = static_cast<std::size_t>(address >> 10) & 0x07u;
        const std::size_t bank = static_cast<std::size_t>(chr_bank_[slot]) % ((banks == 0) ? 1 : banks);
        return chr_[bank * 0x400u + static_cast<std::size_t>(address & 0x3FFu)];
    }

    void write_chr(u16 /*address*/, u8 /*value*/) override {}

    [[nodiscard]] Mirroring mirroring() const noexcept override { return mirroring_; }

    // -- the CPU-clocked IRQ counter -----------------------------------------

    [[nodiscard]] bool clocks_on_cpu_cycles() const noexcept override { return true; }

    void on_cpu_cycle() noexcept override
    {
        if (!irq_enabled_) {
            return;
        }
        const u16 mask = kIrqMask[irq_size_];
        u16 counter = static_cast<u16>(irq_counter_ & mask);
        --counter;
        if (counter == 0) {
            irq_pending_ = true;
        }
        irq_counter_ = static_cast<u16>((irq_counter_ & static_cast<u16>(~mask)) |
                                        (counter & mask));
    }

    [[nodiscard]] bool irq_asserted() const noexcept override { return irq_pending_; }

    // -- inspection, for tests ----------------------------------------------

    [[nodiscard]] u8 prg_bank(int index) const noexcept
    {
        return (index >= 0 && index < 3) ? prg_bank_[index] : 0;
    }
    [[nodiscard]] u8 chr_bank(int index) const noexcept
    {
        return (index >= 0 && index < 8) ? chr_bank_[index] : 0;
    }
    [[nodiscard]] u16 irq_counter() const noexcept { return irq_counter_; }

private:
    [[nodiscard]] std::size_t prg_bank_count() const noexcept
    {
        const std::size_t count = prg_.size() / 0x2000u;
        return (count == 0) ? 1 : count;
    }

    void update_prg(int index, u8 nibble, bool upper) noexcept
    {
        if (upper) {
            prg_bank_[index] = static_cast<u8>((prg_bank_[index] & 0x0Fu) | (nibble << 4));
        } else {
            prg_bank_[index] = static_cast<u8>((prg_bank_[index] & 0xF0u) | nibble);
        }
    }

    void update_chr(int index, u8 nibble, bool upper) noexcept
    {
        if (upper) {
            chr_bank_[index] = static_cast<u8>((chr_bank_[index] & 0x0Fu) | (nibble << 4));
        } else {
            chr_bank_[index] = static_cast<u8>((chr_bank_[index] & 0xF0u) | nibble);
        }
    }

    static constexpr u16 kIrqMask[4] = { 0xFFFF, 0x0FFF, 0x00FF, 0x000F };

    std::vector<u8> prg_;
    std::vector<u8> chr_;
    Mirroring mirroring_;

    u8 prg_bank_[3] = { 0, 0, 0 };
    u8 chr_bank_[8] = { 0, 0, 0, 0, 0, 0, 0, 0 };

    u8 irq_reload_[4] = { 0, 0, 0, 0 };
    u16 irq_counter_ = 0;
    u8 irq_size_ = 0;
    bool irq_enabled_ = false;
    bool irq_pending_ = false;
};

} // namespace fc::nes
