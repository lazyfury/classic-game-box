#include "core/cpu/addressing.hpp"
#include "core/cpu/cpu.hpp"
#include "core/cpu/opcode.hpp"
#include "core/flat_bus.hpp"
#include "core/types.hpp"

#include <gtest/gtest.h>

#include <initializer_list>
#include <vector>

using namespace fc;

namespace {

struct Machine {
    FlatBus bus;
    Cpu cpu{ bus };

    void load(std::initializer_list<u8> program, u16 origin = 0x8000)
    {
        const std::vector<u8> bytes(program);
        bus.clear();
        bus.load(bytes, origin);
        bus.write(0xFFFC, static_cast<u8>(origin & 0x00FF));
        bus.write(0xFFFD, static_cast<u8>(origin >> 8));
        cpu.reset();
    }
};

/// Base cost of fetching an instruction with this addressing mode.
int base_for_mode(AddressingMode mode)
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

/// Reconstruct the datasheet cycle count from the operation class and the
/// addressing mode.
///
/// This is an INDEPENDENT encoding of the same knowledge as kCycleTable.
/// The two agreeing does not prove they match the silicon, but it does catch
/// transcriptions slips in either one - and a slip is by far the likeliest
/// failure mode for a table of 256 numbers.
int derived_cycles(Operation op, AddressingMode mode)
{
    const int base = base_for_mode(mode);

    if (is_store_operation(op)) {
        // A write happens on a fixed cycle, so the indexed absolute forms just
        // cost one more instead of sometimes costing one more.
        if (mode == AddressingMode::AbsoluteX ||
            mode == AddressingMode::AbsoluteY ||
            mode == AddressingMode::IndirectY) {
            return base + 1;
        }
        return base;
    }

    if (is_read_modify_write(op)) {
        if (mode == AddressingMode::Accumulator) {
            return base;   // ASL A is just a register operation
        }
        // Read, modify, write back. The indexed absolute forms pay one more
        // because the un-fixed-up address is needed for the write.
        if (mode == AddressingMode::AbsoluteX || mode == AddressingMode::AbsoluteY) {
            return base + 3;
        }
        return base + 2;
    }

    switch (op) {
    case Operation::PHA:
    case Operation::PHP:
        return 3;
    case Operation::PLA:
    case Operation::PLP:
        return 4;
    case Operation::JSR:
    case Operation::RTS:
    case Operation::RTI:
        return 6;
    case Operation::BRK:
        return 7;
    case Operation::JMP:
        return (mode == AddressingMode::Indirect) ? 5 : 3;
    default:
        break;
    }

    return base;
}

} // namespace

// ===========================================================================
// The table must agree with the derived model, everywhere
// ===========================================================================

TEST(CycleTable, MatchesTheDerivedModelForEveryLegalOpcode)
{
    int checked = 0;

    for (int i = 0; i < 256; ++i) {
        const u8 opcode = static_cast<u8>(i);
        const OpcodeInfo& info = opcode_info(opcode);
        if (!info.is_legal()) {
            continue;
        }

        EXPECT_EQ(opcode_cycles(opcode), derived_cycles(info.op, info.mode))
            << "opcode $" << std::hex << i << std::dec
            << " (" << info.mnemonic() << " " << mode_name(info.mode) << ")";
        ++checked;
    }

    EXPECT_EQ(checked, 151);
}

