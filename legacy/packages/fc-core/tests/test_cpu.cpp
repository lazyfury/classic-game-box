#include "core/bit.hpp"
#include "core/cpu/cpu.hpp"
#include "core/flat_bus.hpp"
#include "core/types.hpp"

#include <gtest/gtest.h>

#include <array>
#include <initializer_list>
#include <vector>

using namespace fc;

namespace {

/// A minimal machine: a flat 64KB bus plus a CPU wired to it.
/// This is the whole point of the Bus abstraction - no NES hardware needed.
struct Machine {
    FlatBus bus;
    Cpu cpu{ bus };

    /// Write a program at `origin` and point the reset vector at it.
    void load(std::initializer_list<u8> program, u16 origin = 0x8000)
    {
        const std::vector<u8> bytes(program);
        bus.clear();
        bus.load(bytes, origin);
        bus.write(0xFFFC, bit::lo_byte(origin));
        bus.write(0xFFFD, bit::hi_byte(origin));
        cpu.reset();
    }
};

} // namespace

// ===========================================================================
// Reset: the entry point comes from the reset vector
// ===========================================================================

TEST(Cpu, ResetReadsTheEntryPointFromTheVector)
{
    Machine m;
    m.bus.write(0xFFFC, 0x00);
    m.bus.write(0xFFFD, 0x80);
    m.cpu.reset();

    EXPECT_EQ(m.cpu.registers().pc, 0x8000);
}

TEST(Cpu, ResetVectorIsLittleEndian)
{
    Machine m;
    m.bus.write(0xFFFC, 0x34); // low byte first
    m.bus.write(0xFFFD, 0x12); // high byte second
    m.cpu.reset();

    EXPECT_EQ(m.cpu.registers().pc, 0x1234);
    EXPECT_NE(m.cpu.registers().pc, 0x3412) << "swapped endianness is a classic bug";
}

TEST(Cpu, ResetClearsCyclesAndHaltState)
{
    Machine m;
    m.load({ 0xEA });
    EXPECT_EQ(m.cpu.total_cycles(), 0u);
    EXPECT_FALSE(m.cpu.is_halted());
}

// ===========================================================================
// Fetch: PC advances, one byte per fetch
// ===========================================================================

TEST(Cpu, PcAdvancesPastTheFetchedBytes)
{
    Machine m;
    m.load({ 0xA9, 0x42 }); // LDA #$42 : 2 bytes

    ASSERT_EQ(m.cpu.registers().pc, 0x8000);
    m.cpu.step();
    EXPECT_EQ(m.cpu.registers().pc, 0x8002) << "opcode + operand consumed";
}

TEST(Cpu, CurrentInstructionPcMarksTheStart)
{
    Machine m;
    m.load({ 0xA9, 0x42, 0xEA });

    m.cpu.step();
    EXPECT_EQ(m.cpu.current_instruction_pc(), 0x8000);

    m.cpu.step();
    EXPECT_EQ(m.cpu.current_instruction_pc(), 0x8002);
}

// ===========================================================================
// Execute: the immediate loads
// ===========================================================================

TEST(Cpu, LdaImmediateLoadsTheAccumulator)
{
    Machine m;
    m.load({ 0xA9, 0x42 });

    EXPECT_EQ(m.cpu.last_opcode(), 0);
    const int cycles = m.cpu.step();

    const auto& r = m.cpu.registers();
    EXPECT_EQ(r.a, 0x42);
    EXPECT_EQ(m.cpu.last_opcode(), 0xA9);
    EXPECT_EQ(cycles, 2);
    EXPECT_EQ(m.cpu.total_cycles(), 2u);

    // 0x42 has bit 7 clear and is non-zero.
    EXPECT_FALSE(r.flag(Flag::Negative));
    EXPECT_FALSE(r.flag(Flag::Zero));
}

TEST(Cpu, LoadImmediateSetsTheZeroFlag)
{
    Machine m;
    m.load({ 0xA9, 0x00 });
    m.cpu.step();

    EXPECT_TRUE(m.cpu.registers().flag(Flag::Zero));
    EXPECT_FALSE(m.cpu.registers().flag(Flag::Negative));
}

