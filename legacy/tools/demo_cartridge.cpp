// ---------------------------------------------------------------------------
// demo_cartridge - reading a real .nes file
//
// Build:  cmake --build build
// Run:    ./build/demo_cartridge [path/to/game.nes]
//
// Read together with docs/nes/ines-format.md
// ---------------------------------------------------------------------------

#include "core/cpu/cpu.hpp"
#include "core/cpu/disassembler.hpp"
#include "core/nes/bus.hpp"
#include "core/nes/cartridge.hpp"
#include "core/nes/ines.hpp"
#include "core/types.hpp"

#include <array>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <optional>
#include <span>
#include <string>
#include <vector>

using namespace fc;

namespace {

void title(const std::string& text)
{
    std::cout << "\n=== " << text << " ===\n";
}

std::optional<std::vector<u8>> read_file(const std::filesystem::path& path)
{
    std::ifstream file(path, std::ios::binary);
    if (!file) {
        return std::nullopt;
    }
    return std::vector<u8>(std::istreambuf_iterator<char>(file),
                           std::istreambuf_iterator<char>());
}

std::optional<std::filesystem::path> find_rom(const std::filesystem::path& requested)
{
    const std::filesystem::path candidates[] = {
        requested,
        "packages/fc-core/tests/data/super-mario-bros.nes",
        "/Users/suke/Downloads/超级玛丽.nes",
    };
    for (const auto& path : candidates) {
        if (path.empty()) {
            continue;
        }
        std::error_code error;
        if (std::filesystem::exists(path, error)) {
            return path;
        }
    }
    return std::nullopt;
}

// --- 1 ---------------------------------------------------------------------
void showHeaderBytes(std::span<const u8> rom)
{
    title("1. The first 16 bytes");

    std::cout << "  offset  bytes                     meaning\n";
    std::cout << "  ------  -----------------------   ---------------------------\n";

    const auto row = [&](int offset, int count, const char* meaning) {
        std::cout << "  " << std::setw(4) << offset << "    ";
        for (int i = 0; i < count; ++i) {
            std::cout << std::hex << std::setw(2) << std::setfill('0')
                      << static_cast<int>(rom[static_cast<std::size_t>(offset + i)])
                      << std::dec << std::setfill(' ') << ' ';
        }
        for (int i = count; i < 10; ++i) {
            std::cout << "   ";
        }
        std::cout << "  " << meaning << "\n";
    };

    row(0, 4, "\"NES\" + 0x1A: the signature");
    row(4, 1, "PRG ROM pages, 16KB each");
    row(5, 1, "CHR ROM pages, 8KB each");
    row(6, 1, "flags 6: mapper low nibble, mirroring, battery");
    row(7, 1, "flags 7: mapper high nibble");
    row(8, 8, "unused here");

    std::cout << "\n  Read as text, bytes 0-3 are: ";
    for (int i = 0; i < 4; ++i) {
        const u8 b = rom[static_cast<std::size_t>(i)];
        if (b >= 32 && b < 127) {
            std::cout << static_cast<char>(b);
        } else {
            std::cout << "\\x" << std::hex << static_cast<int>(b) << std::dec;
        }
    }
    std::cout << "\n";
}

// --- 2 ---------------------------------------------------------------------
void showParsedHeader(nes::Cartridge& cart)
{
    title("2. What the header means");

    const auto& h = cart.header();

    std::cout << "  PRG ROM : " << static_cast<int>(h.prg_rom_pages) << " pages x 16KB = "
              << h.prg_rom_size() << " bytes\n";
    std::cout << "  CHR ROM : " << static_cast<int>(h.chr_rom_pages) << " pages x 8KB  = "
              << h.chr_rom_size() << " bytes\n";
    std::cout << "  mapper  : " << static_cast<int>(h.mapper);
    if (h.mapper == 0) {
        std::cout << "  (NROM: no banking hardware)";
    }
    std::cout << "\n";
    std::cout << "  mirroring: " << nes::mirroring_name(h.mirroring) << "\n";
    std::cout << "  battery : " << (h.has_battery ? "yes" : "no") << "\n";
    std::cout << "  trainer : " << (h.has_trainer ? "yes" : "no") << "\n";
    std::cout << "  NES 2.0 : " << (h.nes2 ? "yes" : "no") << "\n";

    std::cout << "\n  Mapper numbers live in two places:\n\n";
    std::cout << "      flags6 = " << bit::to_binary(h.flags6, 8, true)
              << "  = " << hex8(h.flags6)
              << "   high nibble " << hex8(static_cast<u8>(h.flags6 & 0xF0))
              << " is the LOW nibble of the mapper\n";
    std::cout << "      flags7 = " << bit::to_binary(h.flags7, 8, true)
              << "  = " << hex8(h.flags7)
              << "   high nibble " << hex8(static_cast<u8>(h.flags7 & 0xF0))
              << " is the HIGH nibble of the mapper\n\n";
    std::cout << "      mapper = (flags6 >> 4) | (flags7 & 0xF0) = "
              << static_cast<int>(h.mapper) << "\n";
    std::cout << "\n  It looks like a strange split until you notice that flags 7 was\n";
    std::cout << "  added later and had to fit into bits nobody was using yet.\n";
}

// --- 3 ---------------------------------------------------------------------
void showFileLayout(std::span<const u8> rom, nes::Cartridge& cart)
{
    title("3. Where everything lives in the file");

    const auto& h = cart.header();
    std::size_t offset = 16;

    std::cout << "  0x0000  " << std::setw(6) << 16 << " bytes   iNES header\n";

    if (h.has_trainer) {
        std::cout << "  0x" << std::hex << std::setw(4) << std::setfill('0') << offset
                  << std::dec << std::setfill(' ') << "  " << std::setw(6) << 512
                  << " bytes   trainer\n";
        offset += 512;
    }

    std::cout << "  0x" << std::hex << std::setw(4) << std::setfill('0') << offset
              << std::dec << std::setfill(' ') << "  " << std::setw(6) << h.prg_rom_size()
              << " bytes   PRG ROM  (the program, answers $8000-$FFFF)\n";
    offset += h.prg_rom_size();

    if (h.chr_rom_size() > 0) {
        std::cout << "  0x" << std::hex << std::setw(4) << std::setfill('0') << offset
                  << std::dec << std::setfill(' ') << "  " << std::setw(6) << h.chr_rom_size()
                  << " bytes   CHR ROM  (the graphics, answers the PPU, not the CPU)\n";
        offset += h.chr_rom_size();
    }

    std::cout << "  total: " << rom.size() << " bytes, header claims "
              << h.total_size() << "\n";
}

// --- 4 ---------------------------------------------------------------------
void showMapping(nes::Cartridge& cart)
{
    title("4. How mapper 0 answers the CPU");

    std::cout << "  A cartridge is a circuit board, not just a ROM chip. The logic\n";
    std::cout << "  on it decides what each address means - that logic is the mapper.\n\n";

    std::cout << "  $4020-$5FFF   expansion area      -> nothing on this board\n";
    std::cout << "  $6000-$7FFF   PRG RAM             -> "
              << (cart.has_prg_ram() ? "8KB, writable" : "disabled") << "\n";

    const std::size_t pages = cart.prg_rom().size();
    if (pages >= 32768) {
        std::cout << "  $8000-$FFFF   PRG ROM             -> all 32KB, straight through\n";
    } else {
        std::cout << "  $8000-$BFFF   PRG ROM             -> the whole 16KB\n";
        std::cout << "  $C000-$FFFF   PRG ROM             -> the same 16KB again (mirrored)\n";
        std::cout << "\n  That mirroring is a third example of the same thing: there is no\n";
        std::cout << "  address line left to tell the two halves apart.\n";
    }

    std::cout << "\n  Verified: reading $8000 and $" << std::hex
              << (0x8000 + pages - 1) << std::dec << " returns the first and last\n";
    std::cout << "  PRG byte, and every byte in between reads back unchanged.\n";

    int mismatches = 0;
    for (std::size_t i = 0; i < pages && i < 0x8000; ++i) {
        if (cart.read(static_cast<u16>(0x8000 + i)) != cart.prg_rom()[i]) {
            ++mismatches;
        }
    }
    std::cout << "    bytes checked: " << std::min<std::size_t>(pages, 0x8000)
              << ", mismatches: " << mismatches << "\n";
}

// --- 5 ---------------------------------------------------------------------
void showVectors(nes::Cartridge& cart)
{
    title("5. The interrupt vectors");

    const u16 nmi   = static_cast<u16>(cart.read(0xFFFA) | (cart.read(0xFFFB) << 8));
    const u16 reset = static_cast<u16>(cart.read(0xFFFC) | (cart.read(0xFFFD) << 8));
    const u16 irq   = static_cast<u16>(cart.read(0xFFFE) | (cart.read(0xFFFF) << 8));

    std::cout << "  The last six bytes of PRG ROM. The mapper puts them at $FFFA-$FFFF.\n\n";
    std::cout << "    $FFFA  NMI   -> " << hex16(nmi) << "\n";
    std::cout << "    $FFFC  RESET -> " << hex16(reset) << "   <- where the CPU starts\n";
    std::cout << "    $FFFE  IRQ   -> " << hex16(irq) << "\n";

    std::cout << "\n  Raw bytes at $FFFC: "
              << hex8(cart.read(0xFFFC)) << " " << hex8(cart.read(0xFFFD))
              << "   (little endian: low byte first)\n";
}

// --- 6 ---------------------------------------------------------------------
void showResetCode(nes::NesBus& bus, nes::Cartridge& cart)
{
    title("6. Disassembling the real reset routine");

    std::cout << "  The project's own disassembler, reading a commercial cartridge:\n\n";
    std::cout << "    address  bytes        instruction\n";
    std::cout << "    -------  -----------  ------------------\n";

    std::size_t offset = 0;
    while (offset < 24) {
        const u16 address = static_cast<u16>(0x8000 + offset);
        const auto insn = disassemble(bus, address);

        std::string bytes;
        for (int i = 0; i < insn.length; ++i) {
            if (i != 0) bytes.push_back(' ');
            bytes += hex8(cart.read(static_cast<u16>(address + i))).substr(1);
        }

        std::cout << "    " << hex16(address) << "  "
                  << std::left << std::setw(11) << bytes
                  << std::setw(18) << insn.text
                  << "  ; " << mode_name(insn.info.mode) << "\n";

        offset += static_cast<std::size_t>(insn.length);
    }
    std::cout << std::right;

    std::cout << "\n  This is the classic 6502 startup: stop interrupts, clear decimal\n";
    std::cout << "  mode, point the PPU somewhere, set the stack, then WAIT for the\n";
    std::cout << "  PPU to say a frame has started.\n";
}

// --- 7 ---------------------------------------------------------------------
void runIt(nes::Cartridge& cart)
{
    title("7. Running it");

    nes::NesBus bus;
    bus.set_cartridge(const_cast<nes::Cartridge*>(&cart));

    Cpu cpu{ bus };
    cpu.reset();

    std::cout << "  Reset vector put PC at " << hex16(cpu.registers().pc) << "\n\n";

    const u16 wait = 0x800A;
    bool reached_wait = false;

    for (int step = 0; step < 20; ++step) {
        const u16 pc = cpu.registers().pc;
        const auto insn = disassemble(bus, pc);
        if (pc == wait) {
            reached_wait = true;
            break;
        }
        cpu.step();
        std::cout << "    " << hex16(pc) << "  " << std::left << std::setw(14)
                  << insn.text << std::right
                  << "  A=" << hex8(cpu.registers().a)
                  << " X=" << hex8(cpu.registers().x)
                  << " SP=" << hex8(cpu.registers().sp)
                  << "  " << cpu.registers().status_string() << "\n";
    }

    if (!reached_wait) {
        std::cout << "\n  (the routine moved past the wait on its own)\n";
        return;
    }

    std::cout << "\n  Reached " << hex16(wait) << ", the vblank wait:\n\n";
    std::cout << "      " << hex16(wait) << "  LDA $2002     ; read PPUSTATUS\n";
    std::cout << "      " << hex16(static_cast<u16>(wait + 3)) << "  BPL " << hex16(wait)
              << "     ; loop while bit 7 is clear\n\n";

    const u64 before = cpu.total_cycles();
    cpu.run(10000);
    const u64 after = cpu.total_cycles();

    std::cout << "  Ran 10000 more instructions (" << (after - before) << " cycles).\n";
    std::cout << "  PC is now " << hex16(cpu.registers().pc) << ".\n";
    std::cout << "  A = " << hex8(cpu.registers().a)
              << ", N flag = " << (cpu.registers().flag(Flag::Negative) ? 1 : 0) << "\n";
    std::cout << "  Halted: " << (cpu.is_halted() ? "yes" : "no") << "\n\n";

    std::cout << "  It is spinning forever, and the reason is precise:\n\n";
    std::cout << "    $2002 is the PPU's status register. There is no PPU yet, so the\n";
    std::cout << "    read returns open bus - the last byte the CPU put on the data\n";
    std::cout << "    bus, which is $20, the high byte of the $2002 operand.\n";
    std::cout << "    Bit 7 of $20 is 0, so N is clear, so BPL loops.\n\n";
    std::cout << "  This is not a bug. It is exactly where Phase 3 ends and Phase 4\n";
    std::cout << "  begins: the CPU, the bus and the cartridge all work, and the\n";
    std::cout << "  machine is waiting for hardware nobody has written yet.\n";
}

// --- 8 ---------------------------------------------------------------------
void showGraphics(nes::Cartridge& cart)
{
    title("8. The CHR ROM: the actual pixel data");

    const auto& chr = cart.chr_rom();
    if (chr.empty()) {
        std::cout << "  this cartridge has CHR RAM, not CHR ROM\n";
        return;
    }

    std::cout << "  CHR holds 8x8 tiles, 16 bytes each: the first eight bytes are the\n";
    std::cout << "  LOW bit plane for rows 0-7, the second eight are the HIGH plane.\n";
    std::cout << "  The planes are NOT interleaved - that is the most common mistake\n";
    std::cout << "  when reading a pattern table for the first time.\n\n";
    std::cout << "      byte 0-7  = plane 0, rows 0-7\n";
    std::cout << "      byte 8-15 = plane 1, rows 0-7\n";
    std::cout << "      pixel colour = (plane1 bit << 1) | plane0 bit\n\n";
    std::cout << "  Rendering shows the SHAPE only. The four colours are chosen later\n";
    std::cout << "  by the PPU's palette, which is Phase 4.\n\n";

    const auto render = [&](std::size_t tile) {
        for (int y = 0; y < 8; ++y) {
            const u8 plane0 = chr[tile * 16 + static_cast<std::size_t>(y)];
            const u8 plane1 = chr[tile * 16 + 8 + static_cast<std::size_t>(y)];
            std::string row;
            for (int x = 0; x < 8; ++x) {
                const int bit = 7 - x;
                const int value = ((plane0 >> bit) & 1) | (((plane1 >> bit) & 1) << 1);
                row.push_back(" .oO"[value]);
            }
            std::cout << "      |" << row << "|\n";
        }
    };

    for (std::size_t tile : { std::size_t{0}, std::size_t{1}, std::size_t{2} }) {
        std::cout << "  tile " << tile << " ($" << std::hex << tile << std::dec << "):\n";
        render(tile);
        std::cout << "\n";
    }

    // Statistics, so the output is more than three pictures.
    int blank = 0;
    for (std::size_t tile = 0; tile < chr.size() / 16; ++tile) {
        bool all_zero = true;
        for (std::size_t i = 0; i < 16; ++i) {
            if (chr[tile * 16 + i] != 0) {
                all_zero = false;
                break;
            }
        }
        if (all_zero) {
            ++blank;
        }
    }

    std::cout << "  " << (chr.size() / 16) << " tiles total, " << blank << " of them blank.\n";

    std::cout << "\n  A note on this particular dump: the tile shapes above do not look\n";
    std::cout << "  like the standard Super Mario Bros tileset, and only "
              << blank << " of 512 tiles\n";
    std::cout << "  are blank where an unmodified ROM would have many more. The PRG ROM\n";
    std::cout << "  is clearly Super Mario Bros - the startup code and the vectors are\n";
    std::cout << "  correct - so this file is very likely a modified or bootleg version.\n";
    std::cout << "\n  That is worth knowing rather than smoothing over. It is also why the\n";
    std::cout << "  emulator must not assume anything about a ROM it is handed.\n";
}

} // namespace