TEST(CycleTable, HeadlineValuesFromTheDatasheet)
{
    EXPECT_EQ(opcode_cycles(0xA9), 2);   // LDA #imm
    EXPECT_EQ(opcode_cycles(0xA5), 3);   // LDA zp
    EXPECT_EQ(opcode_cycles(0xB5), 4);   // LDA zp,X
    EXPECT_EQ(opcode_cycles(0xAD), 4);   // LDA abs
    EXPECT_EQ(opcode_cycles(0xBD), 4);   // LDA abs,X
    EXPECT_EQ(opcode_cycles(0xB1), 5);   // LDA (zp),Y
    EXPECT_EQ(opcode_cycles(0xA1), 6);   // LDA (zp,X)

    EXPECT_EQ(opcode_cycles(0x85), 3);   // STA zp
    EXPECT_EQ(opcode_cycles(0x95), 4);   // STA zp,X
    EXPECT_EQ(opcode_cycles(0x9D), 5);   // STA abs,X
    EXPECT_EQ(opcode_cycles(0x91), 6);   // STA (zp),Y

    EXPECT_EQ(opcode_cycles(0x0A), 2);   // ASL A
    EXPECT_EQ(opcode_cycles(0x06), 5);   // ASL zp
    EXPECT_EQ(opcode_cycles(0x0E), 6);   // ASL abs
    EXPECT_EQ(opcode_cycles(0x1E), 7);   // ASL abs,X

    EXPECT_EQ(opcode_cycles(0x4C), 3);   // JMP abs
    EXPECT_EQ(opcode_cycles(0x6C), 5);   // JMP (ind)
    EXPECT_EQ(opcode_cycles(0x20), 6);   // JSR
    EXPECT_EQ(opcode_cycles(0x60), 6);   // RTS
    EXPECT_EQ(opcode_cycles(0x40), 6);   // RTI
    EXPECT_EQ(opcode_cycles(0x00), 7);   // BRK

    EXPECT_EQ(opcode_cycles(0x48), 3);   // PHA
    EXPECT_EQ(opcode_cycles(0x68), 4);   // PLA
    EXPECT_EQ(opcode_cycles(0x08), 3);   // PHP
    EXPECT_EQ(opcode_cycles(0x28), 4);   // PLP

    EXPECT_EQ(opcode_cycles(0xEA), 2);   // NOP
    EXPECT_EQ(opcode_cycles(0xD0), 2);   // BNE (not taken)
}

// ===========================================================================
// Page crossing
// ===========================================================================

TEST(CycleTable, PageCrossingCostsOneExtraCycleOnReads)
{
    Machine m;
    m.load({ 0xBD, 0x00, 0x02 });   // LDA $0200,X
    m.cpu.registers().x = 0x00;
    EXPECT_EQ(m.cpu.step(), 4);

    Machine crossing;
    crossing.load({ 0xBD, 0xFF, 0x02 });   // LDA $02FF,X
    crossing.cpu.registers().x = 0x01;     // -> $0300
    EXPECT_EQ(crossing.cpu.step(), 5);
}

TEST(CycleTable, StoresPayTheSameWithOrWithoutAPageCrossing)
{
    Machine staying;
    staying.load({ 0x9D, 0x00, 0x02 });   // STA $0200,X
    staying.cpu.registers().x = 0x00;
    EXPECT_EQ(staying.cpu.step(), 5);

    Machine crossing;
    crossing.load({ 0x9D, 0xFF, 0x02 });  // STA $02FF,X
    crossing.cpu.registers().x = 0x01;    // -> $0300
    EXPECT_EQ(crossing.cpu.step(), 5) << "writes never vary";
}

TEST(CycleTable, IndirectYAlsoPaysForCrossing)
{
    Machine m;
    m.load({ 0xB1, 0x42 });      // LDA ($42),Y
    m.bus.write(0x0042, 0x00);
    m.bus.write(0x0043, 0x02);   // base $0200
    m.cpu.registers().y = 0x00;
    EXPECT_EQ(m.cpu.step(), 5);

    Machine crossing;
    crossing.load({ 0xB1, 0x42 });   // LDA ($42),Y
    crossing.bus.write(0x0042, 0xFF);
    crossing.bus.write(0x0043, 0x02);   // base $02FF
    crossing.cpu.registers().y = 0x01;  // -> $0300
    EXPECT_EQ(crossing.cpu.step(), 6);
}

TEST(CycleTable, ReadModifyWriteDoesNotGetTheCrossingDiscount)
{
    Machine m;
    m.load({ 0xFE, 0xFF, 0x02 });   // INC $02FF,X
    m.cpu.registers().x = 0x01;     // -> $0300, crosses
    EXPECT_EQ(m.cpu.step(), 7) << "already counted in the datasheet value";
}

TEST(CycleTable, ZeroPageIndexedNeverCrossesSoNeverCosts)
{
    Machine m;
    m.load({ 0xB5, 0xF0 });   // LDA $F0,X
    m.cpu.registers().x = 0x20;   // wraps to $0010, not $0110
    EXPECT_EQ(m.cpu.step(), 4);
}

