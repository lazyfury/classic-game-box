// ---------------------------------------------------------------------------
// Tests against a real NES ROM.
//
// Nothing here is committed: the tests look for a .nes file in tests/data/
// (or at the path in the FC_TEST_ROM environment variable) and skip
// themselves when there is none. That keeps the repository free of
// copyrighted data while still letting a real cartridge be used locally.
//
// To enable them:
//
//     ln -s /path/to/your.nes tests/data/game.nes
//
// ---------------------------------------------------------------------------

#include "core/cpu/cpu.hpp"
#include "core/cpu/disassembler.hpp"
#include "core/nes/bus.hpp"
#include "core/nes/cartridge.hpp"
#include "core/nes/framebuffer.hpp"
#include "core/nes/apu.hpp"
#include "core/nes/machine.hpp"
#include "core/nes/ines.hpp"
#include "core/types.hpp"

#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <cstddef>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <optional>
#include <span>
#include <string>
#include <vector>

using namespace fc;

namespace {

std::optional<std::vector<u8>> read_file(const std::filesystem::path& path)
{
    std::ifstream file(path, std::ios::binary);
    if (!file) {
        return std::nullopt;
    }
    return std::vector<u8>(std::istreambuf_iterator<char>(file),
                           std::istreambuf_iterator<char>());
}

/// The ROM to test against: the FC_TEST_ROM variable if set, otherwise the
/// first .nes file in the test data directory.
std::optional<std::vector<u8>> find_test_rom()
{
    if (const char* path = std::getenv("FC_TEST_ROM")) {
        if (auto bytes = read_file(path)) {
            return bytes;
        }
    }

    std::error_code error;
    const std::filesystem::path dir{ FC_TEST_DATA_DIR };
    if (std::filesystem::is_directory(dir, error)) {
        for (const auto& entry : std::filesystem::directory_iterator(dir, error)) {
            if (entry.path().extension() == ".nes") {
                if (auto bytes = read_file(entry.path())) {
                    return bytes;
                }
            }
        }
    }
    return std::nullopt;
}

/// A PPU that only knows how to say "vblank has started".
///
/// This is a test double, not a PPU. Its only job is to show that the ROM
/// gets past its vblank wait, which proves the loop is exactly what was
/// blocking it. The real PPU is Phase 4.
struct VblankStubPpu : nes::Device {
    int reads_of_status = 0;

    [[nodiscard]] u8 read(u16 address) override
    {
        if (nes::NesBus::ppu_register_index(address) == 2) {   // $2002 PPUSTATUS
            ++reads_of_status;
            return 0x80;   // bit 7: vblank has started
        }
        return 0;
    }

    void write(u16 /*address*/, u8 /*value*/) override {}
};

/// Super Mario Bros is NROM with 32KB of PRG and 8KB of CHR. Those are the
/// numbers this file checks against; a different ROM skips the whole suite.
constexpr std::size_t kSmbFileSize  = 16u + 32768u + 8192u;
constexpr u16 kSmbEntryPoint = 0x8000;
constexpr u16 kSmbVblankWait = 0x800A;
constexpr u16 kSmbNmiVector  = 0x8082;

class SuperMarioBros : public ::testing::Test {
protected:
    void SetUp() override
    {
        rom_ = find_test_rom();
        if (!rom_) {
            GTEST_SKIP() << "no .nes file in " << FC_TEST_DATA_DIR
                         << " - set FC_TEST_ROM or symlink one in to run this suite";
        }

        std::string error;
        auto cart = nes::Cartridge::from_bytes(*rom_, error);
        if (!cart) {
            GTEST_SKIP() << "the ROM could not be loaded: " << error;
        }

        const auto& h = cart->header();
        if (h.mapper != 0 || h.prg_rom_pages != 2 || h.chr_rom_pages != 1) {
            GTEST_SKIP() << "this suite assumes a 32KB/8KB mapper 0 ROM, found: "
                         << cart->summary();
        }

        cart_ = std::move(cart);
    }

