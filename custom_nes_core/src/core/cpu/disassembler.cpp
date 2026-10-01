#include "core/cpu/disassembler.hpp"

#include "core/bit.hpp"

namespace fc {
namespace {

constexpr char kHexDigits[] = "0123456789ABCDEF";

char hex_digit(u8 nibble)
{
    return kHexDigits[nibble & 0x0F];
}

/// Compute a branch target.
///
/// A branch operand is a SIGNED 8 bit offset counted from the address of the
/// *next* instruction:
///
///     target = address_of_branch + 2 + (signed)offset
///
/// The `+ 2` is because the branch is 2 bytes long: by the time the branch is
/// taken, PC has already moved past the operands.
///
/// The offset is two's complement, so 0xFB means -5, not +251.
/// See docs/computer-science/twos-complement.md.
[[nodiscard]] u16 branch_target(u16 address, u8 offset) noexcept
{
    const int signed_offset = static_cast<int>(bit::as_signed(offset));
    const int next_instruction = static_cast<int>(address) + 2;
    return static_cast<u16>(next_instruction + signed_offset);
}

[[nodiscard]] std::string build_text(const DecodedInstruction& insn)
{
    const std::string name = insn.info.mnemonic();

    switch (insn.info.mode) {
    case AddressingMode::Implied:
        return name;

    case AddressingMode::Accumulator:
        return name + " A";

    case AddressingMode::Immediate:
        return name + " #" + hex8(insn.operand_lo);

    case AddressingMode::ZeroPage:
        return name + " " + hex8(insn.operand_lo);

    case AddressingMode::ZeroPageX:
        return name + " " + hex8(insn.operand_lo) + ",X";

    case AddressingMode::ZeroPageY:
        return name + " " + hex8(insn.operand_lo) + ",Y";

    case AddressingMode::Absolute:
        return name + " " + hex16(bit::make_u16(insn.operand_lo, insn.operand_hi));

    case AddressingMode::AbsoluteX:
        return name + " " + hex16(bit::make_u16(insn.operand_lo, insn.operand_hi)) + ",X";

    case AddressingMode::AbsoluteY:
        return name + " " + hex16(bit::make_u16(insn.operand_lo, insn.operand_hi)) + ",Y";

    case AddressingMode::Indirect:
        return name + " (" + hex16(bit::make_u16(insn.operand_lo, insn.operand_hi)) + ")";

    case AddressingMode::IndirectX:
        return name + " (" + hex8(insn.operand_lo) + ",X)";

    case AddressingMode::IndirectY:
        return name + " (" + hex8(insn.operand_lo) + "),Y";

    case AddressingMode::Relative:
        // Show the resolved absolute target, which is what a human wants.
        return name + " " + hex16(insn.target);

    case AddressingMode::Unknown:
        return "???";
    }
    return "???";
}

} // namespace

// ---------------------------------------------------------------------------
// Public helpers
// ---------------------------------------------------------------------------

std::string hex8(u8 value)
{
    std::string out = "$";
    out.push_back(hex_digit(static_cast<u8>(value >> 4)));
    out.push_back(hex_digit(value));
    return out;
}

std::string hex16(u16 value)
{
    std::string out = "$";
    for (int shift = 12; shift >= 0; shift -= 4) {
        out.push_back(hex_digit(static_cast<u8>(value >> shift)));
    }
    return out;
}

std::string bytes_to_string(std::span<const u8> bytes, int count)
{
    std::string out;
    for (int i = 0; i < count; ++i) {
        if (i != 0) {
            out.push_back(' ');
        }
        if (static_cast<std::size_t>(i) < bytes.size()) {
            out.push_back(hex_digit(static_cast<u8>(bytes[static_cast<std::size_t>(i)] >> 4)));
            out.push_back(hex_digit(bytes[static_cast<std::size_t>(i)]));
        } else {
            out += "--";   // ran off the end of the buffer
        }
    }
    return out;
}

// ---------------------------------------------------------------------------
// Decoding
// ---------------------------------------------------------------------------

DecodedInstruction disassemble(std::span<const u8> bytes, u16 address)
{
    DecodedInstruction insn{};
    insn.address = address;

    if (bytes.empty()) {
        insn.info = OpcodeInfo{ Operation::Unknown, AddressingMode::Unknown };
        insn.length = 0;
        insn.truncated = true;
        insn.text = "<no more bytes>";
        return insn;
    }

    insn.opcode = bytes[0];
    insn.info = opcode_info(insn.opcode);
    insn.length = insn.info.length();

    // The operand bytes may be missing if the buffer ends mid-instruction.
    // Record that instead of reading out of bounds.
    if (insn.length > 1 && bytes.size() > 1) {
        insn.operand_lo = bytes[1];
    }
    if (insn.length > 2 && bytes.size() > 2) {
        insn.operand_hi = bytes[2];
    }
    insn.truncated = bytes.size() < static_cast<std::size_t>(insn.length);

    if (insn.info.mode == AddressingMode::Relative) {
        insn.has_target = true;
        insn.target = branch_target(address, insn.operand_lo);
    }

    insn.text = build_text(insn);
    return insn;
}

DecodedInstruction disassemble(Bus& bus, u16 address)
{
    // Peek at the opcode first to learn how many bytes we need.
    const u8 opcode = bus.read(address);
    const int length = opcode_info(opcode).length();

    // A 6502 address is 16 bit, so an instruction may straddle 0xFFFF -> 0x0000.
    u8 bytes[3] = { 0, 0, 0 };
    for (int i = 0; i < length; ++i) {
        bytes[i] = bus.read(static_cast<u16>(address + i));
    }

    return disassemble(std::span<const u8>(bytes, static_cast<std::size_t>(length)), address);
}

} // namespace fc
