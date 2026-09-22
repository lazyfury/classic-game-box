// ---------------------------------------------------------------------------
// demo_disasm - bytes <-> assembly
//
// Build:  cmake --build build
// Run:    ./build/demo_disasm
//
// Read together with docs/computer-science/assembly.md
// ---------------------------------------------------------------------------

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

DecodedInstruction decode_bytes(std::initializer_list<u8> bytes, u16 address = 0x8000)
{
    const std::vector<u8> v(bytes);
    return disassemble(std::span<const u8>(v.data(), v.size()), address);
}

// --- 1 ---------------------------------------------------------------------
void showSymbols()
{
    title("1. The four symbols in 6502 assembly");

    std::cout << "  LDA #$42\n";
    std::cout << "  ^   ^^\n";
    std::cout << "  |   |\n";
    std::cout << "  |   +-- $  hexadecimal prefix.  $42 == 0x42 == 66\n";
    std::cout << "  +------ #  immediate. The operand IS the data, it is not an\n";
    std::cout << "             address to load from.\n\n";

    std::cout << "  LDA $42          without #, $42 is an ADDRESS: read from $0042\n";
    std::cout << "  LDA $42,X        ,  adds index register X to the address\n";
    std::cout << "  JMP ($8000)      ( ) indirect: read a pointer, then jump there\n\n";

    std::cout << "  Summary of the vocabulary:\n";
    std::cout << "    #      the operand is a literal value\n";
    std::cout << "    $      what follows is hexadecimal\n";
    std::cout << "    ,X ,Y  add an index register to the address\n";
    std::cout << "    ( )    the operand is a pointer; dereference it\n\n";

    std::cout << "  Note the two hex prefixes you have now seen:\n";
    std::cout << "    0x42   C++ source code   (this project's own code)\n";
    std::cout << "    $42    6502 assembly     (what the ROM contains)\n";
    std::cout << "  Same number, two languages, two conventions.\n";
}

// --- 2 ---------------------------------------------------------------------
void showBothDirections()
{
    title("2. Assembler and disassembler are inverses");

    struct Case { std::initializer_list<u8> bytes; };

    std::cout << "  assembler:      text  -->  bytes\n";
    std::cout << "  disassembler:   bytes -->  text\n\n";

    std::cout << "  bytes          disassembly\n";
    std::cout << "  -------------  --------------------\n";

    const std::vector<std::vector<u8>> programs = {
        { 0xA9, 0x42 },
        { 0x4C, 0x00, 0x80 },
        { 0x6C, 0x00, 0x80 },
        { 0xBD, 0x00, 0x02 },
        { 0x96, 0x42 },
        { 0x0A },
        { 0xF0, 0x05 },
        { 0x00 },
    };

    for (const auto& p : programs) {
        const auto insn = disassemble(std::span<const u8>(p.data(), p.size()), 0x8000);
        std::cout << "  " << std::left << std::setw(13)
                  << bytes_to_string(std::span<const u8>(p.data(), p.size()), insn.length)
                  << "  " << std::setw(20) << insn.text
                  << "  (" << mode_name(insn.info.mode) << ")\n";
    }
    std::cout << std::right;
}

// --- 3 ---------------------------------------------------------------------
void showAddressingModes()
{
    title("3. The 13 addressing modes");

    struct Case { const char* syntax; u8 opcode; u8 lo; u8 hi; const char* note; };

    const Case cases[] = {
        { "TAX",       0xAA, 0x00, 0x00, "implied - no operand at all" },
        { "ASL A",     0x0A, 0x00, 0x00, "accumulator - operand is A" },
        { "LDA #$42",  0xA9, 0x42, 0x00, "the literal byte 0x42" },
        { "LDA $42",   0xA5, 0x42, 0x00, "address $0042 (high byte is always 0)" },
        { "LDA $42,X", 0xB5, 0x42, 0x00, "($42 + X) & 0xFF - wraps inside page 0" },
        { "LDX $42,Y", 0xB6, 0x42, 0x00, "same, with Y" },
        { "LDA $1234", 0xAD, 0x34, 0x12, "full 16 bit address" },
        { "LDA $1234,X", 0xBD, 0x34, 0x12, "16 bit address + X" },
        { "LDA $1234,Y", 0xB9, 0x34, 0x12, "16 bit address + Y" },
        { "JMP ($1234)", 0x6C, 0x34, 0x12, "read a 16 bit pointer from $1234" },
        { "LDA ($42,X)", 0xA1, 0x42, 0x00, "zp pointer + X, THEN dereference" },
        { "LDA ($42),Y", 0xB1, 0x42, 0x00, "dereference zp pointer, THEN + Y" },
        { "BNE $8008", 0xD0, 0x06, 0x00, "signed offset from the next instruction" },
    };

    std::cout << "  syntax          opcode  len  mode\n";
    std::cout << "  --------------  ------  ---  ------------------------------------\n";

    for (const auto& c : cases) {
        const auto insn = decode_bytes({ c.opcode, c.lo, c.hi });

        std::cout << "  " << std::left << std::setw(14) << insn.text
                  << "  " << hex8(c.opcode)
                  << "    " << insn.length << "   "
                  << mode_name(insn.info.mode) << "\n";

        if (insn.text != c.syntax) {
            std::cout << "        !! expected " << c.syntax << "\n";
        }
        std::cout << "        " << c.note << "\n";
    }
    std::cout << std::right;

    std::cout << "\n  The (zp,X) vs (zp),Y difference trips everyone up:\n";
    std::cout << "    ($42,X) : index the POINTER, then dereference\n";
    std::cout << "    ($42),Y : dereference first, then index the RESULT\n";
}

