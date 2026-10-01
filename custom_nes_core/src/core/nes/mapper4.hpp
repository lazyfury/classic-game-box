#pragma once

// ---------------------------------------------------------------------------
// Mapper 4, the MMC3.
//
//     PRG ROM:  8KB banks ($8000, $A000, $C000 switchable, $E000 fixed)
//     CHR ROM:  1KB or 2KB banks, with a mode bit that rearranges them
//     + a scanline counter that can interrupt the CPU
//     + switchable horizontal / vertical mirroring
//
// This is the mapper that made the second half of the NES library possible,
// and it is where a console stops being a toy. Everything before it is about
// fitting more data on the cart. The MMC3 adds a piece of state that the
// PPU drives: a counter that ticks once per scanline and fires an IRQ when it
// runs out.
//
// Why a scanline IRQ matters
// --------------------------
// The NES PPU has no way to say "you are at scanline 100". A program that
// wants to change the scroll mid-screen - a status bar that does not move
// while the level scrolls under it, a split-screen for a boss, a wavy
// background - has only one tool: count cycles and hope. The MMC3 turns that
// into an event. The game says "interrupt me in 32 scanlines", and the cart
// does it, and the CPU can spend those 32 scanlines doing something else.
//
// How it counts
// -------------
// The counter watches the PPU's address bus, specifically bit 12, which is
// the bit that chooses between the two pattern tables:
//
//     $0000-$0FFF  A12 = 0
//     $1000-$1FFF  A12 = 1
//
// During every scanline the PPU fetches sprite tiles from $1000-$1FFF, so A12
// goes 0 -> 1 once per scanline. That rising edge clocks the counter. This is
// why the mapper needs to see the PPU bus at all, and why Mapper has an
// on_ppu_address() hook.
//
// The registers
// -------------
//     $8000 even   bank select: bits 0-2 choose a register, bit 6 PRG mode,
//                  bit 7 CHR mode
//     $8001 odd    bank data: written to the register $8000 chose
//     $A000 even   mirroring (bit 0: 0 vertical, 1 horizontal)
//     $A001 odd    PRG RAM protect (ignored: every emulated game writes $80)
//     $C000 even   IRQ latch
//     $C001 odd    IRQ reload
//     $E000 even   IRQ disable and acknowledge
//     $E001 odd    IRQ enable
//
// Every register is one bit of the address (A0), which is why the even/odd
// distinction exists: the board saved a decoder by letting one address line
// do the selecting.
//
// Super Mario Bros. 3 is the game this file was written for.
// ---------------------------------------------------------------------------

#include "core/nes/mapper.hpp"

#include <array>
#include <cstddef>
#include <utility>
#include <vector>

namespace fc::nes {

class Mapper4 : public Mapper {
public:
    Mapper4(std::vector<u8> prg, std::vector<u8> chr, Mirroring default_mirroring)
        : prg_(std::move(prg))
        , chr_(std::move(chr))
        , mirroring_(default_mirroring)
    {
    }

    void make_chr_ram(std::size_t size = 8192)
    {
        chr_.assign(size, 0);
        chr_ram_ = true;
    }

    /// A second CHR store, for boards that mix ROM and RAM on the pattern
    /// bus: a few kilobytes of writable memory beside the ROM, selected by
    /// page number rather than by a mode bit.
    void make_chr_ram_window(std::size_t size)
    {
        chr_ram_window_.assign(size, 0);
    }

    // -- the CPU's view, $8000-$FFFF -----------------------------------------

    [[nodiscard]] u8 read_prg(u16 address) override
    {
        if (prg_.empty()) {
            return 0;
        }

        const std::size_t banks = prg_bank_count();
        const std::size_t last = banks - 1u;
        const std::size_t second_last = (banks >= 2u) ? (banks - 2u) : 0u;

        std::size_t bank = 0;
        switch (address & 0xE000u) {
        case 0x8000u:
            // PRG mode bit 6 swaps which half R6 controls.
            bank = (prg_mode_ == 0) ? map_prg_bank(prg_bank_[0]) : second_last;
            break;
        case 0xA000u:
            bank = map_prg_bank(prg_bank_[1]);
            break;
        case 0xC000u:
            bank = (prg_mode_ == 0) ? second_last : map_prg_bank(prg_bank_[0]);
            break;
        default:
            bank = last;
            break;
        }

        bank %= banks;
        return prg_[bank * 0x2000u + static_cast<std::size_t>(address & 0x1FFFu)];
    }

