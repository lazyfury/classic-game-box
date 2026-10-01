#include "core/cpu/cpu.hpp"
#include "core/cpu/opcode.hpp"
#include "core/flat_bus.hpp"
#include "core/types.hpp"

#include <gtest/gtest.h>

#include <cstdint>
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

    void set_vector(u16 vector, u16 target)
    {
        bus.write(vector, static_cast<u8>(target & 0x00FF));
        bus.write(static_cast<u16>(vector + 1), static_cast<u8>(target >> 8));
    }
};

s8 as_signed(u8 value) { return static_cast<s8>(value); }

} // namespace

// ===========================================================================
// ADC / SBC
// ===========================================================================

TEST(InstructionSet, AdcAddsTwoPlusTwo)
{
    Machine m;
    m.load({ 0x69, 0x02 });   // ADC #$02
    m.cpu.registers().a = 0x02;

    m.cpu.step();

    const auto& r = m.cpu.registers();
    EXPECT_EQ(r.a, 0x04);
    EXPECT_FALSE(r.flag(Flag::Carry));
    EXPECT_FALSE(r.flag(Flag::Overflow));
    EXPECT_FALSE(r.flag(Flag::Zero));
    EXPECT_FALSE(r.flag(Flag::Negative));
}

TEST(InstructionSet, AdcIncludesTheCarryFlag)
{
    // 2 + 2 + carry is 4 or 5. ADC is always "add with carry in".
    Machine m;
    m.load({ 0x69, 0x02 });

    m.cpu.registers().a = 0x02;
    m.cpu.registers().set_flag(Flag::Carry, true);
    m.cpu.step();
    EXPECT_EQ(m.cpu.registers().a, 0x05) << "the carry flag really is an input";

    m.cpu.registers().a = 0x02;
    m.cpu.registers().set_flag(Flag::Carry, false);
    m.cpu.registers().pc = 0x8000;
    m.cpu.step();
    EXPECT_EQ(m.cpu.registers().a, 0x04);
}

TEST(InstructionSet, AdcMatchesWideIntegerArithmetic)
{
    // Every operand pair, every carry state, checked against exact integer
    // arithmetic. This is the ground truth the CPU has to reproduce.
    Machine m;
    m.load({ 0x69, 0x00 });   // ADC #imm, the operand byte is patched below

    for (int a = 0; a <= 0xFF; ++a) {
        for (int b = 0; b <= 0xFF; ++b) {
            for (int carry = 0; carry <= 1; ++carry) {
                m.bus.write(0x8001, static_cast<u8>(b));
                m.cpu.registers().pc = 0x8000;
                m.cpu.registers().a = static_cast<u8>(a);
                m.cpu.registers().set_flag(Flag::Carry, carry != 0);
                m.cpu.step();

                const int unsigned_sum = a + b + carry;
                const int signed_sum = as_signed(static_cast<u8>(a))
                                     + as_signed(static_cast<u8>(b)) + carry;

                const auto& r = m.cpu.registers();
                EXPECT_EQ(r.a, static_cast<u8>(unsigned_sum & 0xFF));
                EXPECT_EQ(r.flag(Flag::Carry), unsigned_sum > 0xFF);
                EXPECT_EQ(r.flag(Flag::Overflow), signed_sum < -128 || signed_sum > 127);
                EXPECT_EQ(r.flag(Flag::Zero), (unsigned_sum & 0xFF) == 0);
                EXPECT_EQ(r.flag(Flag::Negative), ((unsigned_sum & 0xFF) & 0x80) != 0);
            }
        }
    }
}

TEST(InstructionSet, SbcSubtractsWithBorrow)
{
    // SBC is A - M - (1 - C). With C = 1 there is no borrow in.
    Machine m;
    m.load({ 0xE9, 0x03 });   // SBC #$03

    m.cpu.registers().a = 0x05;
    m.cpu.registers().set_flag(Flag::Carry, true);
    m.cpu.step();

    EXPECT_EQ(m.cpu.registers().a, 0x02);
    EXPECT_TRUE(m.cpu.registers().flag(Flag::Carry)) << "no borrow -> C stays set";
}