TEST(Cpu, LoadImmediateSetsTheNegativeFlag)
{
    Machine m;
    m.load({ 0xA9, 0x80 });
    m.cpu.step();

    EXPECT_TRUE(m.cpu.registers().flag(Flag::Negative));
    EXPECT_FALSE(m.cpu.registers().flag(Flag::Zero));
    EXPECT_EQ(m.cpu.registers().a, 0x80);
}

TEST(Cpu, LdxAndLdyImmediate)
{
    Machine m;
    m.load({ 0xA2, 0x10,   // LDX #$10
             0xA0, 0xFF }); // LDY #$FF

    m.cpu.step();
    EXPECT_EQ(m.cpu.registers().x, 0x10);

    m.cpu.step();
    EXPECT_EQ(m.cpu.registers().y, 0xFF);
    EXPECT_TRUE(m.cpu.registers().flag(Flag::Negative));
}

TEST(Cpu, NopChangesNothingButPcAndCycles)
{
    Machine m;
    m.load({ 0xEA });

    const auto before = m.cpu.registers();
    const int cycles = m.cpu.step();
    const auto& after = m.cpu.registers();

    EXPECT_EQ(cycles, 2);
    EXPECT_EQ(after.a, before.a);
    EXPECT_EQ(after.x, before.x);
    EXPECT_EQ(after.y, before.y);
    EXPECT_EQ(after.p, before.p);
    EXPECT_EQ(after.pc, static_cast<u16>(before.pc + 1)) << "NOP is 1 byte";
}

// ===========================================================================
// Transfers between registers
// ===========================================================================

TEST(Cpu, TransferInstructionsCopyTheByte)
{
    Machine m;
    m.load({ 0xA2, 0x11,   // LDX #$11
             0xA0, 0x22,   // LDY #$22
             0x8A,         // TXA -> A = X = 0x11
             0x98 });      // TYA -> A = Y = 0x22

    m.cpu.run(2);
    EXPECT_EQ(m.cpu.registers().a, 0x00) << "nothing loaded A yet";
    EXPECT_EQ(m.cpu.registers().x, 0x11);
    EXPECT_EQ(m.cpu.registers().y, 0x22);

    m.cpu.step(); // TXA
    EXPECT_EQ(m.cpu.registers().a, 0x11);

    m.cpu.step(); // TYA
    EXPECT_EQ(m.cpu.registers().a, 0x22);

    EXPECT_EQ(m.cpu.registers().x, 0x11) << "transfers do not consume the source";
    EXPECT_EQ(m.cpu.registers().y, 0x22);
}

TEST(Cpu, TransferUpdatesFlags)
{
    Machine m;
    m.load({ 0xA9, 0x00,   // LDA #$00  -> Z = 1
             0xA2, 0x80,   // LDX #$80  -> N = 1
             0x8A });      // TXA       -> A = 0x80, N = 1, Z = 0

    m.cpu.run(3);
    const auto& r = m.cpu.registers();
    EXPECT_EQ(r.a, 0x80);
    EXPECT_TRUE(r.flag(Flag::Negative));
    EXPECT_FALSE(r.flag(Flag::Zero));
}

// ===========================================================================
// Increment / decrement wrap around
// ===========================================================================

TEST(Cpu, IncrementWrapsAround)
{
    Machine m;
    m.load({ 0xA2, 0xFF,   // LDX #$FF
             0xE8,         // INX -> 0x00, Z = 1
             0xC8 });      // INY -> 0x01

    m.cpu.step(); // LDX
    EXPECT_EQ(m.cpu.registers().x, 0xFF);

    m.cpu.step(); // INX
    EXPECT_EQ(m.cpu.registers().x, 0x00);
    EXPECT_TRUE(m.cpu.registers().flag(Flag::Zero));

    m.cpu.step(); // INY
    EXPECT_EQ(m.cpu.registers().y, 0x01);
    EXPECT_FALSE(m.cpu.registers().flag(Flag::Zero));
}

TEST(Cpu, DecrementWrapsAround)
{
    Machine m;
    m.load({ 0xA2, 0x00,   // LDX #$00
             0xCA,         // DEX -> 0xFF, N = 1
             0x88 });      // DEY -> 0xFF

    m.cpu.run(3);
    EXPECT_EQ(m.cpu.registers().x, 0xFF);
    EXPECT_EQ(m.cpu.registers().y, 0xFF);
    EXPECT_TRUE(m.cpu.registers().flag(Flag::Negative));
}

