#include "core/cpu/disassembler.hpp"
#include "core/cpu/opcode.hpp"
#include "core/flat_bus.hpp"
#include "core/types.hpp"

#include <gtest/gtest.h>

#include <array>
#include <initializer_list>
#include <vector>

using namespace fc;

namespace {

/// Decode a few bytes at a given address.
DecodedInstruction decode(std::initializer_list<u8> bytes, u16 address = 0x8000)
{
    const std::vector<u8> v(bytes);
    return disassemble(std::span<const u8>(v.data(), v.size()), address);
}

} // namespace

// ===========================================================================
// Assembler <-> disassembler: the same instruction in two notations
// ===========================================================================

TEST(Disassembler, KnownAssemblerEquivalences)
{
    struct Case {
        std::initializer_list<u8> bytes;
        const char* text;
        int length;
    };

    const Case cases[] = {
        { { 0xA9, 0x42 },        "LDA #$42",      2 },
        { { 0xA2, 0x10 },        "LDX #$10",      2 },
        { { 0xA0, 0xFF },        "LDY #$FF",      2 },
        { { 0xEA },              "NOP",           1 },
        { { 0xAA },              "TAX",           1 },
        { { 0x0A },              "ASL A",         1 },
        { { 0x4C, 0x00, 0x80 },  "JMP $8000",     3 },
        { { 0x6C, 0x00, 0x80 },  "JMP ($8000)",   3 },
        { { 0x20, 0x34, 0x12 },  "JSR $1234",     3 },
        { { 0x8D, 0x00, 0x02 },  "STA $0200",     3 },
        { { 0x9D, 0x00, 0x02 },  "STA $0200,X",   3 },
        { { 0x99, 0x00, 0x02 },  "STA $0200,Y",   3 },
        { { 0xA5, 0x42 },        "LDA $42",       2 },
        { { 0xB5, 0x42 },        "LDA $42,X",     2 },
        { { 0x96, 0x42 },        "STX $42,Y",     2 },
        { { 0xA1, 0x42 },        "LDA ($42,X)",   2 },
        { { 0xB1, 0x42 },        "LDA ($42),Y",   2 },
        { { 0xBE, 0x00, 0x80 },  "LDX $8000,Y",   3 },
        { { 0xBD, 0x00, 0x80 },  "LDA $8000,X",   3 },
        { { 0x00 },              "BRK",           1 },
    };

    for (const auto& c : cases) {
        const auto insn = decode(c.bytes);
        EXPECT_EQ(insn.text, c.text);
        EXPECT_EQ(insn.length, c.length);
        EXPECT_TRUE(insn.is_legal());
        EXPECT_FALSE(insn.truncated);
    }
}

TEST(Disassembler, EveryAddressingModeHasItsOwnSyntax)
{
    struct Case { u8 opcode; u8 lo; u8 hi; AddressingMode mode; const char* text; };

    const Case cases[] = {
        { 0xEA, 0x00, 0x00, AddressingMode::Implied,      "NOP" },
        { 0x0A, 0x00, 0x00, AddressingMode::Accumulator,  "ASL A" },
        { 0xA9, 0x42, 0x00, AddressingMode::Immediate,    "LDA #$42" },
        { 0xA5, 0x42, 0x00, AddressingMode::ZeroPage,     "LDA $42" },
        { 0xB5, 0x42, 0x00, AddressingMode::ZeroPageX,    "LDA $42,X" },
        { 0xB6, 0x42, 0x00, AddressingMode::ZeroPageY,    "LDX $42,Y" },
        { 0xAD, 0x34, 0x12, AddressingMode::Absolute,     "LDA $1234" },
        { 0xBD, 0x34, 0x12, AddressingMode::AbsoluteX,    "LDA $1234,X" },
        { 0xB9, 0x34, 0x12, AddressingMode::AbsoluteY,    "LDA $1234,Y" },
        { 0x6C, 0x34, 0x12, AddressingMode::Indirect,     "JMP ($1234)" },
        { 0xA1, 0x42, 0x00, AddressingMode::IndirectX,    "LDA ($42,X)" },
        { 0xB1, 0x42, 0x00, AddressingMode::IndirectY,    "LDA ($42),Y" },
    };

    for (const auto& c : cases) {
        const auto insn = decode({ c.opcode, c.lo, c.hi });
        EXPECT_EQ(insn.info.mode, c.mode) << "opcode " << hex8(c.opcode);
        EXPECT_EQ(insn.text, c.text);
    }
}

