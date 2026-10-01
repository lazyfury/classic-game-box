#pragma once

// ---------------------------------------------------------------------------
// RamCartridge - a development stand-in for the cartridge slot.
//
// This is NOT a cartridge. There is no ROM, no header, no mapper. It is a
// 48KB block of RAM mapped over $4020-$FFFF so that the CPU and the bus have
// somewhere to put a program before Phase 3 adds the iNES loader.
//
// It exists for the same reason FlatBus does: hardware we have not written
// yet still has to answer, or nothing can be run.
//
// Phase 3 replaces this with a real Cartridge: iNES parsing, PRG ROM, CHR
// ROM, and a mapper deciding what each address means.
// ---------------------------------------------------------------------------

#include "core/nes/device.hpp"
#include "core/types.hpp"

#include <array>
#include <cstddef>
#include <span>

namespace fc::nes {

class RamCartridge : public Device {
public:
    static constexpr u16 kBase = 0x4020;
    static constexpr u16 kSize = 0x10000 - kBase;   // $BFE0 bytes

    [[nodiscard]] u8 read(u16 address) override
    {
        const std::size_t index = static_cast<std::size_t>(address - kBase);
        if (index >= kSize) {
            return 0;
        }
        return bytes_[index];
    }

    void write(u16 address, u8 value) override
    {
        const std::size_t index = static_cast<std::size_t>(address - kBase);
        if (index >= kSize) {
            return;
        }
        bytes_[index] = value;
    }

    /// Install a program, the way a ROM loader would.
    void load(std::span<const u8> program, u16 address)
    {
        for (std::size_t i = 0; i < program.size(); ++i) {
            const std::size_t index = static_cast<std::size_t>(address + i - kBase);
            if (index >= kSize) {
                continue;
            }
            bytes_[index] = program[i];
        }
    }

    void clear() { bytes_.fill(0); }

private:
    std::array<u8, kSize> bytes_{};
};

} // namespace fc::nes
