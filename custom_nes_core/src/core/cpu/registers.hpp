#pragma once

// ---------------------------------------------------------------------------
// The 6502 register file.
//
// A CPU register is storage that lives *inside* the chip. Reading RAM costs a
// bus cycle; reading a register is nearly free. That is why the 6502 has so
// few of them - silicon was expensive in 1975.
//
//    A   accumulator         8 bit   the only register the ALU can add into
//    X   index register      8 bit   indexing / counter
//    Y   index register      8 bit   indexing / counter
//    SP  stack pointer       8 bit   low byte of an address in page 1
//    PC  program counter    16 bit   address of the NEXT instruction
//    P   status register     8 bit   eight independent 1 bit flags
//
// Total state: 6 bytes. That is the entire "mind" of the machine.
// ---------------------------------------------------------------------------

#include "core/bit.hpp"
#include "core/types.hpp"

#include <string>

namespace fc {

/// Bit positions inside the status register P.
///
///    bit:  7   6   5   4   3   2   1   0
///          N   V   -   B   D   I   Z   C
///
/// The NES CPU (Ricoh 2A03) removed decimal mode, but the D bit still exists
/// in the register - it is just ignored by ADC/SBC.
enum class Flag : u8 {
    Carry      = 0, // C: carry out of bit 7 / "no borrow" after SBC
    Zero       = 1, // Z: the last result was 0
    IrqDisable = 2, // I: 1 = ignore IRQ
    Decimal    = 3, // D: unused on NES
    Break      = 4, // B: set when the flags were pushed by BRK/PHP
    Unused     = 5, // always reads as 1
    Overflow   = 6, // V: signed overflow (see docs/computer-science/overflow-flag.md)
    Negative   = 7, // N: a copy of bit 7 of the last result
};

/// Value of P after reset: I set, unused bit set.
inline constexpr u8 kResetStatus = 0x24;

/// Value of SP after reset: the reset sequence "pushes" 3 bytes without
/// writing them, moving SP from 0xFF down to 0xFD.
inline constexpr u8 kResetStackPointer = 0xFD;

struct Registers {
    u8  a  = 0;                    // accumulator
    u8  x  = 0;                    // index X
    u8  y  = 0;                    // index Y
    u8  sp = kResetStackPointer;   // stack pointer
    u8  p  = kResetStatus;         // status flags
    u16 pc = 0;                    // program counter

    // -- flags ---------------------------------------------------------------

    [[nodiscard]] bool flag(Flag f) const noexcept
    {
        return bit::test(p, static_cast<int>(f));
    }

    void set_flag(Flag f, bool on) noexcept
    {
        p = bit::assign(p, static_cast<int>(f), on);
    }

    /// Set a flag from a "is this value non-zero" test.
    void set_flag_if(Flag f, bool condition) noexcept
    {
        set_flag(f, condition);
    }

    /// The single most used flag update on the 6502: N copies bit 7 of the
    /// result, Z is "result == 0". Almost every ALU instruction ends with it.
    void update_nz(u8 value) noexcept
    {
        set_flag(Flag::Negative, (value & 0x80) != 0);
        set_flag(Flag::Zero, value == 0);
    }

    // -- reset ---------------------------------------------------------------

    void reset() noexcept
    {
        a  = 0;
        x  = 0;
        y  = 0;
        sp = kResetStackPointer;
        p  = kResetStatus;
        // PC is not set here: the CPU loads it from the reset vector.
        pc = 0;
    }

    // -- debugging -----------------------------------------------------------

    /// "NV-BDIZC" with dots for cleared flags, eg "nv-bdIZc".
    [[nodiscard]] std::string status_string() const
    {
        struct Entry { Flag flag; char letter; };
        static constexpr Entry entries[] = {
            { Flag::Negative,   'N' },
            { Flag::Overflow,   'V' },
            { Flag::Unused,     '-' },
            { Flag::Break,      'B' },
            { Flag::Decimal,    'D' },
            { Flag::IrqDisable, 'I' },
            { Flag::Zero,       'Z' },
            { Flag::Carry,      'C' },
        };

        std::string out;
        for (const auto& e : entries) {
            out.push_back(flag(e.flag) ? e.letter : '.');
        }
        return out;
    }
};

} // namespace fc