// --- 4 ---------------------------------------------------------------------
void showLengthRule()
{
    title("4. Instruction length is decided by the addressing mode");

    struct Row { const char* mode; int operand_bytes; };
    const Row rows[] = {
        { "implied",       0 },
        { "accumulator",   0 },
        { "immediate",     1 },
        { "zero page",     1 },
        { "zero page,X",   1 },
        { "zero page,Y",   1 },
        { "indirect,X",    1 },
        { "indirect,Y",    1 },
        { "relative",      1 },
        { "absolute",      2 },
        { "absolute,X",    2 },
        { "absolute,Y",    2 },
        { "indirect",      2 },
    };

    std::cout << "  mode           operand bytes   total length\n";
    std::cout << "  -------------  -------------   ------------\n";
    for (const auto& r : rows) {
        std::cout << "  " << std::left << std::setw(15) << r.mode
                  << std::setw(16) << r.operand_bytes
                  << (1 + r.operand_bytes) << "\n";
    }
    std::cout << std::right;

    std::cout << "\n  A CPU can therefore fetch an instruction without understanding\n";
    std::cout << "  it: look up the opcode, learn the length, advance PC by that\n";
    std::cout << "  much. That is what makes the fetch loop possible at all.\n";

    // Verify with a real count over the whole table.
    int one = 0, two = 0, three = 0;
    for (int i = 0; i < 256; ++i) {
        const auto& info = opcode_info(static_cast<u8>(i));
        if (!info.is_legal()) {
            continue;
        }
        if (info.length() == 1) ++one;
        else if (info.length() == 2) ++two;
        else if (info.length() == 3) ++three;
    }
    std::cout << "\n  Actual counts among the 151 legal opcodes:\n";
    std::cout << "    1 byte: " << one << "   2 bytes: " << two << "   3 bytes: " << three << "\n";
}

// --- 5 ---------------------------------------------------------------------
void showBranchArithmetic()
{
    title("5. Branch targets need two's complement");

    std::cout << "  A branch operand is a SIGNED 8 bit offset from the address of\n";
    std::cout << "  the NEXT instruction (the branch itself is 2 bytes long).\n\n";
    std::cout << "      target = address_of_branch + 2 + (signed)offset\n\n";

    struct Case { u16 address; u8 operand; const char* note; };
    const Case cases[] = {
        { 0x8000, 0x05, "0x05 = +5    forward" },
        { 0x8000, 0xFB, "0xFB = -5    backward" },
        { 0x8000, 0x7F, "0x7F = +127  furthest forward" },
        { 0x8000, 0x80, "0x80 = -128  furthest backward" },
        { 0x8000, 0xFE, "0xFE = -2    a tight two byte loop" },
        { 0xFFFE, 0x00, "next instruction would be $10000 -> $0000" },
        { 0xFFFE, 0xFE, "jumps to itself" },
    };

    std::cout << "  branch    operand   signed   target    note\n";
    std::cout << "  --------  --------  -------  --------  ----\n";

    for (const auto& c : cases) {
        const auto insn = decode_bytes({ 0xD0, c.operand }, c.address);
        const int signed_offset = static_cast<int>(static_cast<std::int8_t>(c.operand));

        std::cout << "  " << std::left
                  << std::setw(10) << hex16(c.address)
                  << std::setw(10) << hex8(c.operand)
                  << std::setw(9)  << signed_offset
                  << std::setw(10) << hex16(insn.target)
                  << c.note << "\n";
    }
    std::cout << std::right;

    std::cout << "\n  If you treat 0xFB as unsigned you get +251 and the emulator jumps\n";
    std::cout << "  to the wrong place. This is the single most common early bug.\n";
    std::cout << "  See docs/computer-science/twos-complement.md section 8.2.\n";
}

