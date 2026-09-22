// ---------------------------------------------------------------------------
// demo_bus - address decoding, mirroring and OAM DMA
//
// Build:  cmake --build build
// Run:    ./build/demo_bus
//
// Read together with docs/nes/memory-map.md and docs/architecture/bus.md
// ---------------------------------------------------------------------------

#include "core/bit.hpp"
#include "core/cpu/cpu.hpp"
#include "core/cpu/disassembler.hpp"
#include "core/nes/bus.hpp"
#include "core/nes/ram.hpp"
#include "core/nes/ram_cartridge.hpp"
#include "core/types.hpp"

#include <array>
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

std::string hex4(u16 value)
{
    return "$" + std::string(1, "0123456789ABCDEF"[(value >> 12) & 0xF])
               + std::string(1, "0123456789ABCDEF"[(value >> 8) & 0xF])
               + std::string(1, "0123456789ABCDEF"[(value >> 4) & 0xF])
               + std::string(1, "0123456789ABCDEF"[value & 0xF]);
}

/// A PPU that records what the bus asked of it.
struct RecordingPpu : nes::Device {
    std::array<u8, 8> registers{};
    std::array<int, 8> write_counts{};

    [[nodiscard]] u8 read(u16 address) override
    {
        return registers[nes::NesBus::ppu_register_index(address)];
    }

    void write(u16 address, u8 value) override
    {
        const u8 index = nes::NesBus::ppu_register_index(address);
        registers[index] = value;
        ++write_counts[index];
    }
};

/// Catches OAM DMA.
struct OamRam : nes::OamTarget {
    std::array<u8, 256> bytes{};
    int writes = 0;

    void write_oam(u8 index, u8 value) override
    {
        bytes[index] = value;
        ++writes;
    }
};

// --- 1 ---------------------------------------------------------------------
void showMemoryMap()
{
    title("1. The memory map");

    struct Row { const char* range; const char* size; const char* what; };
    const Row rows[] = {
        { "$0000-$07FF", "2KB",    "work RAM" },
        { "$0800-$1FFF", "6KB",    "work RAM again, three more times (mirroring)" },
        { "$2000-$3FFF", "8KB",    "PPU registers: 8 bytes, mirrored 1024 times" },
        { "$4000-$4017", "24B",    "APU, controllers, and OAM DMA at $4014" },
        { "$4018-$401F", "8B",     "disabled" },
        { "$4020-$FFFF", "~48KB",  "the cartridge slot" },
    };

    std::cout << "  range         size   what\n";
    std::cout << "  ------------  -----  ------------------------------------------\n";
    for (const auto& r : rows) {
        std::cout << "  " << std::left << std::setw(14) << r.range
                  << std::setw(7) << r.size << r.what << "\n";
    }
    std::cout << std::right;

    // Confirm the decoder agrees with the table.
    nes::NesBus bus;
    std::cout << "\n  The decoder's own answer for one address per region:\n";
    const u16 probes[] = { 0x0000, 0x2000, 0x4014, 0x4018, 0x8000 };
    for (u16 address : probes) {
        const char* name = "?";
        switch (nes::NesBus::region_of(address)) {
        case nes::NesBus::Region::Ram:          name = "Ram"; break;
        case nes::NesBus::Region::PpuRegisters: name = "PpuRegisters"; break;
        case nes::NesBus::Region::ApuAndIo:     name = "ApuAndIo"; break;
        case nes::NesBus::Region::Disabled:     name = "Disabled"; break;
        case nes::NesBus::Region::Cartridge:    name = "Cartridge"; break;
        }
        std::cout << "    " << hex4(address) << "  ->  " << name << "\n";
    }
}