TEST(InstructionSet, SbcSetsCarryToNoBorrow)
{
    Machine m;
    m.load({ 0xE9, 0x05 });   // SBC #$05

    m.cpu.registers().a = 0x03;
    m.cpu.registers().set_flag(Flag::Carry, true);
    m.cpu.step();

    EXPECT_EQ(m.cpu.registers().a, 0xFE) << "3 - 5 = -2";
    EXPECT_FALSE(m.cpu.registers().flag(Flag::Carry)) << "borrow -> C clear";
}

TEST(InstructionSet, SbcMatchesWideIntegerArithmetic)
{
    Machine m;
    m.load({ 0xE9, 0x00 });

    for (int a = 0; a <= 0xFF; ++a) {
        for (int b = 0; b <= 0xFF; ++b) {
            for (int carry = 0; carry <= 1; ++carry) {
                m.bus.write(0x8001, static_cast<u8>(b));
                m.cpu.registers().pc = 0x8000;
                m.cpu.registers().a = static_cast<u8>(a);
                m.cpu.registers().set_flag(Flag::Carry, carry != 0);
                m.cpu.step();

                const int difference = a - b - (1 - carry);
                const int signed_difference = as_signed(static_cast<u8>(a))
                                            - as_signed(static_cast<u8>(b))
                                            - (1 - carry);

                const auto& r = m.cpu.registers();
                EXPECT_EQ(r.a, static_cast<u8>(difference & 0xFF));
                EXPECT_EQ(r.flag(Flag::Carry), difference >= 0) << "no borrow";
                EXPECT_EQ(r.flag(Flag::Overflow),
                          signed_difference < -128 || signed_difference > 127);
            }
        }
    }
}

// ===========================================================================
// Logic
// ===========================================================================

TEST(InstructionSet, AndOraEorAreBitwise)
{
    struct Case { u8 opcode; u8 a; u8 m; u8 expect; };

    const Case cases[] = {
        { 0x29, 0xCA, 0xA6, static_cast<u8>(0xCA & 0xA6) },   // AND
        { 0x09, 0xCA, 0xA6, static_cast<u8>(0xCA | 0xA6) },   // ORA
        { 0x49, 0xCA, 0xA6, static_cast<u8>(0xCA ^ 0xA6) },   // EOR
    };

    for (const auto& c : cases) {
        Machine m;
        m.load({ c.opcode, c.m });
        m.cpu.registers().a = c.a;
        m.cpu.step();

        EXPECT_EQ(m.cpu.registers().a, c.expect)
            << "opcode " << static_cast<int>(c.opcode);
    }
}

TEST(InstructionSet, AndSetsZeroAndNegative)
{
    Machine m;
    m.load({ 0x29, 0x0F });   // AND #$0F
    m.cpu.registers().a = 0xF0;
    m.cpu.step();
    EXPECT_EQ(m.cpu.registers().a, 0x00);
    EXPECT_TRUE(m.cpu.registers().flag(Flag::Zero));

    m.cpu.registers().a = 0x80;
    m.cpu.registers().pc = 0x8000;
    m.cpu.step();
    EXPECT_EQ(m.cpu.registers().a, 0x00) << "0x80 & 0x0F is 0";
    EXPECT_TRUE(m.cpu.registers().flag(Flag::Zero));
}

TEST(InstructionSet, BitTestsWithoutChangingTheAccumulator)
{
    Machine m;
    m.load({ 0x24, 0x10 });   // BIT $10  (zero page)
    m.cpu.registers().a = 0x0F;
    m.bus.write(0x0010, 0xC0);   // bit 7 -> N, bit 6 -> V, A & M = 0 -> Z

    m.cpu.step();

    const auto& r = m.cpu.registers();
    EXPECT_EQ(r.a, 0x0F) << "BIT never touches A";
    EXPECT_TRUE(r.flag(Flag::Zero)) << "0x0F & 0xC0 == 0";
    EXPECT_TRUE(r.flag(Flag::Negative)) << "N comes from operand bit 7";
    EXPECT_TRUE(r.flag(Flag::Overflow)) << "V comes from operand bit 6";
}

