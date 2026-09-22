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

AddressingRequest request(AddressingMode mode, u8 lo, u8 hi = 0,
                          u8 x = 0, u8 y = 0, u16 pc = 0x8000)
{
    AddressingRequest r{};
    r.mode = mode;
    r.operand_lo = lo;
    r.operand_hi = hi;
    r.x = x;
    r.y = y;
    r.instruction_pc = pc;
    return r;
}

/// A machine for the end to end tests.
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

} // namespace

// ===========================================================================
// Modes with no address at all
// ===========================================================================

TEST(Addressing, ImpliedAndAccumulatorHaveNoOperand)
{
    FlatBus bus;

    EXPECT_TRUE(resolve(request(AddressingMode::Implied, 0x00), bus).is_none());
    EXPECT_TRUE(resolve(request(AddressingMode::Accumulator, 0x00), bus).is_none());
}

TEST(Addressing, ImmediateCarriesTheValueNotAnAddress)
{
    FlatBus bus;
    const auto op = resolve(request(AddressingMode::Immediate, 0x42), bus);

    ASSERT_TRUE(op.is_value());
    EXPECT_EQ(op.value, 0x42);
    EXPECT_EQ(op.address, 0) << "an immediate operand never has an address";
}

// ===========================================================================
// Zero page: the high byte is hard wired to zero
// ===========================================================================

TEST(Addressing, ZeroPageIsAlwaysInPageZero)
{
    FlatBus bus;
    const auto op = resolve(request(AddressingMode::ZeroPage, 0xFF), bus);

    ASSERT_TRUE(op.is_address());
    EXPECT_EQ(op.address, 0x00FF);
}

TEST(Addressing, ZeroPageIndexedWrapsInsidePageZero)
{
    FlatBus bus;

    // $42 + $C0 = $0102, but the high byte cannot carry out of page 0.
    const auto op = resolve(request(AddressingMode::ZeroPageX, 0x42, 0, 0xC0), bus);

    EXPECT_EQ(op.address, 0x0002);
    EXPECT_NE(op.address, 0x0102) << "this is the classic zero page wrap";
}

TEST(Addressing, ZeroPageIndexedWrapsFromFF)
{
    FlatBus bus;

    // $FF + $01 = $0100 -> $00
    const auto op = resolve(request(AddressingMode::ZeroPageX, 0xFF, 0, 0x01), bus);
    EXPECT_EQ(op.address, 0x0000);

    // $F0 + $20 = $0110 -> $10
    const auto op2 = resolve(request(AddressingMode::ZeroPageY, 0xF0, 0, 0, 0x20), bus);
    EXPECT_EQ(op2.address, 0x0010);
}

TEST(Addressing, ZeroPageIndexedNeverCrossesAPage)
{
    FlatBus bus;

    // Whatever the index, the result must stay below $0100.
    for (int base = 0; base <= 0xFF; ++base) {
        for (int index = 0; index <= 0xFF; ++index) {
            const auto op = resolve(
                request(AddressingMode::ZeroPageX, static_cast<u8>(base), 0,
                        static_cast<u8>(index)),
                bus);
            EXPECT_LT(op.address, 0x0100);
            EXPECT_EQ(op.address, static_cast<u8>(base + index));
            EXPECT_FALSE(op.page_crossed) << "page 0 has no page to cross";
        }
    }
}

// ===========================================================================
// Absolute
// ===========================================================================

TEST(Addressing, AbsoluteUsesBothOperandBytesLittleEndian)
{
    FlatBus bus;

    const auto op = resolve(request(AddressingMode::Absolute, 0x34, 0x12), bus);
    EXPECT_EQ(op.address, 0x1234);
    EXPECT_NE(op.address, 0x3412) << "swapped operand order is a classic bug";
}

TEST(Addressing, AbsoluteIndexedAddsTheIndexRegister)
{
    FlatBus bus;

    const auto op = resolve(request(AddressingMode::AbsoluteX, 0x00, 0x02, 0x05), bus);
    EXPECT_EQ(op.address, 0x0205);
    EXPECT_FALSE(op.page_crossed);

    const auto op2 = resolve(request(AddressingMode::AbsoluteY, 0x00, 0x02, 0, 0x07), bus);
    EXPECT_EQ(op2.address, 0x0207);
}