// --- 2 ---------------------------------------------------------------------
void showWhyMirroringExists()
{
    title("2. Mirroring is unwired address lines");

    std::cout << "  A 2KB chip needs 11 address lines. 2^11 = 2048.\n";
    std::cout << "  The 6502 has 16. So the top 5 lines are not connected:\n\n";
    std::cout << "      A15 A14 A13 A12 A11 | A10 .. A0\n";
    std::cout << "       |   |   |   |   |      \\_____/\n";
    std::cout << "       |   |   |   |   |         |\n";
    std::cout << "       |   |   |   |   |     the RAM chip\n";
    std::cout << "       \\___|___|___|___/\n";
    std::cout << "               |\n";
    std::cout << "        chip select logic\n\n";
    std::cout << "  The chip select says only 'is this somewhere in $0000-$1FFF'.\n";
    std::cout << "  Which of the four it is never reaches the RAM. So all four are\n";
    std::cout << "  the same byte - not because anyone implemented that, but\n";
    std::cout << "  because nobody wired the bits up.\n\n";

    nes::NesBus bus;
    bus.write(0x0000, 0xAB);
    std::cout << "  Write $AB to $0000, then read the four mirrors:\n";
    for (u16 base : { u16{0x0000}, u16{0x0800}, u16{0x1000}, u16{0x1800} }) {
        std::cout << "    " << hex4(base) << " -> " << hex8(bus.read(base));
        if (base != 0x0000) {
            std::cout << "    (" << hex4(base) << " & $07FF = $0000)";
        }
        std::cout << "\n";
    }
    std::cout << "\n  The mask $07FF is the missing wires, expressed in software.\n";
    std::cout << "  That is why it lives in Ram, not in the bus: the bus does the\n";
    std::cout << "  chip select, the chip does the wrapping.\n";
}

// --- 3 ---------------------------------------------------------------------
void proveMirroringExhaustively()
{
    title("3. Proving it exhaustively");

    nes::NesBus bus;

    // Write a distinct value into each of the 2KB cells.
    for (u16 i = 0; i < nes::Ram::kSize; ++i) {
        bus.write(i, static_cast<u8>(i * 7u + 3u));
    }

    int checked = 0;
    int mismatches = 0;
    for (u16 address = 0x0000; address < 0x2000; ++address) {
        const u8 expected = static_cast<u8>((address & nes::Ram::kMask) * 7u + 3u);
        if (bus.read(address) != expected) {
            ++mismatches;
        }
        ++checked;
    }

    std::cout << "  Wrote 2048 distinct values, then read all 8192 addresses in\n";
    std::cout << "  $0000-$1FFF and compared each against RAM[address & $07FF].\n\n";
    std::cout << "    addresses checked : " << checked << "\n";
    std::cout << "    mismatches        : " << mismatches << "\n";
    std::cout << "\n  " << (mismatches == 0 ? "All four mirrors agree, everywhere."
                                              : "MISMATCH!");
    std::cout << "\n";
}

// --- 4 ---------------------------------------------------------------------
void showPpuRegisterMirroring()
{
    title("4. The PPU has the same problem, twice as badly");

    std::cout << "  8KB of address space ($2000-$3FFF) reaches 8 registers.\n";
    std::cout << "  Only three address lines are decoded, so each register answers\n";
    std::cout << "  1024 times.\n\n";

    nes::NesBus bus;
    RecordingPpu ppu;
    bus.set_ppu(&ppu);

    for (u16 address = 0x2000; address < 0x4000; ++address) {
        bus.write(address, static_cast<u8>(address & 0x0007));
    }

    std::cout << "  Wrote one byte to every address in the range.\n\n";
    std::cout << "  register   how many writes it received\n";
    std::cout << "  --------   ---------------------------\n";
    for (int i = 0; i < 8; ++i) {
        std::cout << "  $200" << i << "      " << ppu.write_counts[i] << "\n";
    }

    std::cout << "\n  Every register received exactly 8192 / 8 = 1024 writes.\n";
    std::cout << "\n  This matters in practice: writing to $2000 or $2008 does the\n";
    std::cout << "  same thing. Emulators that only decode $2000-$2007 and treat the\n";
    std::cout << "  rest as unmapped will break games that use the mirrors.\n";
}