TEST(InstructionSet, BitClearsZeroWhenBitsMatch)
{
    Machine m;
    m.load({ 0x24, 0x10 });
    m.cpu.registers().a = 0x01;
    m.bus.write(0x0010, 0x01);   // A & M != 0

    m.cpu.step();

    EXPECT_FALSE(m.cpu.registers().flag(Flag::Zero));
    EXPECT_FALSE(m.cpu.registers().flag(Flag::Negative));
    EXPECT_FALSE(m.cpu.registers().flag(Flag::Overflow));
}

// ===========================================================================
// The stack
// ===========================================================================

TEST(InstructionSet, PhaAndPlaRoundTrip)
{
    Machine m;
    m.load({
        0xA9, 0x42,   // LDA #$42
        0x48,         // PHA
        0xA9, 0x00,   // LDA #$00
        0x68,         // PLA
    });

    m.cpu.run(4);

    EXPECT_EQ(m.cpu.registers().a, 0x42) << "the byte came back";
    EXPECT_EQ(m.cpu.registers().sp, 0xFD) << "and the stack is balanced";
}

TEST(InstructionSet, PlaUpdatesTheFlags)
{
    Machine m;
    m.load({ 0x48, 0x68 });   // PHA, PLA
    m.cpu.registers().a = 0x80;

    m.cpu.step();   // PHA - PHA does not touch the flags
    m.cpu.registers().a = 0x00;
    m.cpu.step();   // PLA - this one does

    EXPECT_EQ(m.cpu.registers().a, 0x80);
    EXPECT_TRUE(m.cpu.registers().flag(Flag::Negative));
    EXPECT_FALSE(m.cpu.registers().flag(Flag::Zero));
}

TEST(InstructionSet, PhpPushesTheStatusWithBreakSet)
{
    Machine m;
    m.load({ 0x08 });   // PHP
    m.cpu.registers().set_flag(Flag::Carry, true);
    m.cpu.registers().set_flag(Flag::Zero, true);

    m.cpu.step();

    const u8 pushed = m.bus.read(0x01FD);
    EXPECT_TRUE((pushed & 0x01) != 0) << "C";
    EXPECT_TRUE((pushed & 0x02) != 0) << "Z";
    EXPECT_TRUE((pushed & 0x10) != 0) << "B is set when PHP pushes";
    EXPECT_TRUE((pushed & 0x20) != 0) << "the unused bit always reads as 1";
}

TEST(InstructionSet, PlpRestoresTheFlagsAndForcesTheUnusedBit)
{
    Machine m;
    m.load({ 0x28 });   // PLP

    m.cpu.push(0x00);   // a status byte with everything clear
    m.cpu.step();

    EXPECT_TRUE(m.cpu.registers().flag(Flag::Unused)) << "bit 5 is always 1";
    EXPECT_FALSE(m.cpu.registers().flag(Flag::Carry));
    EXPECT_FALSE(m.cpu.registers().flag(Flag::IrqDisable));
}

// ===========================================================================
// Subroutines
// ===========================================================================

TEST(InstructionSet, JsrAndRtsReturnToTheRightPlace)
{
    Machine m;
    m.load({
        0x20, 0x06, 0x80,   // $8000: JSR $8006
        0xA9, 0x01,         // $8003: LDA #$01   <- must resume here
        0xEA,               // $8005: NOP        <- never runs
        0xA9, 0x42,         // $8006: LDA #$42
        0x60,               // $8008: RTS
    });

    m.cpu.step();   // JSR
    EXPECT_EQ(m.cpu.registers().pc, 0x8006);
    EXPECT_EQ(m.cpu.registers().sp, 0xFB) << "two bytes pushed";

    m.cpu.step();   // LDA #$42
    EXPECT_EQ(m.cpu.registers().a, 0x42);

    m.cpu.step();   // RTS
    EXPECT_EQ(m.cpu.registers().pc, 0x8003) << "back to the instruction after JSR";
    EXPECT_EQ(m.cpu.registers().sp, 0xFD);

    m.cpu.step();   // LDA #$01
    EXPECT_EQ(m.cpu.registers().a, 0x01);
}

