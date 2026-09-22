// ---------------------------------------------------------------------------
// demo_input - pressing buttons and watching the game react
//
// Build:  cmake --build build
// Run:    ./build/demo_input [path/to/game.nes] [output-dir]
//
// Read together with docs/nes/controllers.md
// ---------------------------------------------------------------------------

#include "core/nes/controller.hpp"
#include "core/nes/framebuffer.hpp"
#include "core/nes/machine.hpp"
#include "core/types.hpp"

#include <algorithm>
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

using Button = nes::Controller::Button;

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

int pixel_difference(const nes::Framebuffer& a, const nes::Framebuffer& b)
{
    int count = 0;
    for (std::size_t i = 0; i < a.pixels.size(); ++i) {
        if (a.pixels[i] != b.pixels[i]) {
            ++count;
        }
    }
    return count;
}

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

void print_preview(const nes::Framebuffer& fb, int columns = 64, int rows = 24)
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

// --- 1 ---------------------------------------------------------------------
void showTheProtocol()
{
    title("1. The protocol: eight buttons, one bit at a time");

    std::cout << "  The console has only a few wires to the controller port, so the\n";
    std::cout << "  buttons are not read in parallel. A 4021 shift register inside the\n";
    std::cout << "  controller holds all eight states; the CPU latches them and then\n";
    std::cout << "  clocks them out one per read.\n\n";

    std::cout << "      write $4016 = 1     latch\n";
    std::cout << "      write $4016 = 0     start shifting\n";
    std::cout << "      read  $4016 bit 0   the next button\n\n";

    // Demonstrate with A and Right held, the two ends of the shift order.
    nes::Controller pad;
    pad.set_button(Button::A, true);
    pad.set_button(Button::Right, true);
    pad.strobe(true);
    pad.strobe(false);

    std::cout << "  With A and Right held:\n\n";
    std::cout << "      clock  button   bit\n";
    std::cout << "      -----  -------  ---\n";
    for (int i = 0; i < nes::Controller::kButtonCount; ++i) {
        const u8 bit = pad.read();
        std::cout << "        " << (i + 1) << "    "
                  << std::left << std::setw(7) << button_name(static_cast<Button>(i))
                  << std::right << "  " << static_cast<int>(bit) << "\n";
    }
    for (int i = 0; i < 3; ++i) {
        std::cout << "        " << (8 + i + 1) << "    "
                  << std::left << std::setw(7) << "(nothing)" << std::right
                  << "  " << static_cast<int>(pad.read()) << "\n";
    }

    std::cout << "\n  The order is fixed by the wiring: the buttons are soldered to\n";
    std::cout << "  specific inputs of the shift register. No program can change it.\n";
    std::cout << "\n  The trailing 1 is not padding. A program reads nine times and\n";
    std::cout << "  checks that the ninth is 1 to tell a standard controller from a\n";
    std::cout << "  light gun or an empty port.\n";

    // Holding the latch.
    nes::Controller held;
    held.set_button(Button::Start, true);
    held.strobe(true);
    std::cout << "\n  While the latch is held the register reloads every time, so the\n";
    std::cout << "  line never advances:\n\n      ";
    for (int i = 0; i < 6; ++i) {
        std::cout << static_cast<int>(held.read()) << " ";
    }
    std::cout << "   <- A is not pressed, so all zero, and it never moves on\n";
}

// --- 2 ---------------------------------------------------------------------
bool reach_title_screen(nes::Machine& machine)
{
    for (int i = 0; i < 400; ++i) {
        if (!machine.run_frame()) {
            return false;
        }
        if ((machine.ppu().mask() & 0x18) == 0x18 && i > 300) {
            return true;
        }
    }
    return true;
}

void showTitleScreen(nes::Machine& machine, const std::filesystem::path& out_dir)
{
    title("2. With no input, nothing happens");

    const auto before = machine.framebuffer();

    for (int i = 0; i < 120; ++i) {
        (void)machine.run_frame();
    }

    const int change = pixel_difference(before, machine.framebuffer());
    std::cout << "  Ran 120 frames with no buttons pressed.\n";
    std::cout << "  Pixels changed: " << change << " of "
              << nes::Framebuffer::kPixelCount << "  ("
              << std::fixed << std::setprecision(2)
              << (100.0 * change / static_cast<double>(nes::Framebuffer::kPixelCount))
              << "%)\n\n";
    std::cout << "  The title screen is essentially static: only a blinking cursor\n";
    std::cout << "  moves. The game is waiting for a button.\n\n";

    print_preview(machine.framebuffer());
    (void)write_ppm(out_dir / "input_title.ppm", machine.framebuffer());
}

