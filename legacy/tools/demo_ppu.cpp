// ---------------------------------------------------------------------------
// demo_ppu - running a real game far enough to see a picture
//
// Build:  cmake --build build
// Run:    ./build/demo_ppu [path/to/game.nes] [frames] [output-dir]
//
// Read together with docs/nes/ppu.md
// ---------------------------------------------------------------------------

#include "core/cpu/disassembler.hpp"
#include "core/nes/framebuffer.hpp"
#include "core/nes/machine.hpp"
#include "core/types.hpp"

#include <cstdio>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <optional>
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

/// A binary PPM (P6). Every image tool on the planet reads it, and it is
/// about eight lines of code, which is the right amount of file format for a
/// project whose real output is a 240KB array.
bool write_ppm(const std::filesystem::path& path, const nes::Framebuffer& fb)
{
    std::ofstream out(path, std::ios::binary);
    if (!out) {
        return false;
    }
    out << "P6\n" << nes::Framebuffer::kWidth << ' ' << nes::Framebuffer::kHeight << "\n255\n";
    for (u32 pixel : fb.pixels) {
        const char rgb[3] = {
            static_cast<char>((pixel >> 16) & 0xFF),
            static_cast<char>((pixel >> 8) & 0xFF),
            static_cast<char>(pixel & 0xFF),
        };
        out.write(rgb, 3);
    }
    return true;
}

/// Render a coarse ASCII preview, so the terminal shows something even
/// without opening the image. 64x30 characters, two luminance levels per cell
/// is overkill; one is enough to see whether a picture appeared.
void print_ascii_preview(const nes::Framebuffer& fb, int columns = 64, int rows = 30)
{
    static const char* ramp = " .:-=+*#%@";

    for (int row = 0; row < rows; ++row) {
        std::cout << "  |";
        for (int col = 0; col < columns; ++col) {
            const int x = col * nes::Framebuffer::kWidth / columns;
            const int y = row * nes::Framebuffer::kHeight / rows;
            const u32 pixel = fb.at(x, y);

            const int r = static_cast<int>((pixel >> 16) & 0xFF);
            const int g = static_cast<int>((pixel >> 8) & 0xFF);
            const int b = static_cast<int>(pixel & 0xFF);
            const int luma = (r * 30 + g * 59 + b * 11) / 100;

            std::cout << ramp[luma * 9 / 255];
        }
        std::cout << "|\n";
    }
}

void report_state(nes::Machine& machine, int frame)
{
    const auto& cpu = machine.cpu();
    const auto& ppu = machine.ppu();

    std::cout << "\n  frame " << frame
              << "   PPU scanline " << std::setw(3) << ppu.scanline()
              << " dot " << std::setw(3) << ppu.dot()
              << "   CPU cycles " << cpu.total_cycles() << "\n";
    std::cout << "    PPUCTRL   $" << std::hex << std::setw(2) << std::setfill('0')
              << static_cast<int>(ppu.control()) << std::setfill(' ') << std::dec
              << "   NMI on: " << ((ppu.control() & 0x80) ? "yes" : "no")
              << "   sprite table: " << ((ppu.control() & 0x08) ? "$1000" : "$0000")
              << "   bg table: " << ((ppu.control() & 0x10) ? "$1000" : "$0000") << "\n";
    std::cout << "    PPUMASK   $" << std::hex << std::setw(2) << std::setfill('0')
              << static_cast<int>(ppu.mask()) << std::setfill(' ') << std::dec
              << "   background: " << ((ppu.mask() & 0x08) ? "on " : "off")
              << "   sprites: " << ((ppu.mask() & 0x10) ? "on " : "off") << "\n";
    std::cout << "    PPUSTATUS $" << std::hex << std::setw(2) << std::setfill('0')
              << static_cast<int>(ppu.status()) << std::setfill(' ') << std::dec
              << "   vblank: " << ((ppu.status() & 0x80) ? 1 : 0)
              << "   sprite0: " << ((ppu.status() & 0x40) ? 1 : 0)
              << "   overflow: " << ((ppu.status() & 0x20) ? 1 : 0) << "\n";
    std::cout << "    VRAM addr $" << std::hex << std::setw(4) << std::setfill('0')
              << ppu.vram_address() << std::setfill(' ') << std::dec
              << "   sprites on this line: " << ppu.sprites_on_scanline() << "\n";
    std::cout << "    PC        $" << std::hex << std::setw(4) << std::setfill('0')
              << cpu.registers().pc << std::setfill(' ') << std::dec
              << "   A $" << std::hex << std::setw(2) << std::setfill('0')
              << static_cast<int>(cpu.registers().a) << std::setfill(' ') << std::dec
              << "   halted: " << (cpu.is_halted() ? "yes" : "no") << "\n";
}