TEST(Addressing, AbsoluteIndexedReportsPageCrossing)
{
    FlatBus bus;

    // $02FF + 1 crosses into page 3.
    const auto crossing = resolve(request(AddressingMode::AbsoluteX, 0xFF, 0x02, 0x01), bus);
    EXPECT_EQ(crossing.address, 0x0300);
    EXPECT_TRUE(crossing.page_crossed);

    // $0200 + $FF = $02FF, still page 2.
    const auto staying = resolve(request(AddressingMode::AbsoluteX, 0x00, 0x02, 0xFF), bus);
    EXPECT_EQ(staying.address, 0x02FF);
    EXPECT_FALSE(staying.page_crossed);
}

TEST(Addressing, AbsoluteIndexedWrapsAtSixtyFourK)
{
    FlatBus bus;

    // $FFFF + 2 wraps to $0001.
    const auto op = resolve(request(AddressingMode::AbsoluteX, 0xFF, 0xFF, 0x02), bus);
    EXPECT_EQ(op.address, 0x0001);
    EXPECT_TRUE(op.page_crossed);
}

// ===========================================================================
// Indirect: the JMP page bug
// ===========================================================================

TEST(Addressing, IndirectReadsAFullPointerAcrossPages)
{
    FlatBus bus;
    bus.write(0x1000, 0x34);
    bus.write(0x1001, 0x12);

    EXPECT_EQ(read_pointer_indirect(bus, 0x1000), 0x1234);
    EXPECT_EQ(resolve(request(AddressingMode::Indirect, 0x00, 0x10), bus).address, 0x1234);
}

TEST(Addressing, IndirectPointerAtPageEndHasThe6502Bug)
{
    FlatBus bus;
    bus.write(0x10FF, 0x34);   // low byte, read from $10FF
    bus.write(0x1100, 0x99);   // the "correct" high byte - the chip IGNORES this
    bus.write(0x1000, 0x12);   // the high byte is actually read from $1000

    const u16 target = read_pointer_indirect(bus, 0x10FF);

    EXPECT_EQ(target, 0x1234) << "the 6502 reads the high byte from the same page";
    EXPECT_NE(target, 0x9934) << "reading $1100 would be the 'fixed' behaviour";
}

// ===========================================================================
// Indirect zero page
// ===========================================================================

TEST(Addressing, IndirectXIndexesThePointerBeforeDereferencing)
{
    FlatBus bus;

    // pointer = ($42 + X) & 0xFF, then the 16 bit value at [$00pointer].
    bus.write(0x0044, 0x00);
    bus.write(0x0045, 0x02);

    const auto op = resolve(request(AddressingMode::IndirectX, 0x42, 0, 0x02), bus);

    ASSERT_TRUE(op.is_address());
    EXPECT_EQ(op.address, 0x0200);
}

TEST(Addressing, IndirectXPointerWrapsInsidePageZero)
{
    FlatBus bus;

    // pointer = $FF + 0 = $FF. Low byte from $00FF, high byte from $0000.
    bus.write(0x00FF, 0x34);
    bus.write(0x0000, 0x12);
    bus.write(0x0100, 0x99);   // must not be used

    const auto op = resolve(request(AddressingMode::IndirectX, 0xFF), bus);
    EXPECT_EQ(op.address, 0x1234);
}

TEST(Addressing, IndirectYDereferencesBeforeIndexing)
{
    FlatBus bus;

    bus.write(0x0042, 0x00);
    bus.write(0x0043, 0x02);

    const auto op = resolve(request(AddressingMode::IndirectY, 0x42, 0, 0, 0x05), bus);

    ASSERT_TRUE(op.is_address());
    EXPECT_EQ(op.address, 0x0205) << "base $0200 from the pointer, plus Y = 5";
}

