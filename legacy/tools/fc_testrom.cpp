// ---------------------------------------------------------------------------
// fc_testrom - running the outside world's test ROMs.
//
// Every other test in this project compares the emulator against itself:
// native against wasm, Electron against native, a save state against a run
// without one. All of that proves the port is faithful. None of it proves the
// emulator is *right*, because a machine that is consistently wrong passes all
// of it.
//
// A test ROM is written by somebody else, checks the hardware, and says so.
// This tool is how they get run.
//
// Two kinds, and they could hardly be more different.
//
//   nestest   Does not say anything. In its automated mode it simply runs, and
//             the way you check it is to log every instruction and compare
//             against a known-good log. So the log this produces is the
//             point -- see tools/compare_nestest.py.
//
//   blargg    Talks. Writes text to $6004 and a status byte to $6001, which is
//             the closest thing the console has to stdout. The ROM decides
//             what to check and reports its own result, which is why these are
//             worth more than any test written here.
//
// Read together with docs/nes/ and with tools/fc_headless.cpp, which is the
// same idea without the harness.
// ---------------------------------------------------------------------------

#include "core/cpu/disassembler.hpp"
#include "core/cpu/opcode.hpp"
#include "core/nes/machine.hpp"
#include "core/types.hpp"

#include <algorithm>
#include <cstdio>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <optional>
#include <string>
#include <vector>

using namespace fc;

