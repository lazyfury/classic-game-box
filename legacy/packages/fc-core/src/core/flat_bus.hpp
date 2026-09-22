#pragma once

// ---------------------------------------------------------------------------
// FlatBus - a plain 64KB array of bytes.
//
// This is NOT the NES bus. The NES bus has to decode addresses into RAM, PPU
// registers, APU registers and cartridge space (Phase 2). FlatBus exists so
// that the CPU can be tested and demonstrated on its own, with no console
// hardware involved.
//
// Teaching point: the CPU does not change when the bus changes. Same CPU,
// different bus, entirely different machine. That is why the dependency
// direction matters.
// ---------------------------------------------------------------------------

#include "core/bus.hpp"
#include "core/types.hpp"

#include <array>
#include <cstddef>
#include <span>

namespace fc {

class FlatBus : public Bus {
public:
    static constexpr std::size_t kAddressSpace = 0x10000; // 2^16

    [[nodiscard]] u8 read(u16 address) override
    {
        return memory_[address];
    }

    void write(u16 address, u8 value) override
    {
        memory_[address] = value;
    }

    /// Copy a program into memory, as a ROM loader would.
    void load(std::span<const u8> bytes, u16 address)
    {
        for (std::size_t i = 0; i < bytes.size(); ++i) {
            memory_[static_cast<std::size_t>(address) + i] = bytes[i];
        }
    }

    void clear()
    {
        memory_.fill(0);
    }

private:
    std::array<u8, kAddressSpace> memory_{};
};

} // namespace fc