TEST(Addressing, IndirectYReportsPageCrossingOnTheFinalAddition)
{
    FlatBus bus;

    bus.write(0x0042, 0xFF);
    bus.write(0x0043, 0x20);   // base = $20FF

    const auto crossing = resolve(request(AddressingMode::IndirectY, 0x42, 0, 0, 0x01), bus);
    EXPECT_EQ(crossing.address, 0x2100);
    EXPECT_TRUE(crossing.page_crossed);

    const auto staying = resolve(request(AddressingMode::IndirectY, 0x42, 0, 0, 0x00), bus);
    EXPECT_EQ(staying.address, 0x20FF);
    EXPECT_FALSE(staying.page_crossed);
}

// ===========================================================================
// Relative
// ===========================================================================

TEST(Addressing, RelativeProducesATargetNotAnAddress)
{
    FlatBus bus;
    const auto op = resolve(request(AddressingMode::Relative, 0x05, 0, 0, 0, 0x8000), bus);

    ASSERT_TRUE(op.is_target());
    EXPECT_EQ(op.address, 0x8007) << "0x8000 + 2 + 5";
}

TEST(Addressing, RelativeUsesASignedOffset)
{
    FlatBus bus;

    // 0xFB is -5, not +251.
    const auto backwards = resolve(request(AddressingMode::Relative, 0xFB, 0, 0, 0, 0x8000), bus);
    EXPECT_EQ(backwards.address, 0x7FFD);
    EXPECT_NE(backwards.address, 0x80F8);

    // 0x80 is -128.
    const auto furthest = resolve(request(AddressingMode::Relative, 0x80, 0, 0, 0, 0x8000), bus);
    EXPECT_EQ(furthest.address, 0x7F82);

    // 0x7F is +127.
    const auto forward = resolve(request(AddressingMode::Relative, 0x7F, 0, 0, 0, 0x8000), bus);
    EXPECT_EQ(forward.address, 0x8081);
}

TEST(Addressing, RelativeTargetWrapsAtSixtyFourK)
{
    FlatBus bus;
    const auto op = resolve(request(AddressingMode::Relative, 0x00, 0, 0, 0, 0xFFFE), bus);
    EXPECT_EQ(op.address, 0x0000);
}

// ===========================================================================
// Cycle base costs
// ===========================================================================

TEST(Addressing, BaseCycleCostsMatchTheDatasheet)
{
    EXPECT_EQ(addressing_cycles(AddressingMode::Implied), 2);
    EXPECT_EQ(addressing_cycles(AddressingMode::Accumulator), 2);
    EXPECT_EQ(addressing_cycles(AddressingMode::Immediate), 2);
    EXPECT_EQ(addressing_cycles(AddressingMode::ZeroPage), 3);
    EXPECT_EQ(addressing_cycles(AddressingMode::ZeroPageX), 4);
    EXPECT_EQ(addressing_cycles(AddressingMode::ZeroPageY), 4);
    EXPECT_EQ(addressing_cycles(AddressingMode::Absolute), 4);
    EXPECT_EQ(addressing_cycles(AddressingMode::AbsoluteX), 4);
    EXPECT_EQ(addressing_cycles(AddressingMode::AbsoluteY), 4);
    EXPECT_EQ(addressing_cycles(AddressingMode::Indirect), 5);
    EXPECT_EQ(addressing_cycles(AddressingMode::IndirectX), 6);
    EXPECT_EQ(addressing_cycles(AddressingMode::IndirectY), 5);
    EXPECT_EQ(addressing_cycles(AddressingMode::Relative), 2);
}

// ===========================================================================
// End to end: the CPU really reads and writes at the resolved address
// ===========================================================================

TEST(AddressingCpu, LdaZeroPageXReadsTheWrappedAddress)
{
    Machine m;
    m.load({ 0xB5, 0xF0,   // LDA $F0,X
             0x00 });
    m.cpu.registers().x = 0x20;

    // $F0 + $20 = $0110, but it wraps to $0010.
    m.bus.write(0x0010, 0x5A);
    m.bus.write(0x0110, 0xFF);   // must be ignored

    m.cpu.step();

    EXPECT_EQ(m.cpu.registers().a, 0x5A);
}

