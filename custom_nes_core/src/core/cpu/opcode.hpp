#pragma once

// ---------------------------------------------------------------------------
// Opcode metadata: what does a byte mean?
//
// An opcode encodes TWO independent things:
//
//     0xBD  =  LDA $8000,X
//              ^^^   ^^^^^^^
//              |     +-- addressing mode: HOW to compute the address
//              +-------- operation: what to do once we have the data
//
// Neither half is stored separately. The byte IS both. That is why the 6502
// spends 151 of its 256 codes on the "official" instruction set and leaves
// the rest undefined.
//
// There is a third, derived fact:
//
//     LDA #$42      immediate   -> 2 bytes (opcode + 1 operand byte)
//     LDA $42       zero page   -> 2 bytes
//     LDA $8000     absolute    -> 3 bytes (opcode + 2 operand bytes)
//     TAX           implied     -> 1 byte  (no operand at all)
//
// The addressing mode alone decides the length, which is what lets the CPU
// advance PC without understanding the instruction.
//
// Phase 0.4 note: the table used to store the mnemonic as a string. Strings
// are data, not types - the CPU cannot switch on them. Storing an Operation
// enum instead removes the duplication between this table and the CPU's
// execute() switch, and lets the compiler check that every case is handled.
// ---------------------------------------------------------------------------

#include "core/types.hpp"

namespace fc {

/// The 56 mnemonics of the official NMOS 6502, plus Unknown.
///
/// The order follows the customary grouping from the datasheet.
enum class Operation : u8 {
    Unknown,

    // load / store
    LDA, LDX, LDY, STA, STX, STY,

    // register transfers
    TAX, TAY, TSX, TXA, TXS, TYA,

    // stack
    PHA, PHP, PLA, PLP,

    // logic
    AND, EOR, ORA, BIT,

    // arithmetic / compare
    ADC, SBC, CMP, CPX, CPY,
    INC, INX, INY, DEC, DEX, DEY,

    // shifts and rotates
    ASL, LSR, ROL, ROR,

    // jumps and subroutines
    JMP, JSR, RTS, RTI, BRK,

    // conditional branches
    BCC, BCS, BEQ, BMI, BNE, BPL, BVC, BVS,

    // flag control
    CLC, CLD, CLI, CLV, SEC, SED, SEI,

    // nothing at all
    NOP,
};

[[nodiscard]] constexpr const char* operation_name(Operation op) noexcept
{
    switch (op) {
    case Operation::Unknown: return "???";

    case Operation::LDA: return "LDA";
    case Operation::LDX: return "LDX";
    case Operation::LDY: return "LDY";
    case Operation::STA: return "STA";
    case Operation::STX: return "STX";
    case Operation::STY: return "STY";

    case Operation::TAX: return "TAX";
    case Operation::TAY: return "TAY";
    case Operation::TSX: return "TSX";
    case Operation::TXA: return "TXA";
    case Operation::TXS: return "TXS";
    case Operation::TYA: return "TYA";

    case Operation::PHA: return "PHA";
    case Operation::PHP: return "PHP";
    case Operation::PLA: return "PLA";
    case Operation::PLP: return "PLP";

    case Operation::AND: return "AND";
    case Operation::EOR: return "EOR";
    case Operation::ORA: return "ORA";
    case Operation::BIT: return "BIT";

    case Operation::ADC: return "ADC";
    case Operation::SBC: return "SBC";
    case Operation::CMP: return "CMP";
    case Operation::CPX: return "CPX";
    case Operation::CPY: return "CPY";
    case Operation::INC: return "INC";
    case Operation::INX: return "INX";
    case Operation::INY: return "INY";
    case Operation::DEC: return "DEC";
    case Operation::DEX: return "DEX";
    case Operation::DEY: return "DEY";

    case Operation::ASL: return "ASL";
    case Operation::LSR: return "LSR";
    case Operation::ROL: return "ROL";
    case Operation::ROR: return "ROR";

    case Operation::JMP: return "JMP";
    case Operation::JSR: return "JSR";
    case Operation::RTS: return "RTS";
    case Operation::RTI: return "RTI";
    case Operation::BRK: return "BRK";

    case Operation::BCC: return "BCC";
    case Operation::BCS: return "BCS";
    case Operation::BEQ: return "BEQ";
    case Operation::BMI: return "BMI";
    case Operation::BNE: return "BNE";
    case Operation::BPL: return "BPL";
    case Operation::BVC: return "BVC";
    case Operation::BVS: return "BVS";

    case Operation::CLC: return "CLC";
    case Operation::CLD: return "CLD";
    case Operation::CLI: return "CLI";
    case Operation::CLV: return "CLV";
    case Operation::SEC: return "SEC";
    case Operation::SED: return "SED";
    case Operation::SEI: return "SEI";

    case Operation::NOP: return "NOP";
    }
    return "???";
}

/// The 13 ways of naming an operand.
///
///    Implied       TAX            the operand is implicit
///    Accumulator   ASL A          the operand is the accumulator
///    Immediate     LDA #$42       the operand is a literal byte
///    ZeroPage      LDA $42        address is 1 byte, so $0042
///    ZeroPageX     LDA $42,X      1 byte address + X, wraps inside page 0
///    ZeroPageY     LDX $42,Y      1 byte address + Y, wraps inside page 0
///    Absolute      LDA $8000      2 byte address
///    AbsoluteX     LDA $8000,X    2 byte address + X
///    AbsoluteY     LDA $8000,Y    2 byte address + Y
///    Indirect      JMP ($8000)    address stored at $8000
///    IndirectX     LDA ($42,X)    zp pointer + X, then dereference
///    IndirectY     LDA ($42),Y    zp pointer, dereference, then + Y
///    Relative      BNE $8010      1 byte SIGNED offset from the next PC
enum class AddressingMode : u8 {
    Implied,
    Accumulator,
    Immediate,
    ZeroPage,
    ZeroPageX,
    ZeroPageY,
    Absolute,
    AbsoluteX,
    AbsoluteY,
    Indirect,
    IndirectX,
    IndirectY,
    Relative,