TEST(InstructionSet, JsrPushesTheLastByteOfItselfNotTheNextInstruction)
{
    // This is the asymmetry that makes RTS add 1. If JSR pushed $8003, RTS
    // would have to not add anything - and it does add, so JSR must push $8002.
    Machine m;
    m.load({ 0x20, 0x06, 0x80 });

    m.cpu.step();

    // Stack, newest first: low byte at $01FC, high byte at $01FD.
    EXPECT_EQ(m.bus.read(0x01FC), 0x02) << "low byte of $8002, not $8003";
    EXPECT_EQ(m.bus.read(0x01FD), 0x80);
}

TEST(InstructionSet, NestedSubroutineCalls)
{
    Machine m;
    m.load({
        0x20, 0x08, 0x80,   // $8000: JSR $8008
        0xA9, 0x01,         // $8003: LDA #$01   <- the outer return lands here
        0xEA, 0xEA,         // $8005, $8006
        0x60,               // $8007: unused
        0xA9, 0x02,         // $8008: LDA #$02
        0x20, 0x0F, 0x80,   // $800A: JSR $800F
        0x60,               // $800D: RTS
        0xEA,               // $800E
        0xA9, 0x03,         // $800F: LDA #$03
        0x60,               // $8011: RTS
    });

    m.cpu.run(6);

    EXPECT_EQ(m.cpu.registers().a, 0x03) << "the innermost write wins";
    EXPECT_EQ(m.cpu.registers().sp, 0xFD) << "two calls pushed four bytes, two returns pulled four";
    EXPECT_FALSE(m.cpu.is_halted());
}

// ===========================================================================
// BRK / RTI
// ===========================================================================

TEST(InstructionSet, BrkJumpsThroughTheIrqVector)
{
    Machine m;
    m.load({
        0xA9, 0x01,   // $8000: LDA #$01
        0x00,         // $8002: BRK
        0xEA,         // $8003: the byte BRK skips
    });
    m.set_vector(0xFFFE, 0x9000);

    m.cpu.step();   // LDA
    m.cpu.step();   // BRK

    EXPECT_EQ(m.cpu.registers().pc, 0x9000);
    EXPECT_TRUE(m.cpu.registers().flag(Flag::IrqDisable)) << "I is set";
    EXPECT_EQ(m.cpu.registers().sp, 0xFA) << "three bytes pushed";
}

TEST(InstructionSet, BrkPushesReturnAddressAndSetsBreak)
{
    Machine m;
    m.load({ 0x00, 0xEA });   // BRK at $8000
    m.set_vector(0xFFFE, 0x9000);

    m.cpu.step();

    // BRK pushes the address AFTER the byte it skips: $8002.
    EXPECT_EQ(m.bus.read(0x01FD), 0x80) << "high byte";
    EXPECT_EQ(m.bus.read(0x01FC), 0x02) << "low byte of $8002";

    const u8 pushed_p = m.bus.read(0x01FB);
    EXPECT_TRUE((pushed_p & 0x10) != 0) << "B tells software this was a BRK";
    EXPECT_TRUE((pushed_p & 0x20) != 0) << "unused bit";
}

TEST(InstructionSet, RtiRestoresStatusAndReturns)
{
    Machine m;
    m.load({ 0x00, 0xEA });   // BRK at $8000
    m.set_vector(0xFFFE, 0x9000);

    m.cpu.step();   // BRK -> jumps to $9000

    // Put an RTI at $9000.
    m.bus.write(0x9000, 0x40);
    m.cpu.registers().set_flag(Flag::Carry, true);
    m.cpu.registers().set_flag(Flag::Zero, true);

    m.cpu.step();   // RTI

    EXPECT_EQ(m.cpu.registers().pc, 0x8002) << "no +1 for RTI, unlike RTS";
    EXPECT_EQ(m.cpu.registers().sp, 0xFD);
}

TEST(InstructionSet, RtiRestoresTheFlagsThatWerePushed)
{
    Machine m;
    m.load({ 0x00, 0xEA });
    m.set_vector(0xFFFE, 0x9000);

    m.cpu.registers().set_flag(Flag::Carry, true);
    m.cpu.step();   // BRK pushes P (with C set)

    // Change the flags, then RTI and check they come back.
    m.bus.write(0x9000, 0x40);
    m.cpu.registers().set_flag(Flag::Carry, false);

    m.cpu.step();   // RTI

    EXPECT_TRUE(m.cpu.registers().flag(Flag::Carry)) << "restored from the stack";
}

