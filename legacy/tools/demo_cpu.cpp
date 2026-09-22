// ---------------------------------------------------------------------------
// demo_cpu - watch the fetch / decode / execute loop one instruction at a time
//
// Build:  cmake --build build
// Run:    ./build/demo_cpu
//
// The listing and the register trace are produced by the real disassembler
// from packages/fc-core/src/core/cpu/disassembler.cpp - no hand written mnemonic table here.
// ---------------------------------------------------------------------------

#include "core/bit.hpp"
#include "core/cpu/cpu.hpp"
#include "core/cpu/disassembler.hpp"
#include "core/flat_bus.hpp"
#include "core/types.hpp"

#include <array>
#include <iomanip>
#include <iostream>
#include <span>
#include <string>
#include <vector>

using namespace fc;

namespace {

constexpr u16 kOrigin = 0x8000;

void title(const std::string& text)
{
    std::cout << "\n=== " << text << " ===\n";
}

/// Grab the raw bytes of one instruction straight from the bus.
std::string raw_from_bus(FlatBus& bus, u16 address, int length)
{
    std::array<u8, 3> bytes{};
    for (int i = 0; i < length && i < 3; ++i) {
        bytes[static_cast<std::size_t>(i)] = bus.read(static_cast<u16>(address + i));
    }
    return bytes_to_string(std::span<const u8>(bytes.data(), bytes.size()), length);
}

// ---------------------------------------------------------------------------
// Table rendering. Every column uses the same std::setw so the header and the
// rows line up.
// ---------------------------------------------------------------------------

constexpr int kStepWidth   = 4;
constexpr int kPcWidth     = 6;
constexpr int kInstrWidth  = 11;
constexpr int kRegWidth    = 4;
constexpr int kStatusWidth = 8;

void print_header()
{
    std::cout << "  " << std::setw(kStepWidth) << "step"
              << "  " << std::left << std::setw(kPcWidth) << "PC"
              << "  " << std::setw(kInstrWidth) << "instruction"
              << "  " << std::setw(kRegWidth) << "A"
              << "  " << std::setw(kRegWidth) << "X"
              << "  " << std::setw(kRegWidth) << "Y"
              << "  " << std::setw(kRegWidth) << "SP"
              << "  " << std::setw(kStatusWidth) << "P"
              << "  " << std::setw(6) << "cycles" << std::right << "\n";

    std::cout << "  " << std::string(4, '-')
              << "  " << std::string(6, '-')
              << "  " << std::string(11, '-')
              << "  " << std::string(4, '-')
              << "  " << std::string(4, '-')
              << "  " << std::string(4, '-')
              << "  " << std::string(4, '-')
              << "  " << std::string(8, '-')
              << "  " << std::string(6, '-') << "\n";
}

void print_row(int step, u16 pc, const std::string& instruction,
               const Registers& r, u64 cycles)
{
    std::cout << "  " << std::setw(kStepWidth) << step
              << "  " << std::left << std::setw(kPcWidth) << hex16(pc)
              << "  " << std::setw(kInstrWidth) << instruction
              << "  " << std::setw(kRegWidth) << hex8(r.a)
              << "  " << std::setw(kRegWidth) << hex8(r.x)
              << "  " << std::setw(kRegWidth) << hex8(r.y)
              << "  " << std::setw(kRegWidth) << hex8(r.sp)
              << "  " << std::setw(kStatusWidth) << r.status_string()
              << "  " << std::right << std::setw(6) << cycles << "\n";
}

// --- 1 ---------------------------------------------------------------------
void showMemoryLayout(FlatBus& bus, const std::vector<u8>& program)
{
    title("1. The program in memory");

    std::cout << "  Reset vector at $FFFC-$FFFD (little endian):\n";
    std::cout << "    [$FFFC] = " << hex8(bus.read(0xFFFC)) << "   <- low byte\n";
    std::cout << "    [$FFFD] = " << hex8(bus.read(0xFFFD)) << "   <- high byte\n";
    std::cout << "    entry   = " << hex16(kOrigin) << "\n\n";

    std::cout << "  Program at " << hex16(kOrigin) << ":\n\n";
    std::cout << "    address  bytes        disassembly\n";
    std::cout << "    -------  -----------  ------------------\n";

    std::size_t offset = 0;
    while (offset < program.size()) {
        const u16 address = static_cast<u16>(kOrigin + offset);
        const auto insn = disassemble(bus, address);

        std::cout << "    " << std::left << std::setw(7) << hex16(address) << "  "
                  << std::setw(11) << raw_from_bus(bus, address, insn.length) << "  "
                  << std::setw(18) << insn.text
                  << "  ; " << mode_name(insn.info.mode) << "\n";

        offset += static_cast<std::size_t>(insn.length);
    }
    std::cout << std::right;
}

// --- 2 ---------------------------------------------------------------------
void traceExecution(FlatBus& bus, Cpu& cpu)
{
    title("2. Fetch / decode / execute, one instruction at a time");

    print_header();

    print_row(0, cpu.registers().pc, "(reset)", cpu.registers(), cpu.total_cycles());

    int step = 0;
    while (!cpu.is_halted() && step < 12) {
        const u16 pc = cpu.registers().pc;
        const auto insn = disassemble(bus, pc);

        cpu.step();
        ++step;

        print_row(step, pc, insn.text, cpu.registers(), cpu.total_cycles());
    }

    if (cpu.is_halted()) {
        const auto insn = disassemble(bus, cpu.registers().pc);
        std::cout << "\n  CPU halted on " << hex8(cpu.unimplemented_opcode())
                  << " (" << insn.text << "). Phase 0.2 only implements 12 opcodes,\n";
        std::cout << "  and this one is not among them.\n";
        std::cout << "  The emulator stops loudly instead of silently doing nothing,\n";
        std::cout << "  so an unimplemented instruction cannot go unnoticed.\n";
    }
}

// --- 3 ---------------------------------------------------------------------
void explainOneInstruction(FlatBus& bus)
{
    title("3. Anatomy of one instruction: LDA #$42");

    const u16 pc = kOrigin;
    const u8 opcode = bus.read(pc);
    const u8 operand = bus.read(static_cast<u16>(pc + 1));

    std::cout << "  memory:\n";
    std::cout << "    " << hex16(pc) << " : "
              << bit::to_binary(opcode, 8, true) << " : " << hex8(opcode)
              << "   <- opcode  (selects the instruction)\n";
    std::cout << "    " << hex16(static_cast<u16>(pc + 1)) << " : "
              << bit::to_binary(operand, 8, true) << " : " << hex8(operand)
              << "   <- operand (the literal data)\n\n";

    std::cout << "  1. PC = " << hex16(pc) << "\n";
    std::cout << "  2. fetch byte at PC -> " << hex8(opcode)
              << ", PC becomes " << hex16(static_cast<u16>(pc + 1)) << "\n";
    std::cout << "  3. decode: " << hex8(opcode) << " means 'load the accumulator with\n";
    std::cout << "     the byte that follows' - immediate addressing, #$42\n";
    std::cout << "  4. fetch byte at PC -> " << hex8(operand)
              << ", PC becomes " << hex16(static_cast<u16>(pc + 2)) << "\n";
    std::cout << "  5. A = " << hex8(operand) << "\n";
    std::cout << "  6. update flags: N = bit 7 = 0, Z = (A == 0) = 0\n";
    std::cout << "  7. cycles += 2: one fetch for the opcode, one for the operand\n";
}

// --- 4 ---------------------------------------------------------------------
void showResetVectorMechanism()
{
    title("4. Why the CPU does not start at address 0");

    std::cout << "  A CPU has no idea what a 'program' is. Its entire reset\n";
    std::cout << "  contract is: 'read two bytes from $FFFC and jump there'.\n\n";
    std::cout << "  Consequences:\n";
    std::cout << "    * the cartridge decides where execution begins\n";
    std::cout << "    * the same CPU can boot a different system by wiring\n";
    std::cout << "      a different address decoder to $FFFC\n";
    std::cout << "    * a NES ROM cannot lie about its entry point - the bytes\n";
    std::cout << "      in PRG ROM at $FFFC are the truth\n";
}

} // namespace