    Unknown,   // illegal / unofficial opcode
};

/// Number of operand bytes that follow the opcode.
[[nodiscard]] constexpr int operand_length(AddressingMode mode) noexcept
{
    switch (mode) {
    case AddressingMode::Implied:
    case AddressingMode::Accumulator:
        return 0;

    case AddressingMode::Immediate:
    case AddressingMode::ZeroPage:
    case AddressingMode::ZeroPageX:
    case AddressingMode::ZeroPageY:
    case AddressingMode::IndirectX:
    case AddressingMode::IndirectY:
    case AddressingMode::Relative:
        return 1;

    case AddressingMode::Absolute:
    case AddressingMode::AbsoluteX:
    case AddressingMode::AbsoluteY:
    case AddressingMode::Indirect:
        return 2;

    case AddressingMode::Unknown:
        return 0;
    }
    return 0;
}

/// Memory addressing modes (as opposed to Immediate/Relative/Implied).
[[nodiscard]] constexpr bool is_memory_mode(AddressingMode mode) noexcept
{
    switch (mode) {
    case AddressingMode::ZeroPage:
    case AddressingMode::ZeroPageX:
    case AddressingMode::ZeroPageY:
    case AddressingMode::Absolute:
    case AddressingMode::AbsoluteX:
    case AddressingMode::AbsoluteY:
    case AddressingMode::Indirect:
    case AddressingMode::IndirectX:
    case AddressingMode::IndirectY:
        return true;
    default:
        return false;
    }
}

/// True when the address calculation adds an index register. These are the
/// modes that can cross a page boundary, which on real hardware costs an
/// extra cycle for reads.
[[nodiscard]] constexpr bool is_indexed(AddressingMode mode) noexcept
{
    switch (mode) {
    case AddressingMode::ZeroPageX:
    case AddressingMode::ZeroPageY:
    case AddressingMode::AbsoluteX:
    case AddressingMode::AbsoluteY:
    case AddressingMode::IndirectX:
    case AddressingMode::IndirectY:
        return true;
    default:
        return false;
    }
}

[[nodiscard]] constexpr const char* mode_name(AddressingMode mode) noexcept
{
    switch (mode) {
    case AddressingMode::Implied:     return "implied";
    case AddressingMode::Accumulator: return "accumulator";
    case AddressingMode::Immediate:   return "immediate";
    case AddressingMode::ZeroPage:    return "zero page";
    case AddressingMode::ZeroPageX:   return "zero page,X";
    case AddressingMode::ZeroPageY:   return "zero page,Y";
    case AddressingMode::Absolute:    return "absolute";
    case AddressingMode::AbsoluteX:   return "absolute,X";
    case AddressingMode::AbsoluteY:   return "absolute,Y";
    case AddressingMode::Indirect:    return "indirect";
    case AddressingMode::IndirectX:   return "indirect,X";
    case AddressingMode::IndirectY:   return "indirect,Y";
    case AddressingMode::Relative:    return "relative";
    case AddressingMode::Unknown:     return "unknown";
    }
    return "unknown";
}

/// Everything the emulator knows about one opcode byte.
struct OpcodeInfo {
    Operation op;
    AddressingMode mode;