// ===========================================================================
// Whole programs
// ===========================================================================

TEST(Cpu, RunsAShortProgram)
{
    Machine m;
    m.load({ 0xA9, 0x42,   // LDA #$42
             0xAA,         // TAX
             0xA8,         // TAY
             0xEA,         // NOP
             0xE8,         // INX
             0xC8 });      // INY

    const int executed = m.cpu.run(6);
    EXPECT_EQ(executed, 6);

    const auto& r = m.cpu.registers();
    EXPECT_EQ(r.a, 0x42);
    EXPECT_EQ(r.x, 0x43);
    EXPECT_EQ(r.y, 0x43);
    EXPECT_EQ(r.pc, 0x8007);            // 2+1+1+1+1+1 bytes
    EXPECT_EQ(m.cpu.total_cycles(), 2u * 6);
}

TEST(Cpu, StopsAtAnIllegalOpcode)
{
    // 0x02 is undefined on the official 6502: not "unimplemented by us",
    // genuinely not an instruction. The CPU must stop rather than guess.
    Machine m;
    m.load({ 0xA9, 0x01,   // LDA #$01
             0x02,         // illegal
             0xA9, 0x02 }); // would never run

    m.cpu.step();
    EXPECT_FALSE(m.cpu.is_halted());

    m.cpu.step();
    EXPECT_TRUE(m.cpu.is_halted());
    EXPECT_EQ(m.cpu.unimplemented_opcode(), 0x02);

    // Running further does nothing: an emulator bug must not go unnoticed.
    const int extra = m.cpu.run(10);
    EXPECT_EQ(extra, 0);
    EXPECT_EQ(m.cpu.registers().a, 0x01) << "the second LDA must not have run";
}

TEST(Cpu, RunRespectsMaxSteps)
{
    Machine m;
    m.load({ 0xEA, 0xEA, 0xEA, 0xEA });

    const int executed = m.cpu.run(2);
    EXPECT_EQ(executed, 2);
    EXPECT_EQ(m.cpu.registers().pc, 0x8002);
}

// ===========================================================================
// implements() must never drift away from execute()
// ===========================================================================

TEST(Cpu, ImplementsAgreesWithExecution)
{
    // Cpu::implements() is a hand written list that duplicates the switch in
    // execute(). Duplication like that rots. This test walks all 256 opcodes
    // and checks the list against what actually happens.
    for (int i = 0; i < 256; ++i) {
        const u8 opcode = static_cast<u8>(i);

        Machine m;
        // Two dummy operand bytes, in case the instruction needs them.
        m.load({ opcode, 0x00, 0x00, 0xEA });
        m.cpu.step();

        EXPECT_EQ(Cpu::implements(opcode), !m.cpu.is_halted())
            << "mismatch for opcode " << bit::to_hex(opcode);
    }
}

TEST(Cpu, ImplementsEveryLegalOpcodeAfterPhaseOne)
{
    // Phase 1 completes the instruction set: all 151 defined opcodes execute,
    // and the only ones that halt are the 105 undefined ones.
    int legal = 0;
    int illegal = 0;
    for (int i = 0; i < 256; ++i) {
        if (Cpu::implements(static_cast<u8>(i))) {
            ++legal;
        } else {
            ++illegal;
        }
    }

    EXPECT_EQ(legal, 151);
    EXPECT_EQ(legal + illegal, 256);

    // The operations that were still missing in Phase 0.4.
    EXPECT_TRUE(Cpu::implements(0x69));  // ADC #imm
    EXPECT_TRUE(Cpu::implements(0xE9));  // SBC #imm
    EXPECT_TRUE(Cpu::implements(0x29));  // AND #imm
    EXPECT_TRUE(Cpu::implements(0x09));  // ORA #imm
    EXPECT_TRUE(Cpu::implements(0x49));  // EOR #imm
    EXPECT_TRUE(Cpu::implements(0x24));  // BIT zp
    EXPECT_TRUE(Cpu::implements(0x48));  // PHA
    EXPECT_TRUE(Cpu::implements(0x08));  // PHP
    EXPECT_TRUE(Cpu::implements(0x68));  // PLA
    EXPECT_TRUE(Cpu::implements(0x28));  // PLP
    EXPECT_TRUE(Cpu::implements(0x20));  // JSR
    EXPECT_TRUE(Cpu::implements(0x60));  // RTS
    EXPECT_TRUE(Cpu::implements(0x40));  // RTI
    EXPECT_TRUE(Cpu::implements(0x00));  // BRK

    // Genuinely undefined.
    EXPECT_FALSE(Cpu::implements(0x02));
    EXPECT_FALSE(Cpu::implements(0xFF));
}

