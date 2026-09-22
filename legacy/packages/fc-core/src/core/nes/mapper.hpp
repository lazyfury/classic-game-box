#pragma once

// ---------------------------------------------------------------------------
// Mapper - the cartridge's own address decoder.
//
// A cartridge is not just a ROM chip. It is a small circuit board with logic
// on it, and that logic decides what each address means. That logic is the
// mapper.
//
// Why mappers exist
// -----------------
// The 6502 can only address 32KB of cartridge space ($8000-$FFFF). Early
// games fit, and needed no mapper at all. Later games did not, so the mapper
// adds a bank register: the CPU writes a number to some address, and the
// cartridge re-points a window of the address space at a different bank of
// the ROM.
//
//     CPU writes $05 to $8000  ->  the cartridge swaps bank 5 into $A000-$BFFF
//
// The ROM chip never changes. Only the wiring inside the cartridge does.
//
// This is why "mapper" is emulated as code and not as data: it is logic, not
// storage.
//
// The CHR side
// ------------
// Note that read_chr/write_chr are separate from read_prg/write_prg. The
// pattern tables are on the PPU's bus, not the CPU's. They are two different
// address spaces that happen to live on the same board.
// ---------------------------------------------------------------------------

#include "core/nes/ines.hpp"
#include "core/state.hpp"
#include "core/types.hpp"

namespace fc::nes {

class Mapper {
public:
    virtual ~Mapper() = default;

    // -- the CPU's view, $8000-$FFFF -----------------------------------------

    [[nodiscard]] virtual u8 read_prg(u16 address) = 0;
    virtual void write_prg(u16 address, u8 value) = 0;

    // -- the PPU's view, $0000-$1FFF -----------------------------------------

    [[nodiscard]] virtual u8 read_chr(u16 address) = 0;
    virtual void write_chr(u16 address, u8 value) = 0;

    /// The cartridge wires the PPU's nametables, so it owns this.
    [[nodiscard]] virtual Mirroring mirroring() const noexcept = 0;

    // -- the optional extras -------------------------------------------------
    //
    // Two very different families of mapper need to watch the PPU's address
    // bus, and one of them needs to interrupt the CPU:
    //
    //   * the MMC3 clocks a scanline counter on the rising edge of PPU A12
    //     (which happens once per scanline, during the sprite fetch), and
    //     raises /IRQ when the counter runs out
    //   * the MMC2/MMC4 flip a CHR latch when a particular tile is fetched
    //
    // Both hooks have empty bodies by default, so NROM, MMC1 and the rest are
    // not touched by any of it. That is the whole reason the interface is
    // shaped this way: the CPU and PPU do not need to know which mapper is in
    // the slot, only that these messages are safe to send to any of them.

    /// Called for every access the PPU makes to $0000-$1FFF.
    virtual void on_ppu_address(u16 /*address*/) {}

    /// The cartridge's /IRQ line, level triggered. False for every mapper
    /// without an interrupt source.
    [[nodiscard]] virtual bool irq_asserted() const noexcept { return false; }

    // -- the expansion area, $4020-$5FFF -------------------------------------
    //
    // A licensed cartridge leaves these address lines unconnected, and the
    // bus sees open bus. A handful of unlicensed boards instead put a latch
    // there: the Nanjing board's bank registers live at $5000, and boards
    // with extra audio (VRC6, MMC5, ...) answer here too. The Cartridge
    // forwards the cycle and the mapper decides whether it means anything.

    /// Read a byte from the expansion area. Nothing connected reads 0.
    [[nodiscard]] virtual u8 read_expansion(u16 /*address*/) { return 0; }

    /// Write a byte to the expansion area. Nothing connected ignores it.
    virtual void write_expansion(u16 /*address*/, u8 /*value*/) {}

    /// Whether the board has the usual 8KB of work RAM at $6000-$7FFF.
    ///
    /// Almost every cartridge does, and the Cartridge answers those reads
    /// before the mapper sees them. A board that instead puts registers there
    /// (the Jaleco JF-13 at $6000, some multicarts) answers false, and the
    /// Cartridge forwards the cycle to the expansion hooks instead.
    [[nodiscard]] virtual bool has_work_ram() const noexcept { return true; }

    // -- the beam position ---------------------------------------------------
    //
    // MMC3 counts A12 edges for itself through on_ppu_address(). A few
    // boards need to know *where the beam is* rather than *what address is
    // on the bus*: the Nanjing board's automatic 4 KiB CHR-RAM switch is
    // wired to PPU A13/A9, and a scanline is the closest thing this PPU
    // exposes. Default: ignore.
    virtual void on_scanline(int /*scanline*/) noexcept {}

    // -- mappers whose IRQ counts CPU cycles -------------------------------
    //
    // MMC3 gets its clock free from the PPU's A12 line, but the Konami VRC4,
    // Jaleco SS88006 and Sunsoft FME-7 count CPU cycles instead. Asking for
    // them is opt-in so the Machine only pays for a per-cycle callback when
    // the cartridge really needs one.

    /// True if this mapper wants on_cpu_cycle() once per CPU cycle.
    [[nodiscard]] virtual bool clocks_on_cpu_cycles() const noexcept { return false; }

    /// One CPU cycle passed. Only called when clocks_on_cpu_cycles() is true.
    virtual void on_cpu_cycle() noexcept {}

    // -- save states ---------------------------------------------------------
    //
    // The bank registers are state, and they are the part of a save state that
    // is easiest to forget. They are not memory; they are the wiring of a
    // circuit board. A state that restores everything except these comes back
    // showing the wrong part of the ROM, and does it about three instructions
    // later, when the CPU fetches from a window that has moved.
    //
    // A mapper that does not override these saves nothing, which makes its
    // save states incomplete rather than wrong -- the machine is exactly where
    // it was in time, with the board wired the way it was at power on. That is
    // a real limitation and it is measurable rather than hidden:
    // `wasm/verify.sh` round trips every ROM in the folder, so the list of
    // mappers that are not covered shows up as a failing test.

    virtual void serialize(StateWriter& /*out*/) const {}
    virtual bool deserialize(StateReader& /*in*/) { return true; }

    /// True when the two methods above actually write something, so a front
    /// end can say a save state is partial instead of implying it is not.
    [[nodiscard]] virtual bool saves_state() const noexcept { return false; }
};

} // namespace fc::nes