    /// Total instruction size in bytes, opcode included.
    [[nodiscard]] constexpr int length() const noexcept
    {
        return 1 + operand_length(mode);
    }

    /// False for the 105 codes the official 6502 does not define.
    [[nodiscard]] constexpr bool is_legal() const noexcept
    {
        return op != Operation::Unknown;
    }

    [[nodiscard]] constexpr const char* mnemonic() const noexcept
    {
        return operation_name(op);
    }
};

/// Number of defined opcodes on the official NMOS 6502.
inline constexpr int kLegalOpcodeCount = 151;

/// Number of mnemonics.
inline constexpr int kOperationCount = 56;

/// Look up one opcode. Never fails: illegal opcodes return Operation::Unknown.
[[nodiscard]] const OpcodeInfo& opcode_info(u8 opcode) noexcept;

// ---------------------------------------------------------------------------
// Timing
//
// The 6502 datasheet gives a fixed cycle count per opcode. That count already
// includes everything the instruction always pays: the opcode fetch, the
// operand fetches, the memory access, and the read-modify-write write-back.
//
// Two things are NOT included, because they depend on the data:
//
//   1. Page crossing. An indexed READ that carries into a new page takes one
//      extra cycle, because the address has to be fixed up.
//   2. A branch that is taken takes one extra cycle, and one more if the jump
//      crosses a page.
//
// Everything else is a constant, and that constant is this table.
// ---------------------------------------------------------------------------

/// Base cycle count for one opcode, from the datasheet.
[[nodiscard]] u8 opcode_cycles(u8 opcode) noexcept;

// ---------------------------------------------------------------------------
// Behavioural classes of operations, used by the CPU and by cycle accounting.
// ---------------------------------------------------------------------------

/// Instructions whose operand is an address they WRITE to.
[[nodiscard]] constexpr bool is_store_operation(Operation op) noexcept
{
    return op == Operation::STA || op == Operation::STX || op == Operation::STY;
}

/// Read-modify-write: the old value is read, changed, and written back.
/// These cost two extra cycles on real hardware.
[[nodiscard]] constexpr bool is_read_modify_write(Operation op) noexcept
{
    switch (op) {
    case Operation::ASL:
    case Operation::LSR:
    case Operation::ROL:
    case Operation::ROR:
    case Operation::INC:
    case Operation::DEC:
        return true;
    default:
        return false;
    }
}

[[nodiscard]] constexpr bool is_branch_operation(Operation op) noexcept
{
    switch (op) {
    case Operation::BCC:
    case Operation::BCS:
    case Operation::BEQ:
    case Operation::BMI:
    case Operation::BNE:
    case Operation::BPL:
    case Operation::BVC:
    case Operation::BVS:
        return true;
    default:
        return false;
    }
}

/// True when this (operation, mode) pair pays the extra page-crossing cycle.
///
/// Only the three indexed modes can cross a page at all, and only for READS.
/// Writes and read-modify-writes pay a fixed, higher price instead, which the
/// datasheet cycle count already includes.
[[nodiscard]] constexpr bool pays_page_penalty(Operation op, AddressingMode mode) noexcept
{
    if (mode != AddressingMode::AbsoluteX &&
        mode != AddressingMode::AbsoluteY &&
        mode != AddressingMode::IndirectY) {
        return false;
    }
    return !is_store_operation(op) && !is_read_modify_write(op);
}

} // namespace fc