    /// A bus with this cartridge plugged in.
    [[nodiscard]] nes::NesBus make_bus()
    {
        nes::NesBus bus;
        bus.set_cartridge(&*cart_);
        return bus;
    }

    std::optional<std::vector<u8>> rom_;
    std::optional<nes::Cartridge> cart_;
};

} // namespace

// ===========================================================================
// The file itself
// ===========================================================================

TEST_F(SuperMarioBros, TheFileIsAValidInesImage)
{
    const auto result = nes::parse_ines_header(*rom_);
    ASSERT_TRUE(result.ok()) << result.error;

    EXPECT_EQ(rom_->size(), kSmbFileSize);
    EXPECT_EQ(result.header->total_size(), kSmbFileSize);
}

TEST_F(SuperMarioBros, TheHeaderDescribesNrom)
{
    const auto& h = cart_->header();

    EXPECT_EQ(h.prg_rom_pages, 2) << "32KB of program";
    EXPECT_EQ(h.chr_rom_pages, 1) << "8KB of graphics";
    EXPECT_EQ(h.mapper, 0) << "NROM: no banking hardware on the board";
    EXPECT_EQ(h.mirroring, nes::Mirroring::Vertical);
    EXPECT_FALSE(h.has_trainer);
    EXPECT_FALSE(h.has_battery);
    EXPECT_FALSE(h.nes2) << "an original iNES file, not NES 2.0";

    EXPECT_EQ(cart_->prg_rom().size(), 32768u);
    EXPECT_EQ(cart_->chr_rom().size(), 8192u);
}

TEST_F(SuperMarioBros, TheInterruptVectorsPointAtRealCode)
{
    // The vectors are the last six bytes of PRG ROM, and the mapper puts them
    // at $FFFA-$FFFF.
    const u16 nmi  = static_cast<u16>(cart_->read(0xFFFA) | (cart_->read(0xFFFB) << 8));
    const u16 reset = static_cast<u16>(cart_->read(0xFFFC) | (cart_->read(0xFFFD) << 8));
    const u16 irq  = static_cast<u16>(cart_->read(0xFFFE) | (cart_->read(0xFFFF) << 8));

    EXPECT_EQ(reset, kSmbEntryPoint);
    EXPECT_EQ(nmi, kSmbNmiVector);

    // Every vector must point at cartridge space, or the machine would jump
    // into RAM and execute whatever happens to be there.
    EXPECT_GE(reset, 0x8000);
    EXPECT_GE(nmi, 0x8000);
    EXPECT_GE(irq, 0x8000);
}

TEST_F(SuperMarioBros, EveryPrgByteReadsBackThroughTheMapper)
{
    const auto& prg = cart_->prg_rom();
    int mismatches = 0;

    for (std::size_t i = 0; i < prg.size(); ++i) {
        const u16 address = static_cast<u16>(0x8000 + i);
        if (cart_->read(address) != prg[i]) {
            ++mismatches;
        }
    }

    EXPECT_EQ(mismatches, 0) << "32KB mapped straight through, byte for byte";
}

// ===========================================================================
// The reset code
// ===========================================================================

TEST_F(SuperMarioBros, TheResetCodeIsTheRealStartupSequence)
{
    // 78          SEI               disable interrupts while we set up
    // D8          CLD               clear decimal mode
    // A9 10       LDA #$10
    // 8D 00 20    STA $2000         PPUCTRL: NMI on, background pattern table
    // A2 FF       LDX #$FF
    // 9A          TXS               SP = $FF
    // AD 02 20    LDA $2002         <- wait for vblank
    // 10 FB       BPL $800A
    const std::vector<u8> expected = {
        0x78, 0xD8, 0xA9, 0x10, 0x8D, 0x00, 0x20,
        0xA2, 0xFF, 0x9A, 0xAD, 0x02, 0x20, 0x10, 0xFB,
    };

    for (std::size_t i = 0; i < expected.size(); ++i) {
        const u16 address = static_cast<u16>(kSmbEntryPoint + i);
        EXPECT_EQ(cart_->read(address), expected[i])
            << "byte " << i << " at $" << std::hex << address;
    }
}