namespace {

// ---------------------------------------------------------------------------
// ROM reading
// ---------------------------------------------------------------------------

std::optional<std::vector<u8>> read_file(const std::string& path)
{
    std::ifstream file(path, std::ios::binary);
    if (!file) {
        return std::nullopt;
    }
    return std::vector<u8>(std::istreambuf_iterator<char>(file),
                           std::istreambuf_iterator<char>());
}

// ---------------------------------------------------------------------------
// nestest
//
// The log format is fixed, and it was fixed by whoever ran nestest on a real
// NES and wrote the output down. Reproducing it exactly is the whole test:
//
//   C000  4C F5 C5  JMP $C5F5                       A:00 X:00 Y:00 P:24 SP:FD PPU:  0, 21 CYC:7
//   |     |        |                               |
//   |     |        |                               register file
//   |     |        the disassembly, annotated with the value the operand
//   |     |        resolves to, read *before* the instruction runs
//   |     the raw bytes, padded to eight
//   the program counter
//
// Columns 0, 4, 6, 15 and 48. The fifteenth is the odd one: an undocumented
// opcode gets a `*` where a documented one gets a space, which is how the same
// width holds both.
// ---------------------------------------------------------------------------

/// `%04X  %-8s %-33s` -- one byte of hex, two spaces, eight of bytes, one
/// space, thirty-three of disassembly. 4 + 2 + 8 + 1 + 33 = 48.
constexpr const char* kLogPrefix = "%04X  %-8s %-33s";
constexpr const char* kLogSuffix = "A:%02X X:%02X Y:%02X P:%02X SP:%02X PPU:%3d,%3d CYC:%llu";

std::string hex(unsigned value, int digits)
{
    char buffer[8];
    std::snprintf(buffer, sizeof(buffer), "%0*X", digits, value);
    return buffer;
}

/// The operand as the log writes it, with the value it resolves to.
///
/// Every memory mode is annotated, because seeing what an instruction actually
/// touched is the reason the log is worth reading. The exceptions are the ones
/// that do not read or write anything: a branch, JSR and JMP say where they go
/// in the operand itself, and JMP (indirect) shows the pointer it followed.
///
/// The value is read *before* the instruction runs, which for a store means
/// the byte that was there, not the byte that is about to be.
std::string annotated_operand(nes::Machine& machine, const DecodedInstruction& instruction)
{
    auto& bus = machine.bus();
    auto& cpu = machine.cpu();
    const u8 x = cpu.registers().x;
    const u8 y = cpu.registers().y;

    // A pointer on the 6502 is not simply two consecutive bytes, and both of
    // the ways it is not matter here because the log's annotations come from
    // reading through them.
    //
    //   ($nn,X) and ($nn),Y   the pointer lives in page 0 and the high byte
    //                         wraps there: $FF/$00, never $FF/$100
    //   JMP ($nnnn)           the famous bug: the high byte comes from the
    //                         same page as the low one, so $02FF reads
    //                         $02FF and $0200
    //
    // Reproducing both is the point. The CPU does the same thing in
    // addressing.cpp; this is the harness agreeing with it.
    const auto read16 = [&bus](u16 address) {
        return static_cast<u16>(bus.read(address) | (static_cast<u16>(bus.read(address + 1)) << 8));
    };
    const auto read16_zero_page = [&bus](u8 address) {
        const u8 high = static_cast<u8>(address + 1);
        return static_cast<u16>(bus.read(address) | (static_cast<u16>(bus.read(high)) << 8));
    };
    const auto read16_indirect_bug = [&bus](u16 address) {
        const u16 high = static_cast<u16>((address & 0xFF00) | ((address + 1) & 0x00FF));
        return static_cast<u16>(bus.read(address) | (static_cast<u16>(bus.read(high)) << 8));
    };

    switch (instruction.info.mode) {
    case AddressingMode::Implied:
        return {};

    case AddressingMode::Accumulator:
        return "A";

    case AddressingMode::Immediate:
        return "#$" + hex(instruction.operand_lo, 2);

    case AddressingMode::Relative:
        return "$" + hex(instruction.target, 4);

    case AddressingMode::ZeroPage: {
        const u8 address = instruction.operand_lo;
        return "$" + hex(address, 2) + " = " + hex(bus.read(address), 2);
    }

    case AddressingMode::ZeroPageX: {
        const u8 effective = static_cast<u8>(instruction.operand_lo + x);
        return "$" + hex(instruction.operand_lo, 2) + ",X @ " + hex(effective, 2)
             + " = " + hex(bus.read(effective), 2);
    }

    case AddressingMode::ZeroPageY: {
        const u8 effective = static_cast<u8>(instruction.operand_lo + y);
        return "$" + hex(instruction.operand_lo, 2) + ",Y @ " + hex(effective, 2)
             + " = " + hex(bus.read(effective), 2);
    }

    case AddressingMode::Absolute: {
        const u16 address = static_cast<u16>(instruction.operand_lo
                                             | (static_cast<u16>(instruction.operand_hi) << 8));

        // A control transfer names its destination in the operand; there is
        // nothing to annotate.
        if (instruction.info.op == Operation::JSR || instruction.info.op == Operation::JMP) {
            return "$" + hex(address, 4);
        }
        return "$" + hex(address, 4) + " = " + hex(bus.read(address), 2);
    }

    case AddressingMode::AbsoluteX: {
        const u16 base = static_cast<u16>(instruction.operand_lo
                                          | (static_cast<u16>(instruction.operand_hi) << 8));
        const u16 effective = static_cast<u16>(base + x);
        return "$" + hex(base, 4) + ",X @ " + hex(effective, 4)
             + " = " + hex(bus.read(effective), 2);
    }

    case AddressingMode::AbsoluteY: {
        const u16 base = static_cast<u16>(instruction.operand_lo
                                          | (static_cast<u16>(instruction.operand_hi) << 8));
        const u16 effective = static_cast<u16>(base + y);
        return "$" + hex(base, 4) + ",Y @ " + hex(effective, 4)
             + " = " + hex(bus.read(effective), 2);
    }

    case AddressingMode::Indirect: {
        const u16 pointer = static_cast<u16>(instruction.operand_lo
                                             | (static_cast<u16>(instruction.operand_hi) << 8));
        return "($" + hex(pointer, 4) + ") = " + hex(read16_indirect_bug(pointer), 4);
    }

    case AddressingMode::IndirectX: {
        const u8 zero_page = static_cast<u8>(instruction.operand_lo + x);
        const u16 effective = read16_zero_page(zero_page);
        return "($" + hex(instruction.operand_lo, 2) + ",X) @ " + hex(zero_page, 2)
             + " = " + hex(effective, 4) + " = " + hex(bus.read(effective), 2);
    }

    case AddressingMode::IndirectY: {
        const u16 pointer = read16_zero_page(instruction.operand_lo);
        const u16 effective = static_cast<u16>(pointer + y);
        return "($" + hex(instruction.operand_lo, 2) + "),Y = " + hex(pointer, 4)
             + " @ " + hex(effective, 4) + " = " + hex(bus.read(effective), 2);
    }

    case AddressingMode::Unknown:
        break;
    }
    return "$" + hex(instruction.operand_lo, 2);
}

/// One line of the log, for the instruction about to run.
std::string log_line(nes::Machine& machine)
{
    const auto& registers = machine.cpu().registers();
    const u16 pc = registers.pc;

    // Decode once from the bus to find out how long it is, then decode again
    // from the bytes actually read, so the printed bytes and the annotation
    // are talking about the same instruction.
    const DecodedInstruction probe = disassemble(machine.bus(), pc);
    const int length = std::max(1, probe.length);

    std::vector<u8> bytes;
    bytes.reserve(static_cast<std::size_t>(length));
    for (int i = 0; i < length; ++i) {
        bytes.push_back(machine.bus().read(static_cast<u16>(pc + i)));
    }

    const DecodedInstruction instruction =
        disassemble(std::span<const u8>(bytes.data(), bytes.size()), pc);

    // "*" for an opcode the official 6502 does not define, a space for the
    // rest. That single character sits in the column that would otherwise be
    // the second space of the separator, which is why documented and
    // undocumented lines have the same width.
    std::string text = instruction.is_legal() ? " " : "*";
    text += operation_name(instruction.info.op);

    const std::string operand = annotated_operand(machine, instruction);
    if (!operand.empty()) {
        text += " ";
        text += operand;
    }

    char prefix[128];
    std::snprintf(prefix, sizeof(prefix), kLogPrefix, pc,
                  bytes_to_string(bytes, static_cast<int>(bytes.size())).c_str(),
                  text.c_str());

    char suffix[96];
    std::snprintf(suffix, sizeof(suffix), kLogSuffix,
                  registers.a, registers.x, registers.y, registers.p,
                  registers.sp, machine.ppu().scanline(), machine.ppu().dot(),
                  static_cast<unsigned long long>(machine.cpu().total_cycles()));

    return std::string(prefix) + suffix;
}

int run_nestest(nes::Machine& machine, int instruction_count)
{
    // Automated mode: the ROM's reset vector points at the menu, and starting
    // at $C000 instead skips it and runs the checks silently.
    machine.reset();
    machine.cpu().registers().pc = 0xC000;
    machine.cpu().registers().sp = 0xFD;

    for (int i = 0; i < instruction_count; ++i) {
        std::cout << log_line(machine) << "\n";

        if (!machine.run_instructions(1)) {
            std::cerr << "the CPU halted at instruction " << i + 1 << "\n";
            return 1;
        }
    }
    return 0;
}

// ---------------------------------------------------------------------------
// blargg
//
// The convention, invented by blargg and used by most of his tests:
//
//   $6001   bit 7 set   still running, and the low seven bits say what
//           bit 7 clear the result code. Zero is a pass.
//           $81         asking for a reset, once, before it starts
//   $6004   a character to print. Zero means nothing to print.
//
// Work RAM is where a cartridge puts its variables, and it is also the only
// place on the console a test can write to that a harness can read without
// pretending to be the game.
// ---------------------------------------------------------------------------

constexpr u16 kStatusAddress = 0x6001;
constexpr u16 kOutputAddress = 0x6004;

int run_console(nes::Machine& machine, int max_frames)
{
    auto& bus = machine.bus();

    // Wait for the signature before believing anything the ROM says.
    //
    // This is not politeness, it is the whole test. Work RAM is zero at power
    // on, and a zero in $6001 means "finished, passed". A harness that reads
    // the status before the ROM has written it reports that every test in the
    // world passed, instantly -- which is exactly what this one did until the
    // signature was checked. A test that cannot fail is not a test.
    //
    // The signature is three bytes, $DE $B0 $61, written to $6001 as soon as
    // the ROM starts. Until they are all there, $6001 means nothing.
    int frame = 0;
    for (; frame < max_frames; ++frame) {
        if (!machine.run_frame()) {
            std::cerr << "\nthe CPU halted at frame " << frame << "\n";
            return 1;
        }
        if (bus.read(0x6001) == 0xDE && bus.read(0x6002) == 0xB0
            && bus.read(0x6003) == 0x61) {
            break;
        }
    }

    if (frame >= max_frames) {
        std::cerr << "no $DE $B0 $61 signature at $6001 after " << max_frames
                  << " frames; this is not a harness-aware test ROM\n";
        return 2;
    }

    std::cerr << "(signature at frame " << frame << ")\n";

    // Whether the ROM has ever said "still running". See the note below about
    // what a zero in $6001 means before that.
    bool started = false;

    int resets = 0;
    for (; frame < max_frames; ++frame) {
        if (!machine.run_frame()) {
            std::cerr << "\nthe CPU halted at frame " << frame << "\n";
            return 1;
        }

        const u8 status = bus.read(kStatusAddress);

        if (frame % 2000 == 0) {
            std::cerr << "(frame " << frame << " $6000-$6007:";
            for (u16 a = 0x6000; a <= 0x6007; ++a) {
                std::fprintf(stderr, " %02X", bus.read(a));
            }
            std::cerr << ")\n";
        }

        // Still the signature: the ROM has not got as far as reporting
        // anything, so $6001 is not a status yet.
        if (status == 0xDE) {
            continue;
        }

        if (status == 0x81) {
            // The test wants a reset before it starts. Once.
            if (resets++ < 4) {
                machine.reset();
                continue;
            }
        }

        if ((status & 0x80) != 0) {
            started = true;
            continue;
        }

        // A result, but only if the ROM ever said it was running. Work RAM is
        // zero at power on and a zero here means "finished, passed" -- so a
        // ROM that has not started yet, or one that has stalled before its
        // first status write, looks exactly like a test that passed instantly.
        // That is not a small detail: it is the difference between a harness
        // and a harness that reports that everything in the world works.
        if (!started) {
            continue;
        }

        std::cout << "\nresult: " << (status == 0 ? "PASSED" : "FAILED")
                  << " (code " << static_cast<int>(status) << ")"
                  << " after " << frame + 1 << " frames\n";
        return status == 0 ? 0 : 1;

        const u8 character = bus.read(kOutputAddress);
        if (character != 0) {
            std::cout << static_cast<char>(character) << std::flush;
            bus.write(kOutputAddress, 0);
        }
    }

    std::cout << "\nresult: TIMEOUT after " << max_frames << " frames\n";
    return 1;
}

void usage()
{
    std::cout <<
        "usage: fc_testrom <rom.nes> [mode] [options]\n"
        "\n"
        "  --nestest N     run N instructions from $C000, logging each one in\n"
        "                  nestest.log format on stdout\n"
        "  --console N     run up to N frames, printing the test ROM's own\n"
        "                  output from $6004 and its result from $6001\n";
}

} // namespace

int main(int argc, char** argv)
{
    std::string path;
    int nestest_instructions = 0;
    int console_frames = 0;

    for (int i = 1; i < argc; ++i) {
        const std::string argument = argv[i];
        if (argument == "--nestest" && i + 1 < argc) {
            nestest_instructions = std::atoi(argv[++i]);
        } else if (argument == "--console" && i + 1 < argc) {
            console_frames = std::atoi(argv[++i]);
        } else if (argument == "--help" || argument == "-h") {
            usage();
            return 0;
        } else if (!argument.empty() && argument[0] != '-') {
            path = argument;
        }
    }

    if (path.empty() || (nestest_instructions == 0 && console_frames == 0)) {
        usage();
        return 2;
    }

    const auto rom = read_file(path);
    if (!rom) {
        std::cerr << "could not read " << path << "\n";
        return 2;
    }

    nes::Machine machine;
    std::string error;
    if (!machine.load_rom(*rom, error)) {
        std::cerr << "could not load " << path << ": " << error << "\n";
        return 2;
    }

    if (nestest_instructions > 0) {
        return run_nestest(machine, nestest_instructions);
    }
    return run_console(machine, console_frames);
}