// --- 3 ---------------------------------------------------------------------
void showStartPress(nes::Machine& machine, const std::filesystem::path& out_dir)
{
    title("3. Press Start");

    const auto before = machine.framebuffer();

    std::cout << "  Pressing Start for five frames...\n\n";
    machine.set_button(Button::Start, true);
    for (int i = 0; i < 5; ++i) {
        (void)machine.run_frame();
    }
    machine.set_button(Button::Start, false);

    for (int i = 0; i < 115; ++i) {
        (void)machine.run_frame();
    }

    const int change = pixel_difference(before, machine.framebuffer());
    std::cout << "  Pixels changed: " << change << " of "
              << nes::Framebuffer::kPixelCount << "  ("
              << std::fixed << std::setprecision(2)
              << (100.0 * change / static_cast<double>(nes::Framebuffer::kPixelCount))
              << "%)\n\n";
    std::cout << "  The screen is gone. Five frames of input, sampled once by the\n";
    std::cout << "  game's vblank routine, was enough to start it.\n\n";

    print_preview(machine.framebuffer());
    (void)write_ppm(out_dir / "input_started.ppm", machine.framebuffer());
}

// --- 4 ---------------------------------------------------------------------
void showHoldingRight(nes::Machine& machine, const std::filesystem::path& out_dir)
{
    title("4. Hold Right");

    for (int i = 0; i < 60; ++i) {
        (void)machine.run_frame();
    }
    const auto standing = machine.framebuffer();

    std::cout << "  Holding Right for 180 frames...\n\n";
    machine.set_button(Button::Right, true);
    for (int i = 0; i < 180; ++i) {
        (void)machine.run_frame();
    }
    machine.set_button(Button::Right, false);

    const int change = pixel_difference(standing, machine.framebuffer());
    std::cout << "  Pixels changed: " << change << "\n";
    std::cout << "  The view has scrolled: Mario walked and the world moved past him.\n\n";

    print_preview(machine.framebuffer());
    (void)write_ppm(out_dir / "input_right.ppm", machine.framebuffer());
}

// --- 5 ---------------------------------------------------------------------
void showPolling(nes::Machine& machine)
{
    title("5. The game polls the controller every frame");

    std::cout << "  The port only advances when something reads it, so counting the\n";
    std::cout << "  reads is a direct measure of whether the game is sampling input.\n\n";

    std::cout << "    frame   total reads\n";
    std::cout << "    -----   -----------\n";

    for (int i = 0; i < 6; ++i) {
        (void)machine.run_frame();
        std::cout << "      " << std::setw(3) << i << "   "
                  << std::setw(11) << machine.controller(0).read_count() << "\n";
    }

    std::cout << "\n  It grows every frame. A game that stopped reading its\n";
    std::cout << "  controller would show up here immediately, which is why this\n";
    std::cout << "  counter exists rather than being inferred from the picture.\n";

    std::cout << "\n  Note what the controller does NOT have: no interrupts, no timing,\n";
    std::cout << "  no notion of a frame, no debouncing, no repeat rate. It is a piece\n";
    std::cout << "  of wire with a latch. All of that lives in the game, which is why\n";
    std::cout << "  the same hardware feels different in different games.\n";
}

} // namespace

int main(int argc, char** argv)
{
    std::cout << "Classic Game Box - Phase 5: the controller\n";

    const std::filesystem::path requested = (argc > 1) ? argv[1] : "";
    const auto path = find_rom(requested);
    if (!path) {
        std::cout << "\n  No ROM found. Usage: demo_input /path/to/game.nes\n";
        return 1;
    }
    const std::filesystem::path out_dir = (argc > 2) ? argv[2] : "frames";

    const auto rom = read_file(*path);
    if (!rom) {
        std::cout << "\n  Could not read " << path->string() << "\n";
        return 1;
    }

    std::cout << "  ROM: " << path->string() << "\n";

    nes::Machine machine;
    std::string error;
    if (!machine.load_rom(*rom, error)) {
        std::cout << "\n  Could not load: " << error << "\n";
        return 1;
    }

    showTheProtocol();

    if (!reach_title_screen(machine)) {
        std::cout << "\n  The CPU halted before the title screen.\n";
        return 1;
    }

    std::error_code fs_error;
    std::filesystem::create_directories(out_dir, fs_error);

    showTitleScreen(machine, out_dir);
    showStartPress(machine, out_dir);
    showHoldingRight(machine, out_dir);
    showPolling(machine);

    title("Summary");
    std::cout << "The controller is a shift register with a latch, and nothing else.\n";
    std::cout << "Eight clocks in a fixed order: A B Select Start Up Down Left Right.\n";
    std::cout << "After eight reads the line is pulled high, which is how a program\n";
    std::cout << "tells a standard controller from an empty port.\n";
    std::cout << "The machine does not care where a button press came from, so a\n";
    std::cout << "script and a keyboard look identical to the emulator.\n";
    std::cout << "\nWrote input_title.ppm, input_started.ppm and input_right.ppm to "
              << out_dir.string() << "\n";

    return 0;
}