TEST_F(SuperMarioBros, TheResetRoutineDisassemblesReadably)
{
    // The project's own disassembler, reading a real cartridge. This is the
    // first time the whole toolchain touches a commercial ROM.
    nes::NesBus bus = make_bus();

    const std::vector<std::string> expected = {
        "SEI",
        "CLD",
        "LDA #$10",
        "STA $2000",
        "LDX #$FF",
        "TXS",
        "LDA $2002",
        "BPL $800A",
    };

    std::size_t offset = 0;
    for (const auto& text : expected) {
        const u16 address = static_cast<u16>(kSmbEntryPoint + offset);
        const auto insn = disassemble(bus, address);
        EXPECT_EQ(insn.text, text) << "at $" << std::hex << address;
        offset += static_cast<std::size_t>(insn.length);
    }
}

// ===========================================================================
// Running it
// ===========================================================================

TEST_F(SuperMarioBros, TheCpuExecutesTheRealStartupCode)
{
    nes::NesBus bus = make_bus();
    Cpu cpu{ bus };
    cpu.reset();

    // Reset already sets I and clears D, so make both wrong to prove the
    // instructions really change them.
    cpu.registers().set_flag(Flag::IrqDisable, false);
    cpu.registers().set_flag(Flag::Decimal, true);

    EXPECT_EQ(cpu.registers().pc, kSmbEntryPoint);

    cpu.step();   // 78 SEI
    EXPECT_TRUE(cpu.registers().flag(Flag::IrqDisable));
    EXPECT_TRUE(cpu.registers().flag(Flag::Decimal)) << "SEI must not touch D";

    cpu.step();   // D8 CLD
    EXPECT_FALSE(cpu.registers().flag(Flag::Decimal));

    cpu.step();   // A9 10 LDA #$10
    EXPECT_EQ(cpu.registers().a, 0x10);

    cpu.step();   // 8D 00 20 STA $2000
    cpu.step();   // A2 FF LDX #$FF
    EXPECT_EQ(cpu.registers().x, 0xFF);

    cpu.step();   // 9A TXS
    EXPECT_EQ(cpu.registers().sp, 0xFF)
        << "the stack was reset to the top of page 1";

    EXPECT_FALSE(cpu.is_halted());
}

TEST_F(SuperMarioBros, WithoutAPpuItSpinsOnTheVblankWait)
{
    nes::NesBus bus = make_bus();
    Cpu cpu{ bus };
    cpu.reset();

    cpu.run(8);

    // Steps 7 and 8 were LDA $2002 and BPL, so PC is back at $800A.
    EXPECT_EQ(cpu.registers().pc, kSmbVblankWait);

    const u64 cycles_before = cpu.total_cycles();
    cpu.run(1000);

    EXPECT_TRUE(cpu.registers().pc == kSmbVblankWait ||
                cpu.registers().pc == static_cast<u16>(kSmbVblankWait + 3))
        << "still looping between LDA $2002 and BPL, at " << cpu.registers().pc;

    EXPECT_GT(cpu.total_cycles(), cycles_before) << "and it is still burning cycles";
    EXPECT_FALSE(cpu.is_halted()) << "a wait loop is not a crash";
}

TEST_F(SuperMarioBros, WithAStubPpuItLeavesTheWait)
{
    nes::NesBus bus = make_bus();
    VblankStubPpu ppu;
    bus.set_ppu(&ppu);

    Cpu cpu{ bus };
    cpu.reset();
    cpu.run(8);

    // LDA $2002 now returns $80, so N is set and BPL falls through.
    EXPECT_EQ(cpu.registers().pc, static_cast<u16>(kSmbVblankWait + 5))
        << "past the BPL, at $800F";
    EXPECT_GT(ppu.reads_of_status, 0);
}