// ===========================================================================
// Branches
// ===========================================================================

TEST(CycleTable, BranchCostsTwoThreeOrFour)
{
    // Not taken.
    {
        Machine m;
        m.load({ 0xD0, 0x02 });   // BNE
        m.cpu.registers().set_flag(Flag::Zero, true);
        EXPECT_EQ(m.cpu.step(), 2);
    }

    // Taken, same page.
    {
        Machine m;
        m.load({ 0xD0, 0x02 });
        m.cpu.registers().set_flag(Flag::Zero, false);
        EXPECT_EQ(m.cpu.step(), 3);
    }

    // Taken, across a page.
    {
        Machine m;
        m.load({ 0xD0, 0x10 }, 0x80F0);   // -> $8102
        m.cpu.registers().set_flag(Flag::Zero, false);
        EXPECT_EQ(m.cpu.step(), 4);
    }
}

// ===========================================================================
// Whole programs
// ===========================================================================

TEST(CycleTable, ProgramCyclesAddUp)
{
    Machine m;
    m.load({
        0xA9, 0x05,   // LDA #$05   2
        0x69, 0x03,   // ADC #$03   2
        0x85, 0x10,   // STA $10    3
        0xEA,         // NOP        2
    });

    m.cpu.run(4);

    EXPECT_EQ(m.cpu.total_cycles(), 2u + 2u + 3u + 2u);
    EXPECT_EQ(m.cpu.registers().a, 0x08);
}

TEST(CycleTable, InterruptCostsSevenCycles)
{
    Machine m;
    m.load({ 0xEA });
    m.bus.write(0xFFFE, 0x00);
    m.bus.write(0xFFFF, 0x90);

    m.cpu.registers().set_flag(Flag::IrqDisable, false);
    m.cpu.set_irq_line(true);

    EXPECT_EQ(m.cpu.step(), 7);
}

TEST(CycleTable, SubroutineCallAndReturnCostTwelve)
{
    Machine m;
    m.load({
        0x20, 0x04, 0x80,   // $8000: JSR $8004
        0xEA,               // $8003: return point
        0xEA,               // $8004: NOP   <- the subroutine
        0x60,               // $8005: RTS
    });

    EXPECT_EQ(m.cpu.step(), 6) << "JSR";
    EXPECT_EQ(m.cpu.registers().pc, 0x8004);

    EXPECT_EQ(m.cpu.step(), 2) << "NOP";

    EXPECT_EQ(m.cpu.step(), 6) << "RTS";
    EXPECT_EQ(m.cpu.registers().pc, 0x8003);

    EXPECT_EQ(m.cpu.total_cycles(), 14u);
}

// ===========================================================================
// The page penalty predicate itself
// ===========================================================================

TEST(CycleTable, OnlyIndexedReadsPayTheCrossingPenalty)
{
    // Reads: yes.
    EXPECT_TRUE(pays_page_penalty(Operation::LDA, AddressingMode::AbsoluteX));
    EXPECT_TRUE(pays_page_penalty(Operation::LDA, AddressingMode::AbsoluteY));
    EXPECT_TRUE(pays_page_penalty(Operation::LDA, AddressingMode::IndirectY));
    EXPECT_TRUE(pays_page_penalty(Operation::CMP, AddressingMode::AbsoluteX));
    EXPECT_TRUE(pays_page_penalty(Operation::ADC, AddressingMode::IndirectY));

    // Writes: no, they pay a fixed price instead.
    EXPECT_FALSE(pays_page_penalty(Operation::STA, AddressingMode::AbsoluteX));
    EXPECT_FALSE(pays_page_penalty(Operation::STX, AddressingMode::AbsoluteY));

    // Read-modify-write: same.
    EXPECT_FALSE(pays_page_penalty(Operation::INC, AddressingMode::AbsoluteX));
    EXPECT_FALSE(pays_page_penalty(Operation::ASL, AddressingMode::AbsoluteX));

    // Not indexed: nothing to cross.
    EXPECT_FALSE(pays_page_penalty(Operation::LDA, AddressingMode::Absolute));
    EXPECT_FALSE(pays_page_penalty(Operation::LDA, AddressingMode::ZeroPageX));
}