// ===========================================================================
// Relative branches: the signed offset from twos-complement.md
// ===========================================================================

TEST(Disassembler, BranchForwards)
{
    // F0 05 at $8000: target = $8000 + 2 + 5 = $8007
    const auto insn = decode({ 0xF0, 0x05 }, 0x8000);

    EXPECT_EQ(insn.info.mode, AddressingMode::Relative);
    EXPECT_EQ(insn.length, 2);
    EXPECT_TRUE(insn.has_target);
    EXPECT_EQ(insn.target, 0x8007);
    EXPECT_EQ(insn.text, "BEQ $8007");
}

TEST(Disassembler, BranchBackwardsUsesTwosComplement)
{
    // D0 FB at $8000: 0xFB is -5, target = $8000 + 2 - 5 = $7FFD
    const auto insn = decode({ 0xD0, 0xFB }, 0x8000);

    EXPECT_EQ(insn.target, 0x7FFD);
    EXPECT_EQ(insn.text, "BNE $7FFD");
}

TEST(Disassembler, BranchOffsetRangeIsSymmetric)
{
    // +127 goes forward 129 bytes, -128 goes back 126 bytes.
    EXPECT_EQ(decode({ 0x10, 0x7F }, 0x8000).target, 0x8081);
    EXPECT_EQ(decode({ 0x10, 0x80 }, 0x8000).target, 0x7F82);

    // If the offset were treated as unsigned, 0x80 would be +128, not -128.
    EXPECT_NE(decode({ 0x10, 0x80 }, 0x8000).target, 0x8082);
}

TEST(Disassembler, BranchTargetWrapsAtSixtyFourK)
{
    // 10 00 at $FFFE: the next instruction would be at $10000, which does not
    // exist on a 16 bit address bus, so the target wraps to $0000.
    EXPECT_EQ(decode({ 0x10, 0x00 }, 0xFFFE).target, 0x0000);

    // 10 FE at $FFFE: $10000 - 2 = $FFFE, so it jumps to itself.
    EXPECT_EQ(decode({ 0x10, 0xFE }, 0xFFFE).target, 0xFFFE);
}

TEST(Disassembler, AllEightBranchesAreRelative)
{
    const u8 branches[] = { 0x10, 0x30, 0x50, 0x70, 0x90, 0xB0, 0xD0, 0xF0 };
    for (u8 op : branches) {
        const auto insn = decode({ op, 0x02 }, 0x8000);
        EXPECT_EQ(insn.info.mode, AddressingMode::Relative) << "opcode " << hex8(op);
        EXPECT_EQ(insn.length, 2) << "opcode " << hex8(op);
        EXPECT_EQ(insn.target, 0x8004) << "opcode " << hex8(op);
    }
}

// ===========================================================================
// The opcode table follows strict patterns - verify them instead of retyping
// ===========================================================================

TEST(OpcodeTable, CoversAllTwoHundredFiftySixCodes)
{
    int legal = 0;
    for (int i = 0; i < 256; ++i) {
        const auto& info = opcode_info(static_cast<u8>(i));
        if (info.is_legal()) {
            ++legal;
            EXPECT_STRNE(info.mnemonic(), "???") << "opcode " << i;
        } else {
            EXPECT_STREQ(info.mnemonic(), "???") << "opcode " << i;
        }
    }

    // The official NMOS 6502 defines exactly 151 opcodes.
    EXPECT_EQ(legal, kLegalOpcodeCount);
    EXPECT_EQ(legal, 151);
}

TEST(OpcodeTable, LengthAlwaysMatchesTheAddressingMode)
{
    for (int i = 0; i < 256; ++i) {
        const auto& info = opcode_info(static_cast<u8>(i));
        EXPECT_EQ(info.length(), 1 + operand_length(info.mode))
            << "opcode " << i << " mode " << mode_name(info.mode);
    }
}