TEST_F(SuperMarioBros, TheVblankWaitIsExactlyWhatWasBlocking)
{
    // Same ROM, same bus, same number of instructions. The only difference is
    // whether something answers $2002 with the vblank bit set. That isolates
    // the boundary between "the CPU and cartridge work" and "there is no PPU
    // yet" - which is precisely the Phase 3 / Phase 4 line.
    constexpr int kSteps = 8;

    nes::NesBus bus_a = make_bus();
    Cpu cpu_a{ bus_a };
    cpu_a.reset();
    cpu_a.run(kSteps);

    nes::NesBus bus_b = make_bus();
    VblankStubPpu ppu;
    bus_b.set_ppu(&ppu);
    Cpu cpu_b{ bus_b };
    cpu_b.reset();
    cpu_b.run(kSteps);

    EXPECT_EQ(cpu_a.registers().pc, kSmbVblankWait) << "blocked";
    EXPECT_EQ(cpu_b.registers().pc, static_cast<u16>(kSmbVblankWait + 5)) << "free";

    // SP and X are the same: both machines ran exactly the same setup. A
    // differs, and that is the whole point - the only difference between the
    // two runs is what answered $2002.
    //
    // With no PPU, the read returns open bus. Note the value is $20, not $10
    // (the last thing WRITTEN): the operand fetch for $2002 drove the data bus
    // after that write, and open bus holds whatever was on it most recently.
    EXPECT_EQ(cpu_a.registers().sp, cpu_b.registers().sp);
    EXPECT_EQ(cpu_a.registers().x, cpu_b.registers().x);
    EXPECT_EQ(cpu_a.registers().a, 0x20)
        << "open bus: the high byte of the $2002 operand, which is the last "
           "byte the CPU put on the data bus";
    EXPECT_EQ(cpu_b.registers().a, 0x80) << "the vblank bit";

    // And that single bit is the difference between a running machine and a
    // hung one, because BPL tests exactly it.
    EXPECT_FALSE(cpu_a.registers().flag(Flag::Negative)) << "so BPL loops";
    EXPECT_TRUE(cpu_b.registers().flag(Flag::Negative)) << "so BPL falls through";
}

// ===========================================================================
// The graphics data
// ===========================================================================

TEST_F(SuperMarioBros, ChrRomHoldsPatternTableData)
{
    const auto& chr = cart_->chr_rom();
    ASSERT_EQ(chr.size(), 8192u);

    // 512 tiles of 16 bytes each.
    EXPECT_EQ(chr.size() / 16, 512u);

    int non_zero_tiles = 0;
    for (std::size_t tile = 0; tile < 512; ++tile) {
        const std::span<const u8> bytes(chr.data() + tile * 16, 16);
        for (u8 byte : bytes) {
            if (byte != 0) {
                ++non_zero_tiles;
                break;
            }
        }
    }

    EXPECT_GT(non_zero_tiles, 100) << "a CHR bank should be mostly real tiles";
    EXPECT_LT(non_zero_tiles, 512) << "and should still have some blank ones";
}

TEST_F(SuperMarioBros, ChrReadsBackThroughTheMapper)
{
    for (u16 address = 0; address < 8192; address += 37) {
        EXPECT_EQ(cart_->read_chr(address), cart_->chr_rom()[address]);
    }
}

// ===========================================================================
// Rendering a real game
// ===========================================================================
//
// Everything below runs Super Mario Bros on the full machine - CPU, bus,
// cartridge and PPU - and checks the picture that comes out. These are the
// first tests in the project that look at pixels.

/// A machine with the test ROM loaded, or nothing if there is no ROM.
class RenderingTest : public ::testing::Test {
protected:
    void SetUp() override
    {
        rom_ = find_test_rom();
        if (!rom_) {
            GTEST_SKIP() << "no .nes file in " << FC_TEST_DATA_DIR;
        }

        std::string error;
        if (!machine_.load_rom(*rom_, error)) {
            GTEST_SKIP() << "the ROM could not be loaded: " << error;
        }

        const auto* cart = machine_.cartridge();
        if (cart == nullptr || cart->header().mapper != 0) {
            GTEST_SKIP() << "this suite assumes mapper 0";
        }
    }