    void write_prg(u16 address, u8 value) override
    {
        const bool odd = (address & 0x0001u) != 0;

        // $E000 disables and acknowledges; $E001 enables. Checking it first
        // keeps the order of the switch below from growing a special case.
        switch (address & 0xE000u) {
        case 0x8000u:
            if (!odd) {
                bank_select_ = value;
                chr_mode_ = (value & 0x80u) != 0;
                prg_mode_ = (value & 0x40u) != 0;
            } else {
                write_bank_data(value);
            }
            break;

        case 0xA000u:
            if (!odd) {
                mirroring_ = ((value & 0x01u) != 0) ? Mirroring::Horizontal
                                                    : Mirroring::Vertical;
            }
            // $A001 is the PRG RAM protect register. No emulated game cares.
            break;

        case 0xC000u:
            if (!odd) {
                irq_latch_ = value;
            } else {
                irq_reload_ = true;
            }
            break;

        default:   // $E000-$FFFF
            if (!odd) {
                irq_enabled_ = false;
                irq_pending_ = false;
            } else {
                irq_enabled_ = true;
            }
            break;
        }
    }

    // -- the PPU's view, $0000-$1FFF -----------------------------------------

    [[nodiscard]] u8 read_chr(u16 address) override
    {
        const std::size_t slot = static_cast<std::size_t>(address >> 10) & 0x07u;
        const std::size_t page = map_chr_bank(slot, chr_slot_[slot]);
        return read_chr_page(page, static_cast<std::size_t>(address & 0x3FFu));
    }

    void write_chr(u16 address, u8 value) override
    {
        const std::size_t slot = static_cast<std::size_t>(address >> 10) & 0x07u;
        const std::size_t page = map_chr_bank(slot, chr_slot_[slot]);
        ++chr_writes_[page & 0xFFu];
        write_chr_page(page, static_cast<std::size_t>(address & 0x3FFu), value);
    }

    [[nodiscard]] Mirroring mirroring() const noexcept override { return mirroring_; }

    // -- the scanline counter ------------------------------------------------

    void on_ppu_address(u16 address) override
    {
        const bool a12 = (address & 0x1000u) != 0;
        if (a12 && !last_a12_) {
            clock_irq();
        }
        last_a12_ = a12;
    }

    [[nodiscard]] bool irq_asserted() const noexcept override { return irq_pending_; }

    // -- inspection, for tests ----------------------------------------------

    [[nodiscard]] u8 irq_counter() const noexcept { return irq_counter_; }
    [[nodiscard]] u8 irq_latch() const noexcept { return irq_latch_; }
    [[nodiscard]] bool irq_enabled() const noexcept { return irq_enabled_; }
    /// How many times the scanline counter has been clocked, and how many of
    /// those raised /IRQ. A frame of MMC3 rendering should clock it about
    /// once per scanline; a number far above 262 means the A12 edge detector
    /// is seeing extra edges and the games' splits will drift.
    [[nodiscard]] long irq_clock_count() const noexcept { return irq_clocks_; }
    [[nodiscard]] long irq_fire_count() const noexcept { return irq_fires_; }
    [[nodiscard]] u8 chr_register(int index) const noexcept
    {
        return (index >= 0 && index < 6) ? chr_reg_[index] : 0;
    }
    [[nodiscard]] u8 prg_register(int index) const noexcept
    {
        return (index >= 0 && index < 2) ? prg_bank_[index] : 0;
    }
    [[nodiscard]] u8 bank_select() const noexcept { return bank_select_; }

    /// How many times the CPU wrote to a 1KB CHR page, by the page's low byte.
    /// A game that writes a page it reads as ROM is a game that expects RAM
    /// there, which is the only reliable way to find where a board puts its
    /// pattern RAM when the header disagrees.
    [[nodiscard]] long chr_write_count(std::size_t page) const noexcept
    {
        return chr_writes_[page & 0xFFu];
    }

private:
    [[nodiscard]] std::size_t prg_bank_count() const noexcept
    {
        const std::size_t count = prg_.size() / 0x2000u;
        return (count == 0) ? 1 : count;
    }

protected:
    /// A subclass may permute the 8KB PRG bank a register selects. Mapper 249
    /// is the reason this exists: its board wires the ROM address lines in a
    /// scrambled order, so the same register value lands on a different
    /// physical bank. The two hardware-fixed windows at the top are not sent
    /// through here, because their whole job is to stay at the end of the ROM.
    [[nodiscard]] virtual std::size_t map_prg_bank(std::size_t bank) const noexcept
    {
        return bank;
    }

    /// Same, for the 1KB CHR bank a window points at.
    [[nodiscard]] virtual std::size_t map_chr_bank(std::size_t /*slot*/,
                                                   std::size_t bank) const noexcept
    {
        return bank;
    }