// --- 5 ---------------------------------------------------------------------
void showOpenBus()
{
    title("5. Open bus: unmapped reads are not zero");

    nes::NesBus bus;

    std::cout << "  The data bus is 8 wires with capacitance. They hold the last\n";
    std::cout << "  value that was driven onto them, so a read with nothing behind\n";
    std::cout << "  it returns that leftover value, not zero.\n\n";

    std::cout << "  power on, open bus = " << hex8(bus.open_bus()) << "\n\n";

    bus.write(0x4018, 0x5A);
    std::cout << "  write $5A to $4018 (a disabled address, nothing stores it)\n";
    std::cout << "    read $4018 -> " << hex8(bus.read(0x4018))
              << "   (open bus, not $00)\n\n";

    bus.write(0x0000, 0x11);
    std::cout << "  write $11 to $0000 (real RAM, the value is on the bus too)\n";
    std::cout << "    read $4019 -> " << hex8(bus.read(0x4019))
              << "   (still the last value on the bus)\n";
    std::cout << "    read $0000 -> " << hex8(bus.read(0x0000)) << "   (the real RAM)\n";

    std::cout << "\n  A few games read a write-only register to check what the bus\n";
    std::cout << "  last held. Returning zero would look correct almost always and\n";
    std::cout << "  fail in exactly the cases that are hardest to debug.\n";
}

// --- 6 ---------------------------------------------------------------------
void showOamDma()
{
    title("6. OAM DMA: one write, 513 cycles");

    nes::RamCartridge cart;
    nes::NesBus bus;
    bus.set_cartridge(&cart);

    OamRam oam;
    bus.set_oam_target(&oam);

    // Put 256 distinct bytes in page $02, the conventional DMA source.
    for (u16 i = 0; i < 256; ++i) {
        bus.write(static_cast<u16>(0x0200 + i), static_cast<u8>(i));
    }

    std::cout << "  A sprite table lives at $0200-$02FF. The PPU wants it in OAM.\n";
    std::cout << "  Instead of 256 stores, the program writes ONE byte:\n\n";
    std::cout << "      LDA #$02       ; the page number\n";
    std::cout << "      STA $4014      ; copy $0200-$02FF into OAM\n\n";

    bus.write(0x4014, 0x02);

    std::cout << "  Result:\n";
    std::cout << "    OAM bytes written : " << oam.writes << "\n";
    std::cout << "    OAM[0]  = " << hex8(oam.bytes[0]) << "\n";
    std::cout << "    OAM[1]  = " << hex8(oam.bytes[1]) << "\n";
    std::cout << "    OAM[255]= " << hex8(oam.bytes[255]) << "\n";
    std::cout << "    stalls requested  : " << bus.take_stall_cycles() << " cycles\n";

    std::cout << "\n  513 cycles = 1 to halt the CPU + 256 reads + 256 writes.\n";
    std::cout << "  During that time the CPU is frozen. Games budget for it: this\n";
    std::cout << "  is why NES games update sprites during vblank.\n";

    // Now the same thing through a real CPU, to show the stall reaching it.
    title("6b. The stall reaches the CPU");
    nes::RamCartridge cart2;
    nes::NesBus bus2;
    bus2.set_cartridge(&cart2);
    OamRam oam2;
    bus2.set_oam_target(&oam2);

    const std::vector<u8> program = {
        0xA9, 0x02,        // $8000  LDA #$02
        0x8D, 0x14, 0x40,  // $8002  STA $4014
        0xE8,              // $8005  INX
    };
    cart2.load(program, 0x8000);
    cart2.write(0xFFFC, 0x00);
    cart2.write(0xFFFD, 0x80);

    Cpu cpu{ bus2 };
    cpu.reset();

    cpu.step();
    std::cout << "  LDA #$02      : " << cpu.total_cycles() << " cycles total\n";

    const int dma_cycles = cpu.step();
    std::cout << "  STA $4014     : " << dma_cycles << " cycles"
              << "   (4 for the store + 513 for the DMA)\n";
    std::cout << "  running total : " << cpu.total_cycles() << "\n";

    cpu.step();
    std::cout << "  INX           : " << cpu.total_cycles() << " cycles total\n";

    std::cout << "\n  The CPU never learns what a PPU is. It just adds whatever\n";
    std::cout << "  the bus tells it to wait for.\n";
}