void showPalette(nes::Machine& machine)
{
    title("Palette RAM as the game left it");

    std::cout << "  $3F00 universal background      : $"
              << std::hex << std::setw(2) << std::setfill('0')
              << static_cast<int>(machine.ppu().palette_ram(0x00)) << std::setfill(' ')
              << std::dec << "\n";
    std::cout << "  $3F01-$3F03 background palette 0: ";
    for (int i = 1; i <= 3; ++i) {
        std::cout << "$" << std::hex << std::setw(2) << std::setfill('0')
                  << static_cast<int>(machine.ppu().palette_ram(static_cast<u8>(i)))
                  << std::setfill(' ') << std::dec << " ";
    }
    std::cout << "\n";
    std::cout << "  $3F11-$3F13 sprite palette 0    : ";
    for (int i = 0x11; i <= 0x13; ++i) {
        std::cout << "$" << std::hex << std::setw(2) << std::setfill('0')
                  << static_cast<int>(machine.ppu().palette_ram(static_cast<u8>(i)))
                  << std::setfill(' ') << std::dec << " ";
    }
    std::cout << "\n";

    std::cout << "\n  Colour swatches (index -> RGB):\n";
    for (int i = 0; i < 8; ++i) {
        const u8 index = machine.ppu().palette_ram(static_cast<u8>(i));
        const u32 rgb = nes::Ppu::colour(index);
        std::cout << "    $3F0" << i << " = $" << std::hex << std::setw(2)
                  << std::setfill('0') << static_cast<int>(index) << std::setfill(' ')
                  << std::dec << "  ->  #" << std::hex << std::setw(6)
                  << std::setfill('0') << rgb << std::setfill(' ') << std::dec << "\n";
    }
}

} // namespace

int main(int argc, char** argv)
{
    std::cout << "Classic Game Box - Phase 4: the PPU\n";

    const std::filesystem::path requested = (argc > 1) ? argv[1] : "";
    const auto path = find_rom(requested);
    if (!path) {
        std::cout << "\n  No ROM found. Usage: demo_ppu /path/to/game.nes [frames]\n";
        return 1;
    }

    const int frames_to_run = (argc > 2) ? std::atoi(argv[2]) : 60;
    const std::filesystem::path out_dir = (argc > 3) ? argv[3] : "frames";

    const auto rom = read_file(*path);
    if (!rom) {
        std::cout << "\n  Could not read " << path->string() << "\n";
        return 1;
    }

    std::cout << "  ROM: " << path->string() << "  (" << rom->size() << " bytes)\n";

    nes::Machine machine;
    std::string error;
    if (!machine.load_rom(*rom, error)) {
        std::cout << "\n  Could not load: " << error << "\n";
        return 1;
    }
    std::cout << "  " << machine.cartridge()->summary() << "\n";

    title("1. Power on");

    machine.reset();
    std::cout << "  PPU starting state: scanline " << machine.ppu().scanline()
              << ", dot " << machine.ppu().dot() << "\n";
    std::cout << "  The pre-render line (-1) comes first, so no visible scanline has\n";
    std::cout << "  been drawn yet.\n";

    title("2. Running");

    std::error_code fs_error;
    std::filesystem::create_directories(out_dir, fs_error);

    const std::vector<int> snapshot_frames = { 1, 5, 30, frames_to_run };

    for (int frame = 1; frame <= frames_to_run; ++frame) {
        const bool ok = machine.run_frame();
        if (!ok) {
            std::cout << "  frame " << frame << ": the CPU halted on an illegal opcode $"
                      << std::hex << std::setw(2) << std::setfill('0')
                      << static_cast<int>(machine.cpu().unimplemented_opcode())
                      << std::setfill(' ') << std::dec << ". That is an emulator bug.\n";
            return 1;
        }

        for (int snapshot : snapshot_frames) {
            if (frame == snapshot) {
                report_state(machine, frame);

                if (frame == snapshot_frames.back()) {
                    std::cout << "\n  The game's own picture, rendered by the PPU:\n\n";
                    print_ascii_preview(machine.framebuffer());
                }

                const std::string name = "frame_" + std::to_string(frame) + ".ppm";
                const auto file = out_dir / name;
                if (write_ppm(file, machine.framebuffer())) {
                    std::cout << "\n  wrote " << file.string() << "\n";
                }
            }
        }
    }

    title("3. What the game put in the PPU's memory");

    showPalette(machine);

    // Show the top-left corner of the nametable, which is what the game
    // arranged the screen out of.
    std::cout << "\n  Nametable $2000, first four rows (tile indices):\n";
    for (int row = 0; row < 4; ++row) {
        std::cout << "    ";
        for (int col = 0; col < 32; ++col) {
            const u16 address = static_cast<u16>(0x2000 + row * 32 + col);
            std::cout << std::hex << std::setw(2) << std::setfill('0')
                      << static_cast<int>(machine.ppu().read_vram(address))
                      << std::setfill(' ') << std::dec << " ";
        }
        std::cout << "\n";
    }

    title("Summary");
    std::cout << "The PPU has its own address space, its own memory, and its own\n";
    std::cout << "clock at three times the CPU's.\n";
    std::cout << "The CPU can only see eight registers at $2000-$2007.\n";
    std::cout << "Scrolling is five fields of a 15 bit address, copied between\n";
    std::cout << "two registers at fixed moments in the frame.\n";
    std::cout << "The picture above came out of the real ROM's own data.\n";

    return 0;
}