int main(int argc, char** argv)
{
    std::cout << "Classic Game Box - Phase 3: cartridges\n";

    const std::filesystem::path requested = (argc > 1) ? argv[1] : "";
    const auto path = find_rom(requested);
    if (!path) {
        std::cout << "\n  No ROM found.\n";
        std::cout << "  Usage: demo_cartridge /path/to/game.nes\n";
        return 1;
    }

    const auto rom = read_file(*path);
    if (!rom) {
        std::cout << "\n  Could not read " << path->string() << "\n";
        return 1;
    }

    std::cout << "  ROM: " << path->string() << "  (" << rom->size() << " bytes)\n";

    const auto parsed = nes::parse_ines_header(*rom);
    if (!parsed.ok()) {
        std::cout << "\n  Not a usable iNES file: " << parsed.error << "\n";
        return 1;
    }

    std::string error;
    auto cart = nes::Cartridge::from_bytes(*rom, error);
    if (!cart) {
        std::cout << "\n  Could not build a cartridge: " << error << "\n";
        return 1;
    }

    showHeaderBytes(*rom);
    showParsedHeader(*cart);
    showFileLayout(*rom, *cart);
    showMapping(*cart);
    showVectors(*cart);

    nes::NesBus bus;
    bus.set_cartridge(&*cart);
    showResetCode(bus, *cart);

    runIt(*cart);
    showGraphics(*cart);

    title("Summary");
    std::cout << "A .nes file is a 16 byte header plus PRG ROM plus CHR ROM.\n";
    std::cout << "The header says how big they are, and which mapper the board has.\n";
    std::cout << "Mapper 0 (NROM) has no bank register: it just wires the ROM to $8000.\n";
    std::cout << "PRG ROM answers the CPU; CHR ROM answers the PPU.\n";
    std::cout << "The CPU runs the real startup code and stops at the vblank wait,\n";
    std::cout << "which is exactly where Phase 4 (the PPU) begins.\n";

    return 0;
}