TEST(AddressingCpu, LdaAbsoluteXReadsBasePlusIndex)
{
    Machine m;
    m.load({ 0xBD, 0x00, 0x02 });   // LDA $0200,X
    m.cpu.registers().x = 0x03;
    m.bus.write(0x0203, 0x77);

    m.cpu.step();

    EXPECT_EQ(m.cpu.registers().a, 0x77);
}

TEST(AddressingCpu, StaAbsoluteXWritesBasePlusIndex)
{
    Machine m;
    m.load({ 0x9D, 0x00, 0x02 });   // STA $0200,X
    m.cpu.registers().a = 0x99;
    m.cpu.registers().x = 0x10;

    m.cpu.step();

    EXPECT_EQ(m.bus.read(0x0210), 0x99);
    EXPECT_EQ(m.bus.read(0x0200), 0x00) << "only the indexed address is written";
}

TEST(AddressingCpu, LdaIndirectYReadsThroughThePointer)
{
    Machine m;
    m.load({ 0xB1, 0x42 });        // LDA ($42),Y
    m.cpu.registers().y = 0x02;

    m.bus.write(0x0042, 0x00);     // pointer -> $0300
    m.bus.write(0x0043, 0x03);
    m.bus.write(0x0302, 0xAB);

    m.cpu.step();

    EXPECT_EQ(m.cpu.registers().a, 0xAB);
}

TEST(AddressingCpu, LdaIndirectXIndexesThePointerFirst)
{
    Machine m;
    m.load({ 0xA1, 0x40 });        // LDA ($40,X)
    m.cpu.registers().x = 0x02;    // pointer slot is $42

    m.bus.write(0x0042, 0x00);
    m.bus.write(0x0043, 0x04);     // -> $0400
    m.bus.write(0x0400, 0xCD);

    m.cpu.step();

    EXPECT_EQ(m.cpu.registers().a, 0xCD);
}

TEST(AddressingCpu, LdxZeroPageY)
{
    Machine m;
    m.load({ 0xB6, 0x10 });        // LDX $10,Y
    m.cpu.registers().y = 0x05;
    m.bus.write(0x0015, 0x42);

    m.cpu.step();

    EXPECT_EQ(m.cpu.registers().x, 0x42);
}

TEST(AddressingCpu, LdaImmediateIgnoresMemory)
{
    Machine m;
    m.load({ 0xA9, 0x42 });

    // Put something else at address 0x0042 - it must not matter.
    m.bus.write(0x0042, 0xFF);

    m.cpu.step();

    EXPECT_EQ(m.cpu.registers().a, 0x42);
}

TEST(AddressingCpu, JmpIndirectFollowsThePointer)
{
    Machine m;
    m.load({ 0x6C, 0x00, 0x30 });   // JMP ($3000)

    m.bus.write(0x3000, 0x00);
    m.bus.write(0x3001, 0x90);      // -> $9000

    m.cpu.step();

    EXPECT_EQ(m.cpu.registers().pc, 0x9000);
}

TEST(AddressingCpu, JmpIndirectReproducesThePageBug)
{
    Machine m;
    m.load({ 0x6C, 0xFF, 0x30 });   // JMP ($30FF)

    m.bus.write(0x30FF, 0x00);
    m.bus.write(0x3100, 0x99);      // ignored by real hardware
    m.bus.write(0x3000, 0x90);      // actually used

    m.cpu.step();

    EXPECT_EQ(m.cpu.registers().pc, 0x9000);
}

// ===========================================================================
// Branches take or do not take
// ===========================================================================

TEST(AddressingCpu, BranchIsTakenWhenTheFlagIsSet)
{
    Machine m;
    m.load({ 0xD0, 0x02 });   // BNE +2  -> $8004
    m.cpu.registers().set_flag(Flag::Zero, false);

    const int cycles = m.cpu.step();

    EXPECT_EQ(m.cpu.registers().pc, 0x8004);
    EXPECT_EQ(cycles, 3) << "2 base + 1 for taking the branch";
}