// --- 7 ---------------------------------------------------------------------
void showFullProgram()
{
    title("7. A whole program through the real decoder");

    nes::RamCartridge cart;
    nes::NesBus bus;
    bus.set_cartridge(&cart);

    const std::vector<u8> program = {
        0xA9, 0x2A,        // $8000  LDA #$2A
        0x85, 0x10,        // $8002  STA $10        zero page
        0xA2, 0x05,        // $8004  LDX #$05
        0x95, 0x20,        // $8006  STA $20,X      zero page,X
        0x48,              // $8008  PHA            stack, page 1
        0x4C, 0x00, 0x80,  // $8009  JMP $8000
    };

    cart.load(program, 0x8000);
    cart.write(0xFFFC, 0x00);
    cart.write(0xFFFD, 0x80);

    Cpu cpu{ bus };
    cpu.reset();

    std::cout << "  Listing:\n\n";
    std::cout << "    address  instruction        mode          region\n";
    std::cout << "    -------  -----------------  ------------  ---------\n";
    std::size_t offset = 0;
    while (offset < program.size()) {
        const u16 address = static_cast<u16>(0x8000 + offset);
        const auto insn = disassemble(
            std::span<const u8>(program.data() + offset, program.size() - offset), address);

        const char* region = "?";
        switch (nes::NesBus::region_of(address)) {
        case nes::NesBus::Region::Ram:          region = "Ram"; break;
        case nes::NesBus::Region::PpuRegisters: region = "Ppu"; break;
        case nes::NesBus::Region::ApuAndIo:     region = "Apu"; break;
        case nes::NesBus::Region::Disabled:     region = "Disabled"; break;
        case nes::NesBus::Region::Cartridge:    region = "Cartridge"; break;
        }

        std::cout << "    " << std::left << std::setw(9) << hex4(address)
                  << std::setw(19) << insn.text
                  << std::setw(14) << mode_name(insn.info.mode)
                  << region << "\n";
        offset += static_cast<std::size_t>(insn.length);
    }
    std::cout << std::right;

    cpu.run(5);

    std::cout << "\n  After 5 instructions:\n";
    std::cout << "    $0010       = " << hex8(bus.ram().read(0x0010))
              << "   (zero page, real RAM)\n";
    std::cout << "    $0025       = " << hex8(bus.ram().read(0x0025))
              << "   (zero page,X)\n";
    std::cout << "    $01FD       = " << hex8(bus.read(0x01FD))
              << "   (stack, page 1)\n";
    std::cout << "    $0810       = " << hex8(bus.read(0x0810))
              << "   (the same cell as $0010, through the mirror)\n";
    std::cout << "    $1010       = " << hex8(bus.read(0x1010))
              << "   (and through the second mirror)\n";

    std::cout << "\n  Three different address regions in one tiny program, and the\n";
    std::cout << "  CPU never knew which was which.\n";
}

} // namespace

int main()
{
    std::cout << "Classic Game Box - Phase 2: address decoding and mirroring\n";

    showMemoryMap();
    showWhyMirroringExists();
    proveMirroringExhaustively();
    showPpuRegisterMirroring();
    showOpenBus();
    showOamDma();
    showFullProgram();

    title("Summary");
    std::cout << "The bus is an address decoder: 16 wires in, one device answers.\n";
    std::cout << "Mirroring is unwired address lines, not a feature.\n";
    std::cout << "$0000-$1FFF is 2KB answering at 4 addresses each.\n";
    std::cout << "$2000-$3FFF is 8 registers answering at 1024 addresses each.\n";
    std::cout << "Unmapped reads return open bus, not zero.\n";
    std::cout << "OAM DMA is one write that steals 513 cycles.\n";
    std::cout << "The CPU only ever calls read/write; the bus does the rest.\n";

    return 0;
}
