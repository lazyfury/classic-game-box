#pragma once

// ---------------------------------------------------------------------------
// Mappers 21 / 22 / 23 / 25, the Konami VRC2 and VRC4.
//
//     PRG: 8KB windows; two switchable, two pinned
//     CHR: 8 x 1KB windows, each split over a low and a high register
//     Mirroring: runtime
//     + IRQ on VRC4 only
//
// The four mapper numbers are one chip family, and the numbers do not mean
// what they look like: 22 is VRC2, but 21, 23 and 25 are VRC4 in different
// packages. What changed between packages is which *address line* carries
// each of the two register-select bits. The data layout stayed identical.
//
// The chips:
//
//     VRC2a (22)   CHR bank numbers are shifted right one (the low bit is
//                  not connected), and there is a serial EEPROM port.
//     VRC2b (23)   plain VRC2.
//     VRC4a (21)   VRC2 plus an IRQ and a second PRG layout mode.
//     VRC4b (25)   different address wiring again.
//
// With only an iNES header there is no sub-mapper byte, so the emulator uses
// the "OR both wirings" heuristic that Nestopia, FCEUX and Mesen all use:
//
//     A0 = bit1 OR bit6,  A1 = bit2 OR bit7     (mapper 21)
//     A0 = bit1 OR bit3,  A1 = bit0 OR bit2     (mapper 25)
//     A0 = bit0 OR bit2,  A1 = bit1 OR bit3     (mapper 23)
//
// The IRQ counter is CPU-clocked, with a /341 prescaler, which is why the
// Mapper interface has on_cpu_cycle().
// ---------------------------------------------------------------------------

#include "core/nes/mapper.hpp"

#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper21 : public Mapper {
public:
    enum class Variant {
        Vrc2a,   // 22
        Vrc2b,   // 23
        Vrc2c,   // 25
        Vrc4a,   // 21
        Vrc4b,   // 25
        Vrc4c,   // 21
        Vrc4d,   // 25
        Vrc4e,   // 23
    };

    Mapper21(std::vector<u8> prg, std::vector<u8> chr, Mirroring mirroring, int mapper_number)
        : prg_(std::move(prg))
        , chr_(std::move(chr))
        , mirroring_(mirroring)
    {
        switch (mapper_number) {
        case 22: variant_ = Variant::Vrc2a; break;
        case 23: variant_ = Variant::Vrc2b; break;
        case 25: variant_ = Variant::Vrc4b; break;
        case 21:
        default: variant_ = Variant::Vrc4a; break;
        }
        heuristics_ = (mapper_number != 22);
    }

    [[nodiscard]] bool has_work_ram() const noexcept override
    {
        // VRC2a is the one that brought a serial EEPROM port instead of a
        // plain RAM chip, so it answers $6000-$6FFF itself.
        return variant_ != Variant::Vrc2a;
    }

    [[nodiscard]] u8 read_prg(u16 address) override
    {
        if (prg_.empty()) {
            return 0;
        }
        const std::size_t banks = prg_bank_count();
        const std::size_t slot = static_cast<std::size_t>(address >> 13) & 0x03u;
        const std::size_t bank = prg_slot(slot, banks) % banks;
        return prg_[bank * 0x2000u + static_cast<std::size_t>(address & 0x1FFFu)];
    }

    void write_prg(u16 address, u8 value) override
    {
        if (address < 0x8000u) {
            return;
        }

        const u16 a = static_cast<u16>(translate(address) & 0xF00Fu);
        const bool vrc4 = is_vrc4();

        if (a >= 0x8000u && a <= 0x8006u) {
            prg_reg_[0] = static_cast<u8>(value & 0x1Fu);
        } else if (vrc4 && a >= 0x9002u && a <= 0x9003u) {
            prg_mode_ = static_cast<u8>((value >> 1) & 0x01u);
        } else if (a >= 0x9000u && a <= 0x9003u) {
            const u8 mask = (heuristics_ || vrc4) ? 0x03u : 0x01u;
            switch (value & mask) {
            case 0: mirroring_ = Mirroring::Vertical; break;
            case 1: mirroring_ = Mirroring::Horizontal; break;
            case 2: mirroring_ = Mirroring::SingleScreenLower; break;
            default: mirroring_ = Mirroring::SingleScreenUpper; break;
            }
        } else if (a >= 0xA000u && a <= 0xA006u) {
            prg_reg_[1] = static_cast<u8>(value & 0x1Fu);
        } else if (a >= 0xB000u && a <= 0xE006u) {
            const std::size_t reg = static_cast<std::size_t>(((((a >> 12) & 0x07u) - 3u) << 1) |
                                                             ((a >> 1) & 0x01u));
            if ((a & 0x0001u) == 0) {
                chr_lo_[reg] = static_cast<u8>(value & 0x0Fu);
            } else {
                chr_hi_[reg] = static_cast<u8>(value & 0x1Fu);
            }
        } else if (a == 0xF000u) {
            irq_reload_ = static_cast<u8>((irq_reload_ & 0xF0u) | (value & 0x0Fu));
        } else if (a == 0xF001u) {
            irq_reload_ = static_cast<u8>((irq_reload_ & 0x0Fu) | ((value & 0x0Fu) << 4));
        } else if (a == 0xF002u) {
            irq_enabled_after_ack_ = (value & 0x01u) != 0;
            irq_enabled_ = (value & 0x02u) != 0;
            irq_cycle_mode_ = (value & 0x04u) != 0;
            if (irq_enabled_) {
                irq_counter_ = irq_reload_;
                irq_prescaler_ = 341;
            }
            irq_pending_ = false;
        } else if (a == 0xF003u) {
            irq_enabled_ = irq_enabled_after_ack_;
            irq_pending_ = false;
        }
    }

    [[nodiscard]] u8 read_chr(u16 address) override
    {
        if (chr_.empty()) {
            return 0;
        }
        const std::size_t banks = chr_.size() / 0x400u;
        const std::size_t slot = static_cast<std::size_t>(address >> 10) & 0x07u;
        std::size_t page = static_cast<std::size_t>(chr_lo_[slot]) |
                           (static_cast<std::size_t>(chr_hi_[slot]) << 4);
        if (variant_ == Variant::Vrc2a) {
            page >>= 1;   // the low bit is not connected on this package
        }
        page %= (banks == 0) ? 1 : banks;
        return chr_[page * 0x400u + static_cast<std::size_t>(address & 0x3FFu)];
    }

    void write_chr(u16 /*address*/, u8 /*value*/) override {}

    [[nodiscard]] Mirroring mirroring() const noexcept override { return mirroring_; }

    // -- the serial EEPROM port ($6000-$6FFF), VRC2a only ---------------------

    void write_expansion(u16 address, u8 value) override
    {
        if (variant_ == Variant::Vrc2a && address >= 0x6000u && address <= 0x6FFFu) {
            eeprom_latch_ = static_cast<u8>(value & 0x01u);
        }
    }

    [[nodiscard]] u8 read_expansion(u16 address) override
    {
        if (variant_ == Variant::Vrc2a && address >= 0x6000u && address <= 0x6FFFu) {
            return eeprom_latch_;
        }
        return 0;
    }

    // -- the CPU-clocked IRQ counter -----------------------------------------

    [[nodiscard]] bool clocks_on_cpu_cycles() const noexcept override { return is_vrc4(); }

    void on_cpu_cycle() noexcept override
    {
        if (!irq_enabled_) {
            return;
        }
        irq_prescaler_ -= 3;
        if (irq_cycle_mode_ || irq_prescaler_ <= 0) {
            if (irq_counter_ == 0xFFu) {
                irq_counter_ = irq_reload_;
                irq_pending_ = true;
            } else {
                ++irq_counter_;
            }
            irq_prescaler_ += 341;
        }
    }

    [[nodiscard]] bool irq_asserted() const noexcept override { return irq_pending_; }

    // -- inspection, for tests ----------------------------------------------

    [[nodiscard]] u8 prg_register(int index) const noexcept
    {
        return (index >= 0 && index < 2) ? prg_reg_[index] : 0;
    }
    [[nodiscard]] u8 prg_mode() const noexcept { return prg_mode_; }