// --- 6 ---------------------------------------------------------------------
void showIllegalOpcodes()
{
    title("6. Illegal opcodes are reported, not guessed");

    int legal = 0;
    for (int i = 0; i < 256; ++i) {
        if (opcode_info(static_cast<u8>(i)).is_legal()) {
            ++legal;
        }
    }

    std::cout << "  The official 6502 defines " << legal << " of 256 opcodes.\n";
    std::cout << "  The remaining " << (256 - legal) << " are undefined.\n\n";

    const auto known = decode_bytes({ 0xA9, 0x42 });
    const auto unknown = decode_bytes({ 0x02, 0xFF });

    std::cout << "  " << hex8(known.opcode) << " -> \"" << known.text
              << "\"   legal, length " << known.length << "\n";
    std::cout << "  " << hex8(unknown.opcode) << " -> \"" << unknown.text
              << "\"        illegal, length assumed 1\n\n";

    std::cout << "  Some undefined codes do things on real hardware (the so-called\n";
    std::cout << "  illegal opcodes). Nothing here relies on that behaviour, and\n";
    std::cout << "  pretending they are NOPs would produce silently wrong games.\n";
}

// --- 7 ---------------------------------------------------------------------
void showListing(FlatBus& bus, const std::vector<u8>& program)
{
    title("7. A debugger-style listing");

    std::size_t offset = 0;
    while (offset < program.size()) {
        const u16 address = static_cast<u16>(0x8000 + offset);
        const auto insn = disassemble(
            std::span<const u8>(program.data() + offset, program.size() - offset),
            address);

        std::cout << "  " << hex16(address) << "   "
                  << std::left << std::setw(11)
                  << bytes_to_string(
                         std::span<const u8>(program.data() + offset, program.size() - offset),
                         insn.length)
                  << "  " << std::setw(16) << insn.text
                  << "  ; " << mode_name(insn.info.mode) << "\n";

        offset += static_cast<std::size_t>(insn.length);
    }
    std::cout << std::right;

    (void)bus;
}

// --- 8 ---------------------------------------------------------------------
void showKnowledgeGap(const std::vector<u8>& program)
{
    title("8. What the disassembler knows vs what the CPU can do");

    std::cout << "  The disassembler knows the whole official instruction set:\n";
    std::cout << "    151 opcodes, 56 mnemonics, 13 addressing modes.\n\n";
    std::cout << "  The CPU currently executes a small subset:\n\n";
    std::cout << "    address    instruction        disassembler  CPU\n";
    std::cout << "    ---------  -----------------  ------------  ---\n";

    std::size_t offset = 0;
    while (offset < program.size()) {
        const u16 address = static_cast<u16>(0x8000 + offset);
        const auto insn = disassemble(
            std::span<const u8>(program.data() + offset, program.size() - offset),
            address);

        const bool known_by_disasm = insn.is_legal();
        const bool known_by_cpu = Cpu::implements(insn.opcode);

        std::cout << "    " << std::left
                  << std::setw(11) << hex16(address)
                  << std::setw(19) << insn.text
                  << std::setw(14) << (known_by_disasm ? "yes" : "no")
                  << (known_by_cpu ? "yes" : "no") << "\n";

        offset += static_cast<std::size_t>(insn.length);
    }
    std::cout << std::right;

    std::cout << "\n  Two different kinds of ignorance:\n";
    std::cout << "    * the disassembler does not know a byte's MEANING\n";
    std::cout << "    * the CPU does not know how to EXECUTE it (yet)\n";
    std::cout << "  Phase 0.4 adds the addressing modes, Phase 1 adds the rest\n";
    std::cout << "  of the instruction set.\n";
}

} // namespace

int main()
{
    std::cout << "Classic Game Box - Phase 0.3: 6502 assembly and disassembly\n";

    const std::vector<u8> program = {
        0xA9, 0x42,        // LDA #$42
        0xAA,              // TAX
        0x8D, 0x00, 0x02,  // STA $0200
        0xBD, 0x00, 0x02,  // LDA $0200,X
        0x4C, 0x00, 0x80,  // JMP $8000
    };

    FlatBus bus;
    bus.load(program, 0x8000);

    showSymbols();
    showBothDirections();
    showAddressingModes();
    showLengthRule();
    showBranchArithmetic();
    showIllegalOpcodes();
    showListing(bus, program);
    showKnowledgeGap(program);

    title("Summary");
    std::cout << "An opcode encodes BOTH an operation and an addressing mode.\n";
    std::cout << "The addressing mode determines the instruction's length.\n";
    std::cout << "# immediate   $ hex   ,X index   ( ) pointer\n";
    std::cout << "Branch offsets are signed: target = pc + 2 + (s8)offset\n";
    std::cout << "The disassembler knows all 151 opcodes; the CPU implements 12.\n";

    return 0;
}
