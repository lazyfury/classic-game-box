#pragma once

// ---------------------------------------------------------------------------
// Effective address resolution.
//
// The addressing mode does not say where the data is. It says HOW to compute
// where the data is. The computed result is called the EFFECTIVE ADDRESS.
//
//      LDA $0200,X        X = 5
//           |
//           +-- mode: AbsoluteX
//           +-- operand bytes: 00 02  -> $0200
//           +-- effective address = $0200 + 5 = $0205
//
// Then, and only then, does the CPU read [$0205].
//
// Two of the modes reproduce quirks in the real silicon. They are not bugs in
// our emulator - they are bugs in the 1975 chip, and games exist that depend
// on them:
//
//   1. Zero page wrapping
//      $42,X with X = $C0 gives $0002, not $0102. The high byte is hard
//      wired to zero, so the addition cannot carry out of page 0.
//
//   2. JMP (indirect) page wrapping
//      JMP ($10FF) reads its low byte from $10FF and its high byte from
//      $1000 - NOT from $1100. The high byte is taken from the same page
//      as the low byte.
// ---------------------------------------------------------------------------

#include "core/bus.hpp"
#include "core/cpu/opcode.hpp"
#include "core/types.hpp"

namespace fc {

/// What an addressing mode produced.
enum class OperandKind : u8 {
    None,      // implied / accumulator: there is no operand to fetch
    Value,     // immediate: `value` IS the data
    Address,   // memory: `address` is where the data lives
    Target,    // relative: `address` is the branch destination
};

struct Operand {
    OperandKind kind = OperandKind::None;

    /// Valid when kind == Address (where to read/write) or Target (where to jump).
    u16 address = 0;

    /// Valid when kind == Value.
    u8 value = 0;

    /// True when adding the index crossed a 256 byte page boundary.
    /// On real hardware this costs one extra cycle for reads.
    bool page_crossed = false;

    [[nodiscard]] bool is_none() const noexcept    { return kind == OperandKind::None; }
    [[nodiscard]] bool is_value() const noexcept   { return kind == OperandKind::Value; }
    [[nodiscard]] bool is_address() const noexcept { return kind == OperandKind::Address; }
    [[nodiscard]] bool is_target() const noexcept  { return kind == OperandKind::Target; }
};

/// Everything resolve() needs. The CPU fills the operand bytes in while it
/// fetches them, then hands the whole thing over.
struct AddressingRequest {
    AddressingMode mode = AddressingMode::Implied;
    u8  operand_lo      = 0;
    u8  operand_hi      = 0;
    u8  x               = 0;   // index registers, sampled BEFORE the operation
    u8  y               = 0;
    u16 instruction_pc  = 0;   // needed for relative branches
};

/// Compute the effective address (or immediate value, or branch target).
///
/// `bus` is touched only by the three indirect modes, which dereference a
/// pointer. Every other mode is pure arithmetic.
[[nodiscard]] Operand resolve(const AddressingRequest& request, Bus& bus) noexcept;

/// Read a 16 bit pointer from zero page.
///
/// The pointer always lives in page 0, so its high byte wraps inside that
/// page: a pointer at $FF takes its low byte from $00FF and its high byte
/// from $0000.
[[nodiscard]] u16 read_pointer_zero_page(Bus& bus, u8 address) noexcept;

/// Read a 16 bit pointer for JMP (indirect).
///
/// The low byte comes from `address`. The high byte comes from the SAME PAGE:
/// if `address` ends in $FF the high byte is read from $xx00, not $(xx+1)00.
[[nodiscard]] u16 read_pointer_indirect(Bus& bus, u16 address) noexcept;

/// Cycle cost of fetching and resolving this addressing mode, opcode fetch
/// included. This is the base before any operation specific adjustment.
[[nodiscard]] constexpr int addressing_cycles(AddressingMode mode) noexcept
{
    switch (mode) {
    case AddressingMode::Implied:
    case AddressingMode::Accumulator:
    case AddressingMode::Immediate:
    case AddressingMode::Relative:
        return 2;

    case AddressingMode::ZeroPage:
        return 3;

    case AddressingMode::ZeroPageX:
    case AddressingMode::ZeroPageY:
        return 4;

    case AddressingMode::Absolute:
    case AddressingMode::AbsoluteX:
    case AddressingMode::AbsoluteY:
        return 4;

    case AddressingMode::Indirect:
        return 5;

    case AddressingMode::IndirectX:
        return 6;

    case AddressingMode::IndirectY:
        return 5;

    case AddressingMode::Unknown:
        return 0;
    }
    return 0;
}

} // namespace fc