private:
    [[nodiscard]] bool is_vrc4() const noexcept
    {
        return variant_ == Variant::Vrc4a || variant_ == Variant::Vrc4b ||
               variant_ == Variant::Vrc4c || variant_ == Variant::Vrc4d ||
               variant_ == Variant::Vrc4e;
    }

    [[nodiscard]] std::size_t prg_bank_count() const noexcept
    {
        const std::size_t count = prg_.size() / 0x2000u;
        return (count == 0) ? 1 : count;
    }

    [[nodiscard]] std::size_t prg_slot(std::size_t slot, std::size_t banks) const noexcept
    {
        const std::size_t second_last = (banks >= 2u) ? (banks - 2u) : 0u;
        const std::size_t last = banks - 1u;
        if (prg_mode_ == 0) {
            switch (slot) {
            case 0: return prg_reg_[0];
            case 1: return prg_reg_[1];
            case 2: return second_last;
            default: return last;
            }
        }
        switch (slot) {
        case 0: return second_last;
        case 1: return prg_reg_[1];
        case 2: return prg_reg_[0];
        default: return last;
        }
    }

    /// Turn the package's A0/A1 wiring into the canonical two select bits.
    [[nodiscard]] u16 translate(u16 addr) const noexcept
    {
        unsigned a0 = 0;
        unsigned a1 = 0;

        if (heuristics_) {
            switch (variant_) {
            case Variant::Vrc2c:
            case Variant::Vrc4b:
            case Variant::Vrc4d:
                a0 = ((addr >> 1) & 1u) | ((addr >> 3) & 1u);
                a1 = (addr & 1u) | ((addr >> 2) & 1u);
                break;
            case Variant::Vrc4a:
            case Variant::Vrc4c:
                a0 = ((addr >> 1) & 1u) | ((addr >> 6) & 1u);
                a1 = ((addr >> 2) & 1u) | ((addr >> 7) & 1u);
                break;
            default:   // VRC2b / VRC4e
                a0 = (addr & 1u) | ((addr >> 2) & 1u);
                a1 = ((addr >> 1) & 1u) | ((addr >> 3) & 1u);
                break;
            }
        } else {
            // VRC2a: A0 is bit 1, A1 is bit 0.
            a0 = (addr >> 1) & 1u;
            a1 = addr & 1u;
        }

        return static_cast<u16>((addr & 0xFF00u) | ((a1 & 1u) << 1) | (a0 & 1u));
    }

    std::vector<u8> prg_;
    std::vector<u8> chr_;
    Mirroring mirroring_;
    Variant variant_ = Variant::Vrc4a;
    bool heuristics_ = true;

    u8 prg_reg_[2] = { 0, 0 };
    u8 prg_mode_ = 0;
    u8 chr_lo_[8] = { 0, 0, 0, 0, 0, 0, 0, 0 };
    u8 chr_hi_[8] = { 0, 0, 0, 0, 0, 0, 0, 0 };
    u8 eeprom_latch_ = 0;

    u8 irq_reload_ = 0;
    u8 irq_counter_ = 0;
    int irq_prescaler_ = 0;
    bool irq_enabled_ = false;
    bool irq_enabled_after_ack_ = false;
    bool irq_cycle_mode_ = false;
    bool irq_pending_ = false;
};

} // namespace fc::nes
