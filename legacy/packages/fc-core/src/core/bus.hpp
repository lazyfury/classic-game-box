#pragma once

// ---------------------------------------------------------------------------
// Bus - the only way the CPU is allowed to touch the outside world.
//
// A real CPU does not "know" that RAM, the PPU and the cartridge exist. It has
// one address bus (16 wires) and one data bus (8 wires). When it wants a byte
// it puts an address on the wires and reads whatever answers.
//
// Whoever answers is decided by an address decoder, not by the CPU. That is
// exactly what this interface models:
//
//      CPU  --->  Bus (decides who answers)  --->  RAM / PPU / Cartridge
//
// Rule (AGENTS.md): the CPU must never #include a PPU header. If it did, the
// CPU could not be tested on its own, and porting the core would mean
// rewriting the CPU.
//
// Read is intentionally non-const: reading a PPU register has side effects
// (reading $2002 clears the vblank flag, reading $2007 advances VRAM).
// ---------------------------------------------------------------------------

#include "core/types.hpp"

namespace fc {

class Bus {
public:
    virtual ~Bus() = default;

    /// Read one byte from the 16 bit address space.
    [[nodiscard]] virtual u8 read(u16 address) = 0;

    /// Write one byte to the 16 bit address space.
    virtual void write(u16 address, u8 value) = 0;

    /// Cycles the CPU has to wait because the BUS did something on its own.
    ///
    /// This is not an instruction cost - it is the bus stealing time. The
    /// canonical case is OAM DMA ($4014): writing one byte makes the bus
    /// copy 256 bytes and stall the CPU for 513 cycles.
    ///
    /// The CPU drains this after every instruction, so a device can stall
    /// the CPU without the CPU knowing what a device is.
    virtual int take_stall_cycles() { return 0; }
};

} // namespace fc