TEST(OpcodeTable, AluGroupFollowsTheColumnPattern)
{
    // The eight "read/modify the accumulator" instructions all share one
    // layout across the 16 columns:
    //
    //   +0x00 (zp,X)   +0x04 zp     +0x08 #imm    +0x0C abs
    //   +0x10 (zp),Y   +0x14 zp,X   +0x18 abs,Y   +0x1C abs,X
    const u8 bases[] = { 0x01, 0x21, 0x41, 0x61, 0x81, 0xA1, 0xC1, 0xE1 };

    for (u8 base : bases) {
        struct Pair { int offset; AddressingMode mode; };
        const Pair pairs[] = {
            { 0x00, AddressingMode::IndirectX },
            { 0x04, AddressingMode::ZeroPage  },
            { 0x08, AddressingMode::Immediate },
            { 0x0C, AddressingMode::Absolute  },
            { 0x10, AddressingMode::IndirectY },
            { 0x14, AddressingMode::ZeroPageX },
            { 0x18, AddressingMode::AbsoluteY },
            { 0x1C, AddressingMode::AbsoluteX },
        };

        for (const auto& p : pairs) {
            const u8 opcode = static_cast<u8>(base + p.offset);

            // STA has no immediate form: 0x89 is illegal.
            if (base == 0x81 && p.offset == 0x08) {
                EXPECT_FALSE(opcode_info(opcode).is_legal())
                    << "0x89 must be illegal: STA has no immediate mode";
                continue;
            }

            EXPECT_EQ(opcode_info(opcode).mode, p.mode)
                << "opcode " << hex8(opcode) << " (base " << hex8(base)
                << " + " << p.offset << ")";
        }
    }
}

TEST(OpcodeTable, ShiftGroupHasAnAccumulatorMode)
{
    // ASL/ROL/LSR/ROR: zp, A, abs, zp,X, abs,X
    const u8 bases[] = { 0x06, 0x26, 0x46, 0x66 };

    for (u8 base : bases) {
        EXPECT_EQ(opcode_info(static_cast<u8>(base + 0x00)).mode, AddressingMode::ZeroPage);
        EXPECT_EQ(opcode_info(static_cast<u8>(base + 0x04)).mode, AddressingMode::Accumulator);
        EXPECT_EQ(opcode_info(static_cast<u8>(base + 0x08)).mode, AddressingMode::Absolute);
        EXPECT_EQ(opcode_info(static_cast<u8>(base + 0x10)).mode, AddressingMode::ZeroPageX);
        EXPECT_EQ(opcode_info(static_cast<u8>(base + 0x18)).mode, AddressingMode::AbsoluteX);
    }
}

TEST(OpcodeTable, IncDecGroupHasNoAccumulatorMode)
{
    // INC/DEC have four modes only: zp, abs, zp,X, abs,X.
    // Note the offsets differ from the ALU group: there is no (zp,X) slot, so
    // zp,X sits at +0x10 rather than +0x14.
    struct Case { u8 base; const char* name; u8 implied_opcode; const char* implied_name; };

    const Case cases[] = {
        { 0xC6, "DEC", 0xCA, "DEX" },
        { 0xE6, "INC", 0xEA, "NOP" },
    };

    for (const auto& c : cases) {
        EXPECT_STREQ(opcode_info(static_cast<u8>(c.base + 0x00)).mnemonic(), c.name);
        EXPECT_EQ(opcode_info(static_cast<u8>(c.base + 0x00)).mode, AddressingMode::ZeroPage);
        EXPECT_EQ(opcode_info(static_cast<u8>(c.base + 0x08)).mode, AddressingMode::Absolute);
        EXPECT_EQ(opcode_info(static_cast<u8>(c.base + 0x10)).mode, AddressingMode::ZeroPageX);
        EXPECT_EQ(opcode_info(static_cast<u8>(c.base + 0x18)).mode, AddressingMode::AbsoluteX);

        // The +0x04 slot belongs to that row's implied column, NOT to DEC/INC.
        // There is no "DEC A" on the 6502.
        EXPECT_STREQ(opcode_info(c.implied_opcode).mnemonic(), c.implied_name);
        EXPECT_EQ(opcode_info(c.implied_opcode).mode, AddressingMode::Implied);

        // And the +0x14 slot is simply undefined for these rows.
        EXPECT_FALSE(opcode_info(static_cast<u8>(c.base + 0x14)).is_legal());
        EXPECT_STRNE(opcode_info(static_cast<u8>(c.base + 0x04)).mnemonic(), c.name);
    }
}