    /// Whether the 1KB CHR page a slot points at lives in the RAM window
    /// rather than in the ROM. The page is the mapped page number, which for
    /// every board that mixes ROM and RAM is also the register value. The
    /// default is the old behaviour: the whole CHR is RAM, or none of it is.
    [[nodiscard]] virtual bool chr_page_is_ram(std::size_t /*page*/) const noexcept
    {
        return chr_ram_;
    }

protected:
    /// Read one byte of a 1KB CHR page, from the RAM window or the ROM as the
    /// page says. Shared by read_chr and by subclasses that map slots
    /// themselves.
    [[nodiscard]] u8 read_chr_page(std::size_t page, std::size_t offset) const
    {
        if (chr_page_is_ram(page)) {
            if (chr_ram_window_.empty()) {
                return 0;
            }
            const std::size_t banks = chr_ram_window_.size() / 0x400u;
            return chr_ram_window_[(page % banks) * 0x400u + offset];
        }
        const std::size_t banks = chr_.size() / 0x400u;
        if (banks == 0) {
            return 0;
        }
        return chr_[(page % banks) * 0x400u + offset];
    }

    void write_chr_page(std::size_t page, std::size_t offset, u8 value)
    {
        if (chr_page_is_ram(page)) {
            if (chr_ram_window_.empty()) {
                return;
            }
            const std::size_t banks = chr_ram_window_.size() / 0x400u;
            chr_ram_window_[(page % banks) * 0x400u + offset] = value;
            return;
        }
        if (!chr_ram_) {
            return;
        }
        const std::size_t banks = chr_.size() / 0x400u;
        if (banks == 0) {
            return;
        }
        chr_[(page % banks) * 0x400u + offset] = value;
    }

    /// The page a slot currently points at, after any subclass permutation.
    [[nodiscard]] std::size_t chr_page_for_slot(std::size_t slot) const noexcept
    {
        return map_chr_bank(slot, chr_slot_[slot & 0x07u]);
    }

    /// One byte of an 8KB PRG bank, for a subclass that maps a window itself
    /// rather than through map_prg_bank().
    [[nodiscard]] u8 read_prg_bank(std::size_t bank, u16 address) const
    {
        if (prg_.empty()) {
            return 0;
        }
        const std::size_t banks = prg_.size() / 0x2000u;
        const std::size_t b = (banks == 0u) ? 0u : bank % banks;
        return prg_[b * 0x2000u + static_cast<std::size_t>(address & 0x1FFFu)];
    }

    void write_bank_data(u8 value)
    {
        const u8 reg = static_cast<u8>(bank_select_ & 0x07u);
        switch (reg) {
        case 0: case 1: case 2: case 3: case 4: case 5:
            chr_reg_[reg] = value;
            break;
        case 6:
            prg_bank_[0] = static_cast<u8>(value & 0x3Fu);
            break;
        default:
            prg_bank_[1] = static_cast<u8>(value & 0x3Fu);
            break;
        }
        update_chr_slots();
    }

    /// The CHR mode bit rearranges which register feeds which 1KB slot.
    ///
    /// 2KB mode: R0 and R1 each cover 2KB at the bottom; R2-R5 cover the
    ///           top 4KB one kilobyte at a time.
    /// 1KB mode: the two halves swap. R2-R5 move to the bottom and R0/R1
    ///           become the four 1KB banks at the top.
    void update_chr_slots() noexcept
    {
        if (!chr_mode_) {
            chr_slot_[0] = static_cast<u8>(chr_reg_[0] & 0xFEu);
            chr_slot_[1] = static_cast<u8>((chr_reg_[0] & 0xFEu) | 0x01u);
            chr_slot_[2] = static_cast<u8>(chr_reg_[1] & 0xFEu);
            chr_slot_[3] = static_cast<u8>((chr_reg_[1] & 0xFEu) | 0x01u);
            chr_slot_[4] = chr_reg_[2];
            chr_slot_[5] = chr_reg_[3];
            chr_slot_[6] = chr_reg_[4];
            chr_slot_[7] = chr_reg_[5];
        } else {
            chr_slot_[0] = chr_reg_[2];
            chr_slot_[1] = chr_reg_[3];
            chr_slot_[2] = chr_reg_[4];
            chr_slot_[3] = chr_reg_[5];
            chr_slot_[4] = chr_reg_[0];
            chr_slot_[5] = static_cast<u8>(chr_reg_[0] + 1u);
            chr_slot_[6] = chr_reg_[1];
            chr_slot_[7] = static_cast<u8>(chr_reg_[1] + 1u);
        }
    }