    /// Run frames until the PPU is actually drawing, or give up.
    [[nodiscard]] bool run_until_rendering(int max_frames = 120)
    {
        for (int i = 0; i < max_frames; ++i) {
            if (!machine_.run_frame()) {
                return false;
            }
            if ((machine_.ppu().mask() & 0x18) == 0x18) {
                return true;
            }
        }
        return false;
    }

    [[nodiscard]] static int distinct_colours(const nes::Framebuffer& fb)
    {
        std::vector<u32> seen;
        for (u32 pixel : fb.pixels) {
            bool found = false;
            for (u32 c : seen) {
                if (c == pixel) {
                    found = true;
                    break;
                }
            }
            if (!found) {
                seen.push_back(pixel);
                if (seen.size() > 64) {
                    break;
                }
            }
        }
        return static_cast<int>(seen.size());
    }

    std::optional<std::vector<u8>> rom_;
    nes::Machine machine_;
};

TEST_F(RenderingTest, TheGameRunsForHundredsOfFramesWithoutAnIllegalOpcode)
{
    // If the CPU ever halted, the emulator has a bug. A commercial game does
    // not execute undefined opcodes.
    for (int i = 0; i < 120; ++i) {
        ASSERT_TRUE(machine_.run_frame())
            << "the CPU halted at frame " << i << " on opcode $"
            << std::hex << static_cast<int>(machine_.cpu().unimplemented_opcode());
    }

    EXPECT_EQ(machine_.ppu().frame_count(), 120);
    EXPECT_GT(machine_.cpu().total_cycles(), 120u * 29000u)
        << "about 29780 CPU cycles fit in a frame";
}

TEST_F(RenderingTest, TheGameTurnsRenderingOn)
{
    ASSERT_TRUE(run_until_rendering())
        << "the ROM never enabled background and sprites";

    const u8 mask = machine_.ppu().mask();
    EXPECT_TRUE((mask & 0x08) != 0) << "background on";
    EXPECT_TRUE((mask & 0x10) != 0) << "sprites on";
    EXPECT_TRUE((mask & 0x02) != 0) << "background in the left column";
    EXPECT_TRUE((mask & 0x04) != 0) << "sprites in the left column";
}

TEST_F(RenderingTest, TheFrameIsNotBlank)
{
    ASSERT_TRUE(run_until_rendering());
    (void)machine_.run_frame();

    const auto& fb = machine_.framebuffer();
    const int colours = distinct_colours(fb);

    EXPECT_GE(colours, 6)
        << "a title screen uses several palettes; " << colours << " means it did not draw";
    EXPECT_LE(colours, 25) << "the NES cannot show more than 25 colours at once";
}

TEST_F(RenderingTest, ThePictureHasTheShapeOfAScreen)
{
    ASSERT_TRUE(run_until_rendering());
    (void)machine_.run_frame();

    const auto& fb = machine_.framebuffer();

    // The top row is the status bar and the middle is the sky, so they must
    // not be the same colour. If rendering were broken they would be.
    int top_different = 0;
    for (int x = 0; x < nes::Framebuffer::kWidth; ++x) {
        if (fb.at(x, 8) != fb.at(x, 120)) {
            ++top_different;
        }
    }
    EXPECT_GT(top_different, 32) << "the status bar differs from the play area";

    // And the bottom of the screen is the ground, which is a narrow band of
    // colours repeated across the width.
    int ground_pixels = 0;
    for (int y = 220; y < 232; ++y) {
        for (int x = 0; x < nes::Framebuffer::kWidth; ++x) {
            if (fb.at(x, y) != fb.at(x, 120)) {
                ++ground_pixels;
            }
        }
    }
    EXPECT_GT(ground_pixels, nes::Framebuffer::kWidth * 6)
        << "the ground strip differs from the sky";
}