// ===========================================================================
// Illegal opcodes
// ===========================================================================

TEST(Disassembler, IllegalOpcodeIsReportedNotGuessed)
{
    // 0x02 is undefined on the official 6502.
    const auto insn = decode({ 0x02, 0xFF }, 0x8000);

    EXPECT_FALSE(insn.is_legal());
    EXPECT_EQ(insn.info.mode, AddressingMode::Unknown);
    EXPECT_EQ(insn.text, "???");
    EXPECT_EQ(insn.length, 1) << "we cannot know the length, so assume 1";
}

// ===========================================================================
// Truncated input must not read past the buffer
// ===========================================================================

TEST(Disassembler, TruncatedInstructionIsFlagged)
{
    // JMP absolute is 3 bytes; give it only 1.
    const auto insn = decode({ 0x4C }, 0x8000);

    EXPECT_TRUE(insn.truncated);
    EXPECT_EQ(insn.length, 3);
    EXPECT_EQ(insn.operand_lo, 0x00);
    EXPECT_EQ(insn.operand_hi, 0x00);
}

TEST(Disassembler, EmptyInputIsHandled)
{
    const std::vector<u8> none;
    const auto insn = disassemble(std::span<const u8>(none.data(), none.size()), 0x8000);

    EXPECT_TRUE(insn.truncated);
    EXPECT_EQ(insn.length, 0);
}

// ===========================================================================
// Reading straight from the bus
// ===========================================================================

TEST(Disassembler, DisassemblesFromTheBus)
{
    FlatBus bus;
    bus.write(0x8000, 0xBD);   // LDA $0200,X
    bus.write(0x8001, 0x00);
    bus.write(0x8002, 0x02);

    const auto insn = disassemble(bus, 0x8000);
    EXPECT_EQ(insn.text, "LDA $0200,X");
    EXPECT_EQ(insn.length, 3);
}

// ===========================================================================
// Walking a byte stream, the way a debugger listing works
// ===========================================================================

TEST(Disassembler, WalksAProgramInstructionByInstruction)
{
    const std::vector<u8> program = {
        0xA9, 0x42,        // LDA #$42
        0xAA,              // TAX
        0xBD, 0x00, 0x02,  // LDA $0200,X
        0x4C, 0x00, 0x80,  // JMP $8000
    };

    std::vector<std::string> listing;
    std::size_t offset = 0;
    while (offset < program.size()) {
        const auto insn = disassemble(
            std::span<const u8>(program.data() + offset, program.size() - offset),
            static_cast<u16>(0x8000 + offset));

        listing.push_back(insn.text);
        offset += static_cast<std::size_t>(insn.length);
    }

    ASSERT_EQ(listing.size(), 4u);
    EXPECT_EQ(listing[0], "LDA #$42");
    EXPECT_EQ(listing[1], "TAX");
    EXPECT_EQ(listing[2], "LDA $0200,X");
    EXPECT_EQ(listing[3], "JMP $8000");
}

// ===========================================================================
// Helpers
// ===========================================================================

TEST(Disassembler, HexFormattingUsesAssemblyStyle)
{
    EXPECT_EQ(hex8(0x00), "$00");
    EXPECT_EQ(hex8(0x42), "$42");
    EXPECT_EQ(hex8(0xFF), "$FF");
    EXPECT_EQ(hex16(0x0001), "$0001");
    EXPECT_EQ(hex16(0x8000), "$8000");
    EXPECT_EQ(hex16(0x1234), "$1234");
}

TEST(Disassembler, BytesToStringPadsMissingBytes)
{
    const std::vector<u8> bytes = { 0xA9, 0x42 };
    EXPECT_EQ(bytes_to_string(std::span<const u8>(bytes.data(), bytes.size()), 2), "A9 42");
    EXPECT_EQ(bytes_to_string(std::span<const u8>(bytes.data(), bytes.size()), 3), "A9 42 --");
}