    void clock_irq() noexcept
    {
        ++irq_clocks_;
        if (irq_counter_ == 0 || irq_reload_) {
            irq_counter_ = irq_latch_;
        } else {
            --irq_counter_;
        }
        irq_reload_ = false;

        if (irq_counter_ == 0 && irq_enabled_) {
            irq_pending_ = true;
            ++irq_fires_;
        }
    }

    std::vector<u8> prg_;
    std::vector<u8> chr_;
    std::vector<u8> chr_ram_window_;
    std::array<long, 256> chr_writes_{};
    Mirroring mirroring_;
    bool chr_ram_ = false;

    u8 bank_select_ = 0;
    bool chr_mode_ = false;     // false = 2KB, true = 1KB
    bool prg_mode_ = false;     // false = R6 at $8000, true = R6 at $C000
    u8 chr_reg_[6] = { 0, 0, 0, 0, 0, 0 };
    u8 chr_slot_[8] = { 0, 1, 2, 3, 4, 5, 6, 7 };
    u8 prg_bank_[2] = { 0, 0 };

    bool last_a12_ = false;
    u8 irq_latch_ = 0;
    u8 irq_counter_ = 0;
    bool irq_reload_ = false;
    bool irq_enabled_ = false;
    bool irq_pending_ = false;

    // The two counters below are diagnostics, not behaviour, but they are kept
    // in the state so that "how many times has this fired" stays monotonic
    // across a save and load. Written as explicitly 32 bit: `long` is 64 bit
    // on arm64 and 32 on wasm32, and a state file that changed size depending
    // on which build wrote it could not be compared between them.
    long irq_clocks_ = 0;
    long irq_fires_ = 0;

    // -- save states ---------------------------------------------------------
    //
    // MMC3 is the mapper that makes save states hard. There are eight bank
    // registers, two of them four bit halves in one byte; a derived table of
    // eight CHR slot numbers; the two PRG window registers; the mirroring that
    // is packed into the same byte as the bank select; and a scanline counter
    // with its own latch, reload flag and pending flag, which is armed or not
    // depending on the exact A12 edge that last went past.
    //
    // All of it is written. The derived table is written too, rather than
    // recomputed, so that a reader never has to reproduce the mapper's
    // internal arithmetic and get it subtly wrong.

public:
    void serialize(StateWriter& out) const override
    {
        out.put_u8(bank_select_);
        out.put_u8(static_cast<u8>(mirroring_));
        out.put_flag(chr_mode_);
        out.put_flag(prg_mode_);

        for (const u8 bank : chr_reg_) {
            out.put_u8(bank);
        }
        for (const u8 slot : chr_slot_) {
            out.put_u8(slot);
        }
        for (const u8 bank : prg_bank_) {
            out.put_u8(bank);
        }

        out.put_flag(last_a12_);
        out.put_u8(irq_latch_);
        out.put_u8(irq_counter_);
        out.put_flag(irq_reload_);
        out.put_flag(irq_enabled_);
        out.put_flag(irq_pending_);
        out.put_s32(static_cast<s32>(irq_clocks_));
        out.put_s32(static_cast<s32>(irq_fires_));

        out.put_flag(chr_ram_);
        if (chr_ram_) {
            out.sized_bytes(chr_);
        }
        out.put_flag(!chr_ram_window_.empty());
        if (!chr_ram_window_.empty()) {
            out.sized_bytes(chr_ram_window_);
        }
    }

    bool deserialize(StateReader& in) override
    {
        in.get_u8(bank_select_);

        u8 mirroring = 0;
        in.get_u8(mirroring);
        mirroring_ = static_cast<Mirroring>(mirroring);

        in.get_flag(chr_mode_);
        in.get_flag(prg_mode_);

        for (auto& bank : chr_reg_) {
            in.get_u8(bank);
        }
        for (auto& slot : chr_slot_) {
            in.get_u8(slot);
        }
        for (auto& bank : prg_bank_) {
            in.get_u8(bank);
        }

        in.get_flag(last_a12_);
        in.get_u8(irq_latch_);
        in.get_u8(irq_counter_);
        in.get_flag(irq_reload_);
        in.get_flag(irq_enabled_);
        in.get_flag(irq_pending_);

        s32 clocks = 0;
        s32 fires = 0;
        in.get_s32(clocks);
        in.get_s32(fires);
        irq_clocks_ = clocks;
        irq_fires_ = fires;

        in.get_flag(chr_ram_);
        if (chr_ram_) {
            in.sized_bytes(chr_);
        }
        bool has_window = false;
        in.get_flag(has_window);
        if (has_window) {
            in.sized_bytes(chr_ram_window_);
        }
        return in.ok();
    }

    [[nodiscard]] bool saves_state() const noexcept override { return true; }

private:
};

} // namespace fc::nes