TEST(AddressingCpu, BranchIsNotTakenWhenTheFlagIsClear)
{
    Machine m;
    m.load({ 0xD0, 0x02 });   // BNE +2
    m.cpu.registers().set_flag(Flag::Zero, true);

    const int cycles = m.cpu.step();

    EXPECT_EQ(m.cpu.registers().pc, 0x8002) << "PC only moved past the instruction";
    EXPECT_EQ(cycles, 2);
}

TEST(AddressingCpu, BranchAcrossAPageCostsOneMoreCycle)
{
    Machine m;
    m.load({ 0xD0, 0x10 }, 0x80F0);   // BNE +16 -> $8102, crossing from page $80

    m.cpu.registers().set_flag(Flag::Zero, false);

    const int cycles = m.cpu.step();

    EXPECT_EQ(m.cpu.registers().pc, 0x8102);
    EXPECT_EQ(cycles, 4) << "2 base + 1 taken + 1 page crossing";
}

// ===========================================================================
// Read-modify-write really writes back
// ===========================================================================

TEST(AddressingCpu, AslAccumulatorShiftsInPlace)
{
    Machine m;
    m.load({ 0x0A });              // ASL A
    m.cpu.registers().a = 0x81;

    m.cpu.step();

    EXPECT_EQ(m.cpu.registers().a, 0x02);
    EXPECT_TRUE(m.cpu.registers().flag(Flag::Carry)) << "bit 7 fell out into C";
    EXPECT_FALSE(m.cpu.registers().flag(Flag::Zero));
}

TEST(AddressingCpu, AslAbsoluteModifiesMemory)
{
    Machine m;
    m.load({ 0x0E, 0x00, 0x02 });   // ASL $0200
    m.bus.write(0x0200, 0x40);

    m.cpu.step();

    EXPECT_EQ(m.bus.read(0x0200), 0x80) << "the result must be written back";
    EXPECT_FALSE(m.cpu.registers().flag(Flag::Carry));
    EXPECT_TRUE(m.cpu.registers().flag(Flag::Negative));
}

TEST(AddressingCpu, RorRotatesTheCarryBitIn)
{
    Machine m;
    m.load({ 0x6A });              // ROR A
    m.cpu.registers().a = 0x02;
    m.cpu.registers().set_flag(Flag::Carry, true);

    m.cpu.step();

    EXPECT_EQ(m.cpu.registers().a, 0x81) << "old carry becomes bit 7";
    EXPECT_FALSE(m.cpu.registers().flag(Flag::Carry)) << "bit 0 fell out into C";
}

// ===========================================================================
// A whole program using indexed addressing
// ===========================================================================

TEST(AddressingCpu, CopiesAnArrayUsingIndexedAddressing)
{
    // Copy 4 bytes from $0300 to $0200, one at a time:
    //
    //      LDX #$00
    // loop LDA $0300,X
    //      STA $0200,X
    //      INX
    //      CPX #$04
    //      BNE loop
    Machine m;
    m.load({
        0xA2, 0x00,        // LDX #$00
        0xBD, 0x00, 0x03,  // LDA $0300,X
        0x9D, 0x00, 0x02,  // STA $0200,X
        0xE8,              // INX
        0xE0, 0x04,        // CPX #$04
        0xD0, 0xF5,        // BNE -11 -> back to loop at $8002
    });

    m.bus.write(0x0300, 0x11);
    m.bus.write(0x0301, 0x22);
    m.bus.write(0x0302, 0x33);
    m.bus.write(0x0303, 0x44);

    m.cpu.run(100);

    EXPECT_EQ(m.bus.read(0x0200), 0x11);
    EXPECT_EQ(m.bus.read(0x0201), 0x22);
    EXPECT_EQ(m.bus.read(0x0202), 0x33);
    EXPECT_EQ(m.bus.read(0x0203), 0x44);
    EXPECT_EQ(m.cpu.registers().x, 0x04);
}