int main()
{
    std::cout << "Classic Game Box - Phase 0.2: the fetch / decode / execute loop\n";

    const std::vector<u8> program = {
        0xA9, 0x42,   // LDA #$42    A = 0x42
        0xAA,         // TAX         X = A
        0xA8,         // TAY         Y = A
        0xE8,         // INX         X = X + 1
        0xC8,         // INY         Y = Y + 1
        0xA9, 0x00,   // LDA #$00    A = 0, Z = 1
        0xEA,         // NOP
    };

    FlatBus bus;
    Cpu cpu{ bus };

    bus.load(program, kOrigin);
    bus.write(0xFFFC, bit::lo_byte(kOrigin));
    bus.write(0xFFFD, bit::hi_byte(kOrigin));

    cpu.reset();

    showMemoryLayout(bus, program);
    traceExecution(bus, cpu);
    explainOneInstruction(bus);
    showResetVectorMechanism();

    title("Summary");
    std::cout << "CPU state = 6 bytes: A, X, Y, SP, P, PC\n";
    std::cout << "The loop = fetch(opcode) -> decode -> execute -> update state\n";
    std::cout << "PC only ever moves forward, one byte per fetch\n";
    std::cout << "The CPU never touches memory directly: everything goes via the Bus\n";
    std::cout << "Entry point comes from the reset vector at $FFFC-$FFFD\n";
    std::cout << "\nThe listing above came from the real disassembler, not from a\n";
    std::cout << "table written by hand for this demo.\n";

    return 0;
}