// ===========================================================================
// The stack lives in page 1 and grows downward
// ===========================================================================

TEST(Cpu, PushWritesToPageOneAndMovesSpDown)
{
    Machine m;
    m.load({});

    ASSERT_EQ(m.cpu.registers().sp, 0xFD);
    m.cpu.push(0xAB);

    EXPECT_EQ(m.cpu.registers().sp, 0xFC);
    EXPECT_EQ(m.bus.read(0x01FD), 0xAB) << "stored at $0100 | SP (before decrement)";
}

TEST(Cpu, PushThenPopReturnsTheSameByte)
{
    Machine m;
    m.load({});

    m.cpu.push(0x11);
    m.cpu.push(0x22);
    m.cpu.push(0x33);

    EXPECT_EQ(m.cpu.registers().sp, 0xFA);

    EXPECT_EQ(m.cpu.pop(), 0x33) << "last in, first out";
    EXPECT_EQ(m.cpu.pop(), 0x22);
    EXPECT_EQ(m.cpu.pop(), 0x11);
    EXPECT_EQ(m.cpu.registers().sp, 0xFD);
}

TEST(Cpu, StackPointerWrapsWithinPageOne)
{
    Machine m;
    m.load({});

    m.cpu.registers().sp = 0x00;
    m.cpu.push(0x5A);

    EXPECT_EQ(m.cpu.registers().sp, 0xFF);
    EXPECT_EQ(m.bus.read(0x0100), 0x5A);
}

TEST(Cpu, StackPointerCyclesThroughEveryValueAndNeverLeavesPageOne)
{
    Machine m;
    m.load({});

    std::array<bool, 256> visited{};

    for (int i = 0; i < 256; ++i) {
        const u8 sp_before = m.cpu.registers().sp;
        m.cpu.push(static_cast<u8>(i));

        visited[sp_before] = true;

        // The write must land at $0100 | sp_before - never outside page 1.
        EXPECT_EQ(m.bus.read(static_cast<u16>(0x0100 | sp_before)),
                  static_cast<u8>(i));
    }

    for (int i = 0; i < 256; ++i) {
        EXPECT_TRUE(visited[i]) << "SP value " << i << " was never used";
    }

    // 256 pushes is a whole cycle: SP comes back to where it started.
    EXPECT_EQ(m.cpu.registers().sp, 0xFD);
}

// ===========================================================================
// Compare: the C flag is "no borrow"
// ===========================================================================

TEST(Cpu, CompareSetsCarryWhenRegisterIsGreaterOrEqual)
{
    // CMP  is  A - M  with the result thrown away.
    //   C = 1  when A >= M   (no borrow)
    //   Z = 1  when A == M
    // Programs then use BCS/BCC to branch on it.
    struct Case { u8 a; u8 m; bool expect_carry; bool expect_zero; };
    const Case cases[] = {
        { 0x05, 0x03, true,  false },   // 5 > 3
        { 0x05, 0x05, true,  true  },   // 5 == 5
        { 0x03, 0x05, false, false },   // 3 < 5  -> borrow
        { 0x00, 0x00, true,  true  },
        { 0xFF, 0x00, true,  false },
        { 0x00, 0xFF, false, false },   // 0 - 255 borrows
    };

    for (const auto& c : cases) {
        Machine m;
        m.load({ 0xC9, c.m });        // CMP #imm
        m.cpu.registers().a = c.a;
        m.cpu.step();

        const auto& r = m.cpu.registers();
        EXPECT_EQ(r.flag(Flag::Carry), c.expect_carry)
            << "A=" << static_cast<int>(c.a) << " M=" << static_cast<int>(c.m);
        EXPECT_EQ(r.flag(Flag::Zero), c.expect_zero)
            << "A=" << static_cast<int>(c.a) << " M=" << static_cast<int>(c.m);
    }
}
