#pragma once

// ---------------------------------------------------------------------------
// Device - the seam that keeps the CPU ignorant of the hardware.
//
// A real CPU has no idea that RAM, the PPU or a cartridge exist. It puts an
// address on 16 wires and reads or writes 8 data wires. Whoever answers is
// decided by an address decoder built out of logic gates.
//
// This interface is that decoder's plug. The CPU only ever calls
// Bus::read()/Bus::write(); the bus decides which Device answers.
//
// Rule from AGENTS.md: the CPU must never include a PPU header. If it did,
// the CPU could not be tested alone and porting the core would mean
// rewriting the CPU.
// ---------------------------------------------------------------------------

#include "core/types.hpp"

namespace fc::nes {

/// A device that answers over some range of the CPU address space.
class Device {
public:
    virtual ~Device() = default;

    /// `address` is the FULL 16 bit CPU address, not an offset.
    ///
    /// That is deliberate: a device whose address lines are only partly
    /// connected needs to see the bits it ignores. The 2KB RAM is the
    /// canonical example - it ignores the top 5 bits entirely.
    [[nodiscard]] virtual u8 read(u16 address) = 0;

    virtual void write(u16 address, u8 value) = 0;
};

/// Receives the 256 bytes of an OAM DMA ($4014).
///
/// Phase 4 makes the PPU implement this. It is separate from Device because
/// a DMA is not an address range - it is a bulk transfer into a specific
/// piece of hardware.
class OamTarget {
public:
    virtual ~OamTarget() = default;
    virtual void write_oam(u8 index, u8 value) = 0;
};

} // namespace fc::nes