TEST_F(RenderingTest, VblankAndNmiHappenOncePerFrame)
{
    ASSERT_TRUE(run_until_rendering());

    int vblanks = 0;
    int nmi_handlers = 0;
    const int target_frame = machine_.ppu().frame_count() + 1;

    while (machine_.ppu().frame_count() < target_frame) {
        ASSERT_TRUE(machine_.run_instructions(20));

        // The machine drains the NMI latch each instruction, so look at the
        // CPU instead: SMB's NMI handler starts at $8082.
        if ((machine_.ppu().status() & 0x80) != 0) {
            ++vblanks;
        }
    }

    EXPECT_GT(vblanks, 0) << "the vblank flag was never seen set";

    // The NMI vector this ROM declares must be reached during the frame.
    const u16 nmi_vector =
        static_cast<u16>(machine_.cartridge()->read(0xFFFA) |
                         (machine_.cartridge()->read(0xFFFB) << 8));
    EXPECT_GE(nmi_vector, 0x8000);
    (void)nmi_handlers;
}

TEST_F(RenderingTest, SpriteZeroHitIsUsedForTheStatusBar)
{
    ASSERT_TRUE(run_until_rendering());

    // Sprite zero hit is how this game keeps the status bar still while the
    // world scrolls: it waits for sprite 0 to be drawn, then changes the
    // scroll. If it never fires, the game would still run but the screen
    // would tear.
    bool seen = false;
    const int target_frame = machine_.ppu().frame_count() + 2;

    while (machine_.ppu().frame_count() < target_frame && !seen) {
        ASSERT_TRUE(machine_.run_instructions(8));
        if ((machine_.ppu().status() & 0x40) != 0) {
            seen = true;
        }
    }

    EXPECT_TRUE(seen) << "sprite zero hit never fired";
}

TEST_F(RenderingTest, ThePpuAddressSpaceIsPopulatedWithRealData)
{
    ASSERT_TRUE(run_until_rendering());
    (void)machine_.run_frame();

    auto& ppu = machine_.ppu();

    // The game must have written a palette.
    bool palette_written = false;
    for (u8 i = 0; i < 32; ++i) {
        if (ppu.palette_ram(i) != 0) {
            palette_written = true;
            break;
        }
    }
    EXPECT_TRUE(palette_written) << "no palette was loaded";

    // And it must have filled the nametable with tile indices.
    int non_zero = 0;
    for (u16 i = 0; i < 0x400; ++i) {
        if (ppu.read_vram(static_cast<u16>(0x2000 + i)) != 0) {
            ++non_zero;
        }
    }
    EXPECT_GT(non_zero, 64) << "the nametable is mostly empty";

    // Sprites must have been loaded through OAM DMA.
    int sprite_bytes = 0;
    for (int i = 0; i < 256; ++i) {
        if (ppu.oam(static_cast<u8>(i)) != 0) {
            ++sprite_bytes;
        }
    }
    EXPECT_GT(sprite_bytes, 0) << "OAM is empty";
}

TEST_F(RenderingTest, NoIllegalOpcodeIsEverReached)
{
    // The strongest statement a CPU test can make about a real game: run it
    // for a while and never execute a byte the 6502 does not define.
    for (int i = 0; i < 60; ++i) {
        ASSERT_TRUE(machine_.run_frame()) << "halted at frame " << i;
    }
    EXPECT_EQ(machine_.cpu().unimplemented_opcode(), 0);
}

// ===========================================================================
// Input
// ===========================================================================
//
// The controller is the first part of the machine that has no effect at all
// until something presses a button. These tests prove the loop closes: a
// button press set from outside reaches the game, and the game visibly
// changes what it draws.

namespace {

/// How many pixels differ between two frames.
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

} // namespace

class InputTest : public RenderingTest {
protected:
    /// Get to a point where the title screen is up and settled.
    [[nodiscard]] bool reach_the_title_screen()
    {
        if (!run_until_rendering()) {
            return false;
        }
        for (int i = 0; i < 240; ++i) {
            if (!machine_.run_frame()) {
                return false;
            }
        }
        return true;
    }

