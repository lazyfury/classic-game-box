// ---------------------------------------------------------------------------
// demo_instructions - the complete 6502, running a real algorithm
//
// Build:  cmake --build build
// Run:    ./build/demo_instructions
//
// Read together with docs/computer-science/instruction-set.md
// and docs/computer-science/timing.md
// ---------------------------------------------------------------------------

#include "core/bit.hpp"
#include "core/cpu/cpu.hpp"
#include "core/cpu/disassembler.hpp"
#include "core/cpu/opcode.hpp"
#include "core/flat_bus.hpp"
#include "core/types.hpp"

#include <iomanip>
#include <iostream>
#include <span>
#include <string>
#include <vector>

using namespace fc;

namespace {

void title(const std::string& text)
{
    std::cout << "\n=== " << text << " ===\n";
}

std::string raw_bytes(FlatBus& bus, u16 address, int length)
{
    std::string out;
    for (int i = 0; i < length; ++i) {
        if (i != 0) out.push_back(' ');
        out += hex8(bus.read(static_cast<u16>(address + i))).substr(1);
    }
    return out;
}

struct Machine {
    FlatBus bus;
    Cpu cpu{ bus };

    void load(std::span<const u8> program, u16 origin = 0x8000)
    {
        bus.load(program, origin);
        bus.write(0xFFFC, bit::lo_byte(origin));
        bus.write(0xFFFD, bit::hi_byte(origin));
        cpu.reset();
    }
};

// --- 1 ---------------------------------------------------------------------
void showCompleteness()
{
    title("1. The instruction set is complete");

    int legal = 0;
    int illegal = 0;
    for (int i = 0; i < 256; ++i) {
        if (Cpu::implements(static_cast<u8>(i))) ++legal;
        else ++illegal;
    }

    std::cout << "  Defined opcodes   : " << legal << "\n";
    std::cout << "  Undefined opcodes : " << illegal << "\n";
    std::cout << "  Mnemonics         : " << kOperationCount << "\n\n";

    // Prove it: execute every one of the 151 defined opcodes and check that
    // the CPU does NOT halt. A halt means the operation was not implemented.
    int executed = 0;
    int halted = 0;
    for (int i = 0; i < 256; ++i) {
        const u8 opcode = static_cast<u8>(i);
        if (!Cpu::implements(opcode)) continue;

        Machine m;
        const std::vector<u8> program = { opcode, 0x00, 0x00, 0x00, 0xEA };
        m.load(program);
        m.cpu.step();
        ++executed;
        if (m.cpu.is_halted()) ++halted;
    }

    std::cout << "  Executed every defined opcode: " << executed << "\n";
    std::cout << "  Of those, halted as unimplemented: " << halted << "\n\n";
    std::cout << "  " << (halted == 0 ? "Complete." : "INCOMPLETE!") << "\n";
}

// --- 2 ---------------------------------------------------------------------
void showAdcFlags()
{
    title("2. ADC: one adder, two overflows");

    std::cout << "  ADC computes A + M + C. It sets C for unsigned overflow and V\n";
    std::cout << "  for signed overflow, and those are different questions.\n\n";

    struct Case { u8 a; u8 b; bool carry_in; const char* note; };
    const Case cases[] = {
        { 0x02, 0x02, false, "2 + 2 = 4, nothing special" },
        { 0x7F, 0x01, false, "+127 + 1 overflows signed, but not unsigned" },
        { 0xFF, 0x01, false, "-1 + 1 wraps unsigned, but is correct signed" },
        { 0x80, 0x80, false, "-128 + -128 overflows both ways" },
        { 0x50, 0x50, false, "80 + 80 = 160 overflows signed only" },
        { 0x02, 0x02, true,  "2 + 2 + carry = 5" },
    };

    std::cout << "  A     M     C    result  C  V  N  Z   meaning\n";
    std::cout << "  ----------------------------------------------------------------\n";

    for (const auto& c : cases) {
        Machine m;
        const std::vector<u8> program = { 0x69, c.b };   // ADC #imm
        m.load(program);
        m.cpu.registers().a = c.a;
        m.cpu.registers().set_flag(Flag::Carry, c.carry_in);
        m.cpu.step();

        const auto& r = m.cpu.registers();
        std::cout << "  " << std::left
                  << std::setw(6) << hex8(c.a)
                  << std::setw(6) << hex8(c.b)
                  << std::setw(5) << (c.carry_in ? "1" : "0")
                  << std::setw(8) << hex8(r.a)
                  << (r.flag(Flag::Carry) ? "1" : "0") << "  "
                  << (r.flag(Flag::Overflow) ? "1" : "0") << "  "
                  << (r.flag(Flag::Negative) ? "1" : "0") << "  "
                  << (r.flag(Flag::Zero) ? "1" : "0") << "   "
                  << c.note << "\n";
    }
    std::cout << std::right;

    std::cout << "\n  See docs/computer-science/overflow-flag.md for the derivation.\n";
}

// --- 3 ---------------------------------------------------------------------
void showSbcIdiom()
{
    title("3. SBC: why SEC comes first");

    std::cout << "  SBC computes A - M - (1 - C). The instruction is defined so that\n";
    std::cout << "  C means \"no borrow\":\n\n";
    std::cout << "      SEC        ; C = 1, meaning \"no borrow\"\n";
    std::cout << "      SBC #$03   ; A = A - 3\n\n";

    struct Case { u8 a; u8 m; bool carry; };
    const Case cases[] = {
        { 0x05, 0x03, true },    // 5 - 3
        { 0x03, 0x05, true },    // 3 - 5 borrows
        { 0x05, 0x03, false },   // 5 - 3 - 1
    };

    std::cout << "  A     M     C     result  C  V    meaning\n";
    std::cout << "  --------------------------------------------------\n";

    for (const auto& c : cases) {
        Machine m;
        const std::vector<u8> program = { 0xE9, c.m };
        m.load(program);
        m.cpu.registers().a = c.a;
        m.cpu.registers().set_flag(Flag::Carry, c.carry);
        m.cpu.step();

        const auto& r = m.cpu.registers();
        std::cout << "  " << std::left
                  << std::setw(6) << hex8(c.a)
                  << std::setw(6) << hex8(c.m)
                  << std::setw(6) << (c.carry ? "1" : "0")
                  << std::setw(8) << hex8(r.a)
                  << (r.flag(Flag::Carry) ? "1" : "0") << "  "
                  << (r.flag(Flag::Overflow) ? "1" : "0") << "    "
                  << static_cast<int>(bit::as_signed(c.a)) << " - "
                  << static_cast<int>(bit::as_signed(c.m))
                  << (c.carry ? "" : " - 1") << "\n";
    }
    std::cout << std::right;

    std::cout << "\n  Note the result column is the same bits either way: the carry\n";
    std::cout << "  flag is one wire, read as \"carry\" for ADC and as \"no borrow\"\n";
    std::cout << "  for SBC. Two's complement makes one adder do both jobs.\n";
}

// --- 4 ---------------------------------------------------------------------
std::vector<u8> buildSumProgram()
{
    // Sum the ten bytes at `data`, then store the total with a subroutine.
    //
    //      $8000  LDX #$00
    //      $8002  LDA #$00
    // loop $8004  CLC
    //      $8005  ADC $8017,X     (data)
    //      $8008  INX
    //      $8009  CPX #$0A
    //      $800B  BNE loop
    //      $800D  JSR $8013       (store)
    // done $8010  JMP $8010        <- JMP to itself: a deliberate stop
    //      $8013  STA $8021       (result)  <- store
    //      $8016  RTS
    // data $8017  1,2,3,4,5,6,7,8,9,10
    //      $8021  result
    return {
        0xA2, 0x00,        // $8000  LDX #$00
        0xA9, 0x00,        // $8002  LDA #$00
        0x18,              // $8004  CLC          <- loop
        0x7D, 0x17, 0x80,  // $8005  ADC $8017,X
        0xE8,              // $8008  INX
        0xE0, 0x0A,        // $8009  CPX #$0A
        0xD0, 0xF7,        // $800B  BNE $8004
        0x20, 0x13, 0x80,  // $800D  JSR $8013
        0x4C, 0x10, 0x80,  // $8010  JMP $8010    <- done
        0x8D, 0x21, 0x80,  // $8013  STA $8021    <- store
        0x60,              // $8016  RTS
        0x01, 0x02, 0x03, 0x04, 0x05,   // $8017  data
        0x06, 0x07, 0x08, 0x09, 0x0A,   // $801C
        0x00,              // $8021  result
    };
}

void showProgramListing(FlatBus& bus, std::span<const u8> program)
{
    title("4. A real program: sum an array");

    std::cout << "  Assembled listing:\n\n";
    std::cout << "    address  bytes         instruction      mode\n";
    std::cout << "    -------  ------------  ---------------  -------------\n";

    std::size_t offset = 0;
    while (offset < program.size() && offset < 0x17) {
        const u16 address = static_cast<u16>(0x8000 + offset);
        const auto insn = disassemble(
            std::span<const u8>(program.data() + offset, program.size() - offset), address);

        std::cout << "    " << std::left << std::setw(9) << hex16(address)
                  << std::setw(14) << raw_bytes(bus, address, insn.length)
                  << std::setw(17) << insn.text
                  << mode_name(insn.info.mode) << "\n";

        offset += static_cast<std::size_t>(insn.length);
    }
    std::cout << std::right;

    std::cout << "\n  Data at $8017: ";
    for (u16 i = 0; i < 10; ++i) {
        std::cout << static_cast<int>(bus.read(static_cast<u16>(0x8017 + i)));
        if (i != 9) std::cout << ",";
    }
    std::cout << "\n";
}

void runSumProgram()
{
    const std::vector<u8> program = buildSumProgram();

    FlatBus bus;
    Cpu cpu{ bus };
    bus.load(program, 0x8000);
    bus.write(0xFFFC, 0x00);
    bus.write(0xFFFD, 0x80);

    showProgramListing(bus, program);

    cpu.reset();

    // 2 setup + 10 loop iterations x 5 instructions + JSR/STA/RTS = 55.
    // The 56th would be the JMP that stops the machine.
    const int executed = cpu.run(55);

    const u8 total = bus.read(0x8021);

    std::cout << "\n  Result at $8021: " << hex8(total)
              << "  (" << static_cast<int>(total) << ")\n";
    std::cout << "  1+2+3+4+5+6+7+8+9+10 = 55 = " << hex8(0x37) << "\n\n";
    std::cout << "  Final state after " << executed << " instructions (PC is at $8010,\n";
    std::cout << "  the JMP that spins forever and stops the machine):\n";
    std::cout << "    A  = " << hex8(cpu.registers().a) << "\n";
    std::cout << "    X  = " << hex8(cpu.registers().x) << "\n";
    std::cout << "    P  = " << cpu.registers().status_string()
              << "   (I is set by reset; C stays set: the last ADC had no carry)\n";
    std::cout << "    SP = " << hex8(cpu.registers().sp)
              << "   (back to its reset value: JSR and RTS balanced)\n";
    std::cout << "    cycles = " << cpu.total_cycles() << "\n";
    std::cout << "    PC = " << hex16(cpu.registers().pc) << "\n";

    std::cout << "\n  What this program used:\n";
    std::cout << "    ADC        arithmetic with carry, and CLC to clear the carry in\n";
    std::cout << "    indexed    ADC $8017,X reads data[X]\n";
    std::cout << "    CPX + BNE  the loop condition and the backwards branch\n";
    std::cout << "    JSR/RTS    a subroutine call with a proper return\n";
    std::cout << "    absolute   STA $8021 writes the answer\n";
    std::cout << "    JMP self   a deliberate stop\n";
}

// --- 5 ---------------------------------------------------------------------
void showSubroutineStack()
{
    title("5. What JSR actually pushes");

    const std::vector<u8> program = {
        0x20, 0x05, 0x80,   // $8000  JSR $8005
        0xEA, 0xEA,         // $8003  the return point and padding
        0x60,               // $8005  RTS
    };

    FlatBus bus;
    Cpu cpu{ bus };
    bus.load(program, 0x8000);
    bus.write(0xFFFC, 0x00);
    bus.write(0xFFFD, 0x80);
    cpu.reset();

    cpu.step();   // JSR $8005

    std::cout << "  JSR $8005 at $8000\n\n";
    std::cout << "  PC after fetching the instruction : " << hex16(0x8003) << "\n";
    std::cout << "  But the pushed value is           : "
              << hex16(bit::make_u16(bus.read(0x01FC), bus.read(0x01FD))) << "\n\n";
    std::cout << "  JSR pushes PC - 1, the address of its own last byte.\n";
    std::cout << "  RTS therefore pulls and adds 1, landing on $8003.\n";
    std::cout << "  The asymmetry is real: it is how the chip is built.\n\n";
    std::cout << "  Stack bytes: [$01FD]=" << hex8(bus.read(0x01FD))
              << " [$01FC]=" << hex8(bus.read(0x01FC))
              << "   (high byte first, so the low byte comes back first)\n";

    std::cout << "\n  After JSR, PC = " << hex16(cpu.registers().pc)
              << " and SP = " << hex8(cpu.registers().sp) << "\n";

    cpu.step();   // RTS at $8005
    std::cout << "\n  After RTS, PC = " << hex16(cpu.registers().pc)
              << "   (not $8002, and not $8005)\n";
    std::cout << "  SP = " << hex8(cpu.registers().sp) << "  (the two bytes are off the stack again)\n";
}

// --- 6 ---------------------------------------------------------------------
void showInterrupts()
{
    title("6. Interrupts: three vectors, two maskable states");

    std::cout << "    $FFFA  NMI    cannot be masked, the PPU raises it every frame\n";
    std::cout << "    $FFFC  RESET  power on\n";
    std::cout << "    $FFFE  IRQ    masked by the I flag; BRK shares this vector\n\n";

    // NMI, with I set.
    {
        FlatBus bus;
        Cpu cpu{ bus };
        const std::vector<u8> program = { 0xEA, 0xEA };
        bus.load(program, 0x8000);
        bus.write(0xFFFC, 0x00);
        bus.write(0xFFFD, 0x80);
        bus.write(0xFFFA, 0x00);
        bus.write(0xFFFB, 0x90);
        cpu.reset();

        cpu.registers().set_flag(Flag::IrqDisable, true);
        cpu.request_nmi();
        const int cycles = cpu.step();

        std::cout << "  NMI with I=1:\n";
        std::cout << "    PC -> " << hex16(cpu.registers().pc)
                  << "   (NMI ignores I)\n";
        std::cout << "    cycles = " << cycles << "\n";
        std::cout << "    pushed status bit 4 (B) = "
                  << (bit::test(bus.read(0x01FB), 4) ? 1 : 0)
                  << "   (clear: hardware interrupt)\n";
    }

    // IRQ masked, then unmasked.
    {
        FlatBus bus;
        Cpu cpu{ bus };
        const std::vector<u8> program = { 0xEA, 0xEA, 0xEA };
        bus.load(program, 0x8000);
        bus.write(0xFFFC, 0x00);
        bus.write(0xFFFD, 0x80);
        bus.write(0xFFFE, 0x00);
        bus.write(0xFFFF, 0x90);
        cpu.reset();

        cpu.registers().set_flag(Flag::IrqDisable, true);
        cpu.set_irq_line(true);
        cpu.step();
        std::cout << "\n  IRQ with I=1:\n";
        std::cout << "    PC -> " << hex16(cpu.registers().pc)
                  << "   (masked, so the NOP at $8000 ran)\n";

        cpu.registers().set_flag(Flag::IrqDisable, false);
        cpu.step();
        std::cout << "  IRQ with I=0:\n";
        std::cout << "    PC -> " << hex16(cpu.registers().pc)
                  << "   (taken)\n";
        std::cout << "    I is now set again, so the still-high line cannot\n";
        std::cout << "    re-enter immediately.\n";
    }

    // BRK.
    {
        FlatBus bus;
        Cpu cpu{ bus };
        const std::vector<u8> program = { 0x00, 0xEA };
        bus.load(program, 0x8000);
        bus.write(0xFFFC, 0x00);
        bus.write(0xFFFD, 0x80);
        bus.write(0xFFFE, 0x00);
        bus.write(0xFFFF, 0x90);
        cpu.reset();

        cpu.step();

        std::cout << "\n  BRK at $8000:\n";
        std::cout << "    PC -> " << hex16(cpu.registers().pc) << "   (same vector as IRQ)\n";
        std::cout << "    pushed return address = "
                  << hex16(bit::make_u16(bus.read(0x01FC), bus.read(0x01FD)))
                  << "   (PC+2: BRK skips one byte)\n";
        std::cout << "    pushed status bit 4 (B) = "
                  << (bit::test(bus.read(0x01FB), 4) ? 1 : 0)
                  << "   (set: software asked for this)\n";
        std::cout << "\n  That B bit is the only way the handler can tell a BRK\n";
        std::cout << "  from a real hardware interrupt.\n";
    }
}

// --- 7 ---------------------------------------------------------------------
void showTiming()
{
    title("7. Timing is data, not arithmetic");

    std::cout << "  Every opcode has a fixed datasheet cost. Only two things vary:\n";
    std::cout << "    * an indexed read that crosses a page               +1\n";
    std::cout << "    * a branch that is taken                            +1\n";
    std::cout << "      ...and one more if it jumps across a page         +1\n\n";

    struct Case { const char* label; std::vector<u8> program; u8 x; u8 y; u16 origin; };
    const Case cases[] = {
        { "LDA #$42     immediate",        { 0xA9, 0x42 },             0, 0, 0x8000 },
        { "LDA $42      zero page",        { 0xA5, 0x42 },             0, 0, 0x8000 },
        { "LDA $02FF,X  crossing",         { 0xBD, 0xFF, 0x02 },       1, 0, 0x8000 },
        { "LDA $0200,X  not crossing",     { 0xBD, 0x00, 0x02 },       1, 0, 0x8000 },
        { "LDA ($42),Y  crossing",         { 0xB1, 0x42 },             0, 1, 0x8000 },
        { "STA $0200,X  write",            { 0x9D, 0x00, 0x02 },       1, 0, 0x8000 },
        { "ASL $0200    read-modify-write",{ 0x0E, 0x00, 0x02 },       0, 0, 0x8000 },
        { "BNE taken    same page",        { 0xD0, 0x02 },             0, 0, 0x8000 },
        { "BNE taken    across page",      { 0xD0, 0x10 },             0, 0, 0x80F0 },
        { "JSR $8004",                     { 0x20, 0x04, 0x80 },       0, 0, 0x8000 },
        { "BRK",                           { 0x00 },                   0, 0, 0x8000 },
    };

    std::cout << "  instruction                    cycles\n";
    std::cout << "  -----------------------------  ------\n";

    for (const auto& c : cases) {
        FlatBus bus;
        Cpu cpu{ bus };
        bus.load(c.program, c.origin);
        bus.write(0xFFFC, bit::lo_byte(c.origin));
        bus.write(0xFFFD, bit::hi_byte(c.origin));
        bus.write(0x0042, 0xFF);   // pointer for ($42),Y
        bus.write(0x0043, 0x02);   // -> base $02FF
        cpu.reset();

        cpu.registers().x = c.x;
        cpu.registers().y = c.y;
        if (std::string(c.label).rfind("BNE", 0) == 0) {
            cpu.registers().set_flag(Flag::Zero, false);
        }

        const int cycles = cpu.step();
        std::cout << "  " << std::left << std::setw(31) << c.label
                  << cycles << "\n";
    }
    std::cout << std::right;

    std::cout << "\n  Why it matters: the PPU runs at exactly 3x the CPU clock, and\n";
    std::cout << "  games count cycles to hit a particular scanline. Wrong timing is\n";
    std::cout << "  not a performance problem, it is a correctness problem.\n";

    // The table itself.
    bool seen[9] = {};
    for (int i = 0; i < 256; ++i) {
        const u8 c = opcode_cycles(static_cast<u8>(i));
        if (c < 9) { seen[c] = true; }
    }
    std::cout << "\n  Distinct base cycle counts in the table: ";
    for (int i = 0; i < 9; ++i) {
        if (seen[i]) std::cout << i << " ";
    }
    std::cout << "\n";
}

} // namespace

int main()
{
    std::cout << "Classic Game Box - Phase 1: the complete 6502\n";

    showCompleteness();
    showAdcFlags();
    showSbcIdiom();
    runSumProgram();
    showSubroutineStack();
    showInterrupts();
    showTiming();

    title("Summary");
    std::cout << "151 opcodes, 56 mnemonics, all implemented.\n";
    std::cout << "ADC adds with carry in; C is unsigned overflow, V is signed.\n";
    std::cout << "SBC subtracts with borrow; SEC must precede it.\n";
    std::cout << "JSR pushes PC-1 so RTS can add 1.\n";
    std::cout << "NMI cannot be masked; IRQ and BRK share the $FFFE vector.\n";
    std::cout << "Timing is a 256 entry table plus two data dependent penalties.\n";

    return 0;
}
