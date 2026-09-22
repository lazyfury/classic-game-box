// ---------------------------------------------------------------------------
// demo_addressing - from an addressing mode to an effective address
//
// Build:  cmake --build build
// Run:    ./build/demo_addressing
//
// Read together with docs/computer-science/addressing-modes.md
// ---------------------------------------------------------------------------

#include "core/bit.hpp"
#include "core/cpu/addressing.hpp"
#include "core/cpu/cpu.hpp"
#include "core/cpu/disassembler.hpp"
#include "core/cpu/opcode.hpp"
#include "core/flat_bus.hpp"
#include "core/types.hpp"

#include <iomanip>
#include <iostream>
#include <string>
#include <vector>

using namespace fc;

namespace {

void title(const std::string& text)
{
    std::cout << "\n=== " << text << " ===\n";
}

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

std::string describe(const Operand& op)
{
    switch (op.kind) {
    case OperandKind::None:
        return "(no operand)";
    case OperandKind::Value:
        return "value = " + hex8(op.value);
    case OperandKind::Address:
        return "address = " + hex16(op.address);
    case OperandKind::Target:
        return "target = " + hex16(op.address);
    }
    return "?";
}

// --- 1 ---------------------------------------------------------------------
void showTheIdea()
{
    title("1. A mode is a recipe, not a location");

    std::cout << "  LDA $0200,X       with X = 5\n\n";
    std::cout << "    mode      = absolute,X      <- the recipe\n";
    std::cout << "    operand   = $0200           <- the base, from the byte stream\n";
    std::cout << "    index     = 5               <- from register X\n";
    std::cout << "    --------------------------------\n";
    std::cout << "    effective = $0200 + 5 = $0205\n\n";
    std::cout << "  Only now does the CPU actually read [$0205].\n\n";
    std::cout << "  That computed number is called the EFFECTIVE ADDRESS.\n";
    std::cout << "  Every one of the 13 modes is a different recipe for producing it.\n";
}

// --- 2 ---------------------------------------------------------------------
void showAllModes(FlatBus& bus)
{
    title("2. Every mode, resolved");

    struct Case {
        const char* syntax;
        AddressingMode mode;
        u8 lo, hi, x, y;
        const char* note;
    };

    const Case cases[] = {
        { "TAX",         AddressingMode::Implied,     0x00, 0x00, 0x00, 0x00, "no operand" },
        { "ASL A",       AddressingMode::Accumulator, 0x00, 0x00, 0x00, 0x00, "operand is A" },
        { "LDA #$42",    AddressingMode::Immediate,   0x42, 0x00, 0x00, 0x00, "the byte itself" },
        { "LDA $42",     AddressingMode::ZeroPage,    0x42, 0x00, 0x00, 0x00, "high byte is 0" },
        { "LDA $F0,X",   AddressingMode::ZeroPageX,   0xF0, 0x00, 0x20, 0x00, "wraps in page 0" },
        { "LDX $10,Y",   AddressingMode::ZeroPageY,   0x10, 0x00, 0x00, 0x05, "wraps in page 0" },
        { "LDA $1234",   AddressingMode::Absolute,    0x34, 0x12, 0x00, 0x00, "full 16 bits" },
        { "LDA $0200,X", AddressingMode::AbsoluteX,   0x00, 0x02, 0x05, 0x00, "base + X" },
        { "LDA $0200,Y", AddressingMode::AbsoluteY,   0x00, 0x02, 0x00, 0x07, "base + Y" },
        { "JMP ($3000)", AddressingMode::Indirect,    0x00, 0x30, 0x00, 0x00, "pointer at $3000" },
        { "LDA ($40,X)", AddressingMode::IndirectX,   0x40, 0x00, 0x02, 0x00, "index pointer" },
        { "LDA ($40),Y", AddressingMode::IndirectY,   0x40, 0x00, 0x00, 0x05, "index result" },
        { "BNE $8007",   AddressingMode::Relative,    0x05, 0x00, 0x00, 0x00, "signed offset" },
    };

    // Pointers used by the indirect modes.
    bus.write(0x3000, 0x00);
    bus.write(0x3001, 0x90);   // -> $9000

    bus.write(0x0040, 0x00);   // pointer slot for (zp),Y with operand $40
    bus.write(0x0041, 0x02);   // -> base $0200
    bus.write(0x0042, 0x00);   // pointer slot for (zp,X) when X = 2
    bus.write(0x0043, 0x04);   // -> $0400

    std::cout << "  " << std::left << std::setw(13) << "syntax"
              << std::setw(14) << "mode"
              << std::setw(29) << "result"
              << "note" << "\n";
    std::cout << "  " << std::string(13, '-') << std::string(14, '-')
              << std::string(29, '-') << "----" << "\n";

    for (const auto& c : cases) {
        const auto op = resolve(request(c.mode, c.lo, c.hi, c.x, c.y), bus);

        std::cout << "  " << std::left << std::setw(13) << c.syntax
                  << std::setw(14) << mode_name(c.mode)
                  << std::setw(29) << describe(op)
                  << c.note << "\n";
    }
    std::cout << std::right;
}

// --- 3 ---------------------------------------------------------------------
void showZeroPageWrap(FlatBus& bus)
{
    title("3. Zero page wraps inside page 0");

    std::cout << "  The high byte of a zero page address is hard wired to $00.\n";
    std::cout << "  So the addition cannot carry out of page 0:\n\n";

    std::cout << "  " << std::left << std::setw(7) << "base"
              << std::setw(8) << "index"
              << std::setw(11) << "naive sum"
              << std::setw(11) << "actual" << "note\n";
    std::cout << "  " << std::string(7, '-') << std::string(8, '-')
              << std::string(11, '-') << std::string(11, '-') << "----\n";

    struct Case { u8 base; u8 index; };
    const Case cases[] = {
        { 0x42, 0x01 },
        { 0x42, 0xC0 },
        { 0xF0, 0x20 },
        { 0xFF, 0x01 },
        { 0xFF, 0xFF },
    };

    for (const auto& c : cases) {
        const u16 sums = static_cast<u16>(c.base) + c.index;
        const auto op = resolve(request(AddressingMode::ZeroPageX, c.base, 0, c.index), bus);

        std::cout << "  " << std::left
                  << std::setw(7)  << hex8(c.base)
                  << std::setw(8)  << hex8(c.index)
                  << std::setw(11) << hex16(sums)
                  << std::setw(11) << hex16(op.address);

        if (sums != op.address) {
            std::cout << "carry thrown away";
        } else {
            std::cout << "no wrap";
        }
        std::cout << "\n";
    }
    std::cout << std::right;

    std::cout << "\n  $42 + $C0 = $0102, but the CPU reads $0002.\n";
    std::cout << "  This is not a bug in the emulator - it is how the chip is wired.\n";
    std::cout << "  Games rely on it, so we must reproduce it exactly.\n";
}

// --- 4 ---------------------------------------------------------------------
void showIndirectPageBug(FlatBus& bus)
{
    title("4. JMP (indirect) reads its high byte from the wrong place");

    bus.write(0x10FF, 0x34);   // low byte
    bus.write(0x1100, 0x99);   // what you would expect
    bus.write(0x1000, 0x12);   // what the chip actually uses

    const u16 target = read_pointer_indirect(bus, 0x10FF);

    std::cout << "  JMP ($10FF)\n\n";
    std::cout << "    [$10FF] = " << hex8(bus.read(0x10FF)) << "   low byte\n";
    std::cout << "    [$1100] = " << hex8(bus.read(0x1100)) << "   the 'obvious' high byte\n";
    std::cout << "    [$1000] = " << hex8(bus.read(0x1000)) << "   the byte the 6502 really reads\n\n";
    std::cout << "    target = " << hex16(target) << "\n\n";
    std::cout << "  The pointer's high byte is fetched from the SAME 256 byte page\n";
    std::cout << "  as its low byte. A pointer ending in $FF therefore wraps.\n\n";
    std::cout << "  This is a genuine bug in the 1975 silicon. It was fixed in the\n";
    std::cout << "  65C02, but NES games were written against the buggy NMOS part,\n";
    std::cout << "  so the emulator has to reproduce it.\n";
}

// --- 5 ---------------------------------------------------------------------
void showIndirectZeroPageDifference(FlatBus& bus)
{
    title("5. (zp,X) versus (zp),Y - the order of operations");

    bus.clear();
    // One pointer, used by both forms, so the difference is purely the order
    // of operations. [$0044] = $00, [$0045] = $02  ->  points at $0200.
    bus.write(0x0044, 0x00);
    bus.write(0x0045, 0x02);

    const u8 slot = static_cast<u8>(0x42 + 0x02);   // $44
    const u16 pointer = read_pointer_zero_page(bus, slot);

    const auto indirect_x = resolve(request(AddressingMode::IndirectX, 0x42, 0, 0x02), bus);
    const auto indirect_y = resolve(request(AddressingMode::IndirectY, 0x44, 0, 0, 0x02), bus);

    std::cout << "  Pointer set up so that [$0044] = $00 and [$0045] = $02,\n";
    std::cout << "  which is a 16 bit pointer to " << hex16(pointer) << ".\n\n";

    std::cout << "  LDA ($42,X)   with X = 2\n";
    std::cout << "    1. index the POINTER first:  $42 + X = " << hex16(slot) << "\n";
    std::cout << "    2. read the pointer there:   " << hex16(pointer) << "\n";
    std::cout << "    3. effective address       = " << hex16(indirect_x.address) << "\n\n";

    std::cout << "  LDA ($44),Y   with Y = 2\n";
    std::cout << "    1. read the pointer at $0044 = " << hex16(pointer) << "\n";
    std::cout << "    2. index the RESULT:  " << hex16(pointer) << " + Y = "
              << hex16(indirect_y.address) << "\n\n";

    std::cout << "  Comma inside the parentheses  -> index the pointer.\n";
    std::cout << "  Comma outside the parentheses -> index the result.\n";
    std::cout << "  Note that X only exists in the first form and Y only in the\n";
    std::cout << "  second: the 6502 has no ($42),X or ($42,Y) instruction.\n";
}

// --- 6 ---------------------------------------------------------------------
void showPageCrossing(FlatBus& bus)
{
    title("6. Page crossing: when the index carries into the high byte");

    struct Case { u8 lo, hi, index; };
    const Case cases[] = {
        { 0x00, 0x02, 0x05 },   // $0200 + 5  = $0205, no cross
        { 0xFF, 0x02, 0x01 },   // $02FF + 1  = $0300, crosses
        { 0x00, 0x02, 0xFF },   // $0200 + FF = $02FF, still page 2
        { 0xFF, 0xFF, 0x02 },   // $FFFF + 2  = $0001, wraps the whole address space
    };

    std::cout << "  base     index   effective   page crossed\n";
    std::cout << "  -------  ------  ----------  ------------\n";

    for (const auto& c : cases) {
        const auto op = resolve(request(AddressingMode::AbsoluteX, c.lo, c.hi, c.index), bus);

        std::cout << "  " << std::left << std::setw(9)
                  << hex16(bit::make_u16(c.lo, c.hi))
                  << std::setw(8) << hex8(c.index)
                  << std::setw(12) << hex16(op.address)
                  << (op.page_crossed ? "yes" : "no") << "\n";
    }
    std::cout << std::right;

    std::cout << "\n  A read that crosses a page costs one extra cycle, because the\n";
    std::cout << "  CPU has to fix up the high byte of the address. Writes do not\n";
    std::cout << "  pay this, which is why STA $0200,X is always one cycle slower\n";
    std::cout << "  than LDA $0200,X but never varies.\n";
}

// --- 7 ---------------------------------------------------------------------
void runARealProgram()
{
    title("7. A real loop, using indexed addressing");

    const std::vector<u8> program = {
        0xA2, 0x00,        // LDX #$00
        0xBD, 0x00, 0x03,  // LDA $0300,X      <- read from the source array
        0x9D, 0x00, 0x02,  // STA $0200,X      <- write to the destination
        0xE8,              // INX
        0xE0, 0x04,        // CPX #$04
        0xD0, 0xF5,        // BNE -11          <- loop back to LDA
    };

    FlatBus bus;
    Cpu cpu{ bus };
    bus.load(program, 0x8000);
    bus.write(0xFFFC, 0x00);
    bus.write(0xFFFD, 0x80);

    bus.write(0x0300, 0x11);
    bus.write(0x0301, 0x22);
    bus.write(0x0302, 0x33);
    bus.write(0x0303, 0x44);

    std::cout << "  A disassembly listing of the loop:\n\n";
    std::size_t offset = 0;
    while (offset < program.size()) {
        const u16 address = static_cast<u16>(0x8000 + offset);
        const auto insn = disassemble(
            std::span<const u8>(program.data() + offset, program.size() - offset), address);
        std::cout << "    " << hex16(address) << "   " << std::left << std::setw(16)
                  << insn.text << "  ; " << mode_name(insn.info.mode) << "\n";
        offset += static_cast<std::size_t>(insn.length);
    }
    std::cout << std::right;

    std::cout << "\n  Source array $0300: ";
    for (u16 i = 0; i < 4; ++i) {
        std::cout << hex8(bus.read(static_cast<u16>(0x0300 + i))) << " ";
    }

    std::cout << "\n  Destination $0200: ";
    for (u16 i = 0; i < 4; ++i) {
        std::cout << hex8(bus.read(static_cast<u16>(0x0200 + i))) << " ";
    }
    std::cout << "   <- all zero\n";

    cpu.reset();
    const int executed = cpu.run(200);

    std::cout << "\n  Ran " << executed << " instructions";
    if (cpu.is_halted()) {
        std::cout << " then stopped at " << hex8(cpu.unimplemented_opcode())
                  << " (BRK): the loop finished and PC walked off the end\n";
        std::cout << "  of the program into zeroed memory. Phase 0.4 does not\n";
        std::cout << "  implement BRK yet, so the CPU halts loudly.\n\n";
    } else {
        std::cout << ".\n\n";
    }

    std::cout << "  Destination $0200: ";
    for (u16 i = 0; i < 4; ++i) {
        std::cout << hex8(bus.read(static_cast<u16>(0x0200 + i))) << " ";
    }
    std::cout << "  <- copied\n";
    std::cout << "  Final X = " << hex8(cpu.registers().x)
              << ",  P = " << cpu.registers().status_string()
              << "  (Z set because CPX #$04 made them equal)\n";
}

} // namespace

int main()
{
    std::cout << "Classic Game Box - Phase 0.4: effective addresses\n";

    FlatBus bus;

    showTheIdea();
    showAllModes(bus);
    showZeroPageWrap(bus);
    showIndirectPageBug(bus);
    showIndirectZeroPageDifference(bus);
    showPageCrossing(bus);
    runARealProgram();

    title("Summary");
    std::cout << "An addressing mode is a recipe for computing the effective address.\n";
    std::cout << "Zero page wraps inside page 0: $42,X with X=$C0 reads $0002.\n";
    std::cout << "JMP ($xxFF) reads its high byte from $xx00 - a real silicon bug.\n";
    std::cout << "($42,X) indexes the pointer; ($42),Y indexes the result.\n";
    std::cout << "An indexed read that crosses a page costs one extra cycle.\n";

    return 0;
}