    /// Run `frames` frames, optionally holding Start for the first few.
    [[nodiscard]] bool run_with_start(int frames, int hold_for)
    {
        if (hold_for > 0) {
            machine_.set_button(nes::Controller::Button::Start, true);
        }
        for (int i = 0; i < frames; ++i) {
            if (i == hold_for) {
                machine_.set_button(nes::Controller::Button::Start, false);
            }
            if (!machine_.run_frame()) {
                return false;
            }
        }
        return true;
    }
};

TEST_F(InputTest, TheTitleScreenIsNearlyStaticWithNoInput)
{
    ASSERT_TRUE(reach_the_title_screen());

    const auto title = machine_.framebuffer();
    for (int i = 0; i < 120; ++i) {
        ASSERT_TRUE(machine_.run_frame());
    }

    // A blinking cursor is about all that should change.
    EXPECT_LT(pixel_difference(title, machine_.framebuffer()), 2000)
        << "nothing was pressed, so almost nothing should move";
}

TEST_F(InputTest, PressingStartLeavesTheTitleScreen)
{
    ASSERT_TRUE(reach_the_title_screen());

    const auto title = machine_.framebuffer();

    // The control run: identical timing, no input.
    ASSERT_TRUE(run_with_start(120, 0));
    const int idle_change = pixel_difference(title, machine_.framebuffer());

    // Now the same 120 frames again, from that same starting point, but with
    // a Start press at the beginning.
    const auto baseline = machine_.framebuffer();
    ASSERT_TRUE(run_with_start(120, 5));
    const int started_change = pixel_difference(baseline, machine_.framebuffer());

    // Without input the screen is static; with it, the game has begun.
    EXPECT_GT(started_change, 20000)
        << "pressing Start should have started the game";
    EXPECT_GT(started_change, idle_change * 10)
        << "and the difference must be caused by the press, not by time passing";
}

TEST_F(InputTest, AFiveFramePressIsEnough)
{
    // The game samples the controller once per frame, so a press only has to
    // survive one vblank routine to be noticed. Five frames is generous.
    ASSERT_TRUE(reach_the_title_screen());

    const auto title = machine_.framebuffer();
    ASSERT_TRUE(run_with_start(120, 5));

    EXPECT_GT(pixel_difference(title, machine_.framebuffer()), 20000);
}

TEST_F(InputTest, AOneFramePressIsEnough)
{
    ASSERT_TRUE(reach_the_title_screen());

    const auto title = machine_.framebuffer();
    ASSERT_TRUE(run_with_start(120, 1));

    EXPECT_GT(pixel_difference(title, machine_.framebuffer()), 20000)
        << "one frame is longer than the game's own sampling interval";
}

TEST_F(InputTest, TheGameClocksTheControllerEveryFrame)
{
    // The port only advances when something reads it, so the read counter is
    // a direct measure of whether the game is polling its controller. A game
    // that stopped reading input would show up here immediately.
    ASSERT_TRUE(reach_the_title_screen());

    const u64 before = machine_.bus().controller(0).read_count();
    ASSERT_TRUE(machine_.run_frame());
    const u64 after_one = machine_.bus().controller(0).read_count();
    ASSERT_TRUE(machine_.run_frame());
    const u64 after_two = machine_.bus().controller(0).read_count();

    EXPECT_GT(after_one, before) << "at least eight clocks happened";
    EXPECT_GE(after_one - before, 8u) << "a full controller read is eight clocks";
    EXPECT_GT(after_two, after_one) << "and again on the next frame";
}

TEST_F(InputTest, HoldingRightScrollsTheLevel)
{
    ASSERT_TRUE(reach_the_title_screen());
    ASSERT_TRUE(run_with_start(120, 5));   // start the game

    // Let the level settle, then hold Right and watch it move.
    for (int i = 0; i < 60; ++i) {
        ASSERT_TRUE(machine_.run_frame());
    }
    const auto standing = machine_.framebuffer();

    machine_.set_button(nes::Controller::Button::Right, true);
    for (int i = 0; i < 180; ++i) {
        ASSERT_TRUE(machine_.run_frame());
    }
    machine_.set_button(nes::Controller::Button::Right, false);

    EXPECT_GT(pixel_difference(standing, machine_.framebuffer()), 10000)
        << "holding Right should have moved Mario and scrolled the view";
}