// ===========================================================================
// Interrupts
// ===========================================================================

TEST(InstructionSet, NmiIsNotMaskable)
{
    Machine m;
    m.load({ 0xEA, 0xEA });
    m.set_vector(0xFFFA, 0x9000);

    m.cpu.registers().set_flag(Flag::IrqDisable, true);   // I is set
    m.cpu.request_nmi();

    const int cycles = m.cpu.step();

    EXPECT_EQ(m.cpu.registers().pc, 0x9000) << "NMI ignores I";
    EXPECT_EQ(cycles, 7);
    EXPECT_FALSE(m.cpu.nmi_pending()) << "the latch is cleared once serviced";
}

TEST(InstructionSet, NmiPushesBreakClear)
{
    Machine m;
    m.load({ 0xEA });
    m.set_vector(0xFFFA, 0x9000);

    m.cpu.request_nmi();
    m.cpu.step();

    const u8 pushed_p = m.bus.read(0x01FB);
    EXPECT_TRUE((pushed_p & 0x10) == 0) << "B is clear for a hardware interrupt";
    EXPECT_TRUE((pushed_p & 0x20) != 0) << "unused bit still 1";
}

TEST(InstructionSet, IrqIsMaskedByTheIFlag)
{
    Machine m;
    m.load({ 0xEA, 0xEA, 0xEA });
    m.set_vector(0xFFFE, 0x9000);

    m.cpu.registers().set_flag(Flag::IrqDisable, true);
    m.cpu.set_irq_line(true);

    m.cpu.step();
    EXPECT_EQ(m.cpu.registers().pc, 0x8001) << "masked, so the NOP ran";
    EXPECT_EQ(m.cpu.last_opcode(), 0xEA);

    m.cpu.registers().set_flag(Flag::IrqDisable, false);
    m.cpu.step();
    EXPECT_EQ(m.cpu.registers().pc, 0x9000) << "unmasked now";
}

TEST(InstructionSet, InterruptIsTakenBetweenInstructionsNotInsideOne)
{
    // An IRQ arriving right before a read-modify-write must not split it.
    // The NES relies on this: games use INC on hardware registers atomically.
    Machine m;
    m.load({ 0xEE, 0x00, 0x02 });   // INC $0200
    m.set_vector(0xFFFE, 0x9000);

    m.bus.write(0x0200, 0x10);
    m.cpu.registers().set_flag(Flag::IrqDisable, false);   // unmask
    m.cpu.set_irq_line(true);

    m.cpu.step();   // the interrupt wins, because it is checked first

    EXPECT_EQ(m.cpu.registers().pc, 0x9000);
    EXPECT_EQ(m.cpu.registers().sp, 0xFA);

    // The INC has not run yet.
    m.bus.write(0x9000, 0x40);   // RTI
    m.cpu.step();
    EXPECT_EQ(m.cpu.registers().pc, 0x8000) << "resumes at the INC";
    EXPECT_EQ(m.bus.read(0x0200), 0x10) << "still unmodified";

    m.cpu.set_irq_line(false);
    m.cpu.step();
    EXPECT_EQ(m.bus.read(0x0200), 0x11) << "now it runs, atomically";
}

TEST(InstructionSet, ServicingAnInterruptDisablesFurtherIrqs)
{
    Machine m;
    m.load({ 0xEA, 0xEA, 0xEA });
    m.set_vector(0xFFFE, 0x9000);

    m.cpu.registers().set_flag(Flag::IrqDisable, false);   // unmask
    m.cpu.set_irq_line(true);
    m.cpu.step();

    EXPECT_TRUE(m.cpu.registers().flag(Flag::IrqDisable));
    EXPECT_EQ(m.cpu.registers().pc, 0x9000);

    // The line is still high, but I now blocks it, so the handler runs.
    m.bus.write(0x9000, 0xEA);   // NOP
    m.cpu.step();
    EXPECT_EQ(m.cpu.last_opcode(), 0xEA) << "the NOP at $9000 ran";
}
