#pragma once

// ---------------------------------------------------------------------------
// Disassembler - bytes back into assembly text.
//
// An assembler goes   LDA #$42   ->  A9 42
// A disassembler goes A9 42      ->  LDA #$42
//
// The disassembler is not decoration. It is the tool you will use to debug
// real NES ROMs in Phase 3-4: when a game does something strange, you dump the
// surrounding instructions and read what the programmer actually wrote.
//
// Note the syntax: the output uses the 6502 convention `$42` for hex, not the
// C++ `0x42`. They are different languages and they get different prefixes.
// ---------------------------------------------------------------------------

#include "core/bus.hpp"
#include "core/cpu/opcode.hpp"
#include "core/types.hpp"

#include <span>
#include <string>

namespace fc {

/// One decoded instruction, plus the text a human would read.
struct DecodedInstruction {
    u16  address   = 0;                        // where it was decoded from
    u8   opcode    = 0;
    u8   operand_lo = 0;                       // first operand byte, if any
    u8   operand_hi = 0;                       // second operand byte, if any
    OpcodeInfo info { Operation::Unknown, AddressingMode::Unknown };
    int  length    = 1;                        // total bytes, opcode included
    bool truncated = false;                    // ran off the end of the buffer
    std::string text;                          // "LDA #$42"

    // Relative branches resolve to an absolute target; everything else does
    // not address anything directly.
    bool has_target = false;
    u16  target     = 0;

    [[nodiscard]] bool is_legal() const noexcept { return info.is_legal(); }
};

/// Decode the instruction at the start of `bytes`.
///
/// `address` is the address those bytes live at. It matters for relative
/// branches, because a branch target is computed from where the instruction
/// ends. Pass the real address or branch targets will be wrong.
[[nodiscard]] DecodedInstruction disassemble(std::span<const u8> bytes, u16 address);

/// Decode straight from the bus, the way a debugger would.
[[nodiscard]] DecodedInstruction disassemble(Bus& bus, u16 address);

/// "A9 42" - the raw bytes of an instruction, no prefix, uppercase.
[[nodiscard]] std::string bytes_to_string(std::span<const u8> bytes, int count);

/// "$42" - 6502 assembly style hex, always two digits.
[[nodiscard]] std::string hex8(u8 value);

/// "$8000" - 6502 assembly style hex, always four digits.
[[nodiscard]] std::string hex16(u16 value);

} // namespace fc