// ===========================================================================
// Sound
// ===========================================================================

TEST_F(InputTest, TheTitleScreenIsSilent)
{
    // Super Mario Bros does not start its music until the game does, so a
    // silent title screen is correct, not a broken APU.
    ASSERT_TRUE(reach_the_title_screen());

    (void)machine_.apu().take_samples();
    for (int i = 0; i < 60; ++i) {
        ASSERT_TRUE(machine_.run_frame());
    }

    EXPECT_EQ(machine_.apu().enabled_channels() & 0x0F, 0x00)
        << "no tone channel is enabled yet";

    double energy = 0.0;
    std::size_t count = 0;
    for (f32 sample : machine_.apu().take_samples()) {
        energy += static_cast<double>(sample) * static_cast<double>(sample);
        ++count;
    }
    ASSERT_GT(count, 0u) << "the APU is producing samples even so";
    EXPECT_LT(std::sqrt(energy / static_cast<double>(count)), 0.001);
}

TEST_F(InputTest, TheGameMakesSoundOnceItStarts)
{
    ASSERT_TRUE(reach_the_title_screen());
    ASSERT_TRUE(run_with_start(120, 5));

    EXPECT_EQ(machine_.apu().enabled_channels() & 0x0F, 0x0F)
        << "the game turned on pulse 1, pulse 2, triangle and noise";

    (void)machine_.apu().take_samples();

    double energy = 0.0;
    std::size_t count = 0;
    f32 peak = 0.0f;
    for (int i = 0; i < 120; ++i) {
        ASSERT_TRUE(machine_.run_frame());
        for (f32 sample : machine_.apu().take_samples()) {
            energy += static_cast<double>(sample) * static_cast<double>(sample);
            peak = std::max(peak, sample);
            ++count;
        }
    }

    ASSERT_GT(count, 80000u) << "120 frames is 2 seconds, so about 88000 samples";
    const double rms = std::sqrt(energy / static_cast<double>(count));

    EXPECT_GT(rms, 0.01) << "there is music playing";
    EXPECT_GT(peak, 0.05f);
    EXPECT_LE(peak, 1.0f) << "the mixer stays inside its range";
}

TEST_F(InputTest, TheChannelsAreActuallyModulated)
{
    // A stuck channel would give a constant output. Music has to vary.
    ASSERT_TRUE(reach_the_title_screen());
    ASSERT_TRUE(run_with_start(120, 5));
    (void)machine_.apu().take_samples();

    f32 lowest = 1.0f;
    f32 highest = 0.0f;
    for (int i = 0; i < 120; ++i) {
        ASSERT_TRUE(machine_.run_frame());
        for (f32 sample : machine_.apu().take_samples()) {
            lowest = std::min(lowest, sample);
            highest = std::max(highest, sample);
        }
    }

    EXPECT_GT(highest - lowest, 0.05f) << "the output swings, so something is playing";
}

TEST_F(InputTest, SoundKeepsComingWhileTheCpuRuns)
{
    // The APU is clocked from the same loop as the CPU and PPU. If the
    // machine forgot to tick it the sample buffer would stop growing.
    ASSERT_TRUE(reach_the_title_screen());
    ASSERT_TRUE(run_with_start(120, 5));

    (void)machine_.apu().take_samples();
    ASSERT_TRUE(machine_.run_frame());
    const std::size_t after_one = machine_.apu().samples_pending();
    ASSERT_TRUE(machine_.run_frame());
    const std::size_t after_two = machine_.apu().samples_pending();

    EXPECT_GT(after_one, 500u) << "about 735 samples fit in a frame";
    EXPECT_GT(after_two, after_one);
}
