#include "core/nes/ines.hpp"
#include "core/nes/machine.hpp"
#include "core/nes/ppu.hpp"
#include "core/nes/ram_cartridge.hpp"
#include "core/types.hpp"

#include <gtest/gtest.h>

#include <cstddef>
#include <span>
#include <string>
#include <vector>

using namespace fc;

namespace {

/// A synthetic cartridge, so PPU tests do not need a real ROM.
std::vector<u8> make_ines(u8 prg_pages, u8 chr_pages, u8 flags6 = 0)
{
    std::vector<u8> rom(16, 0);
    rom[0] = 'N';
    rom[1] = 'E';
    rom[2] = 'S';
    rom[3] = 0x1A;
    rom[4] = prg_pages;
    rom[5] = chr_pages;
    rom[6] = flags6;
    rom.resize(16 + std::size_t(prg_pages) * 16384u + std::size_t(chr_pages) * 8192u, 0);
    return rom;
}

/// A cartridge with a CHR ROM whose bytes are predictable, so a rendering
/// test can work out what a tile should look like.
std::vector<u8> make_ines_with_chr(std::span<const u8> chr)
{
    std::vector<u8> rom = make_ines(1, 0);
    rom[5] = 1;
    rom.resize(16 + 16384 + 8192, 0);
    for (std::size_t i = 0; i < chr.size() && i < 8192; ++i) {
        rom[16 + 16384 + i] = chr[i];
    }
    return rom;
}

/// A PPU with a cartridge attached and nothing else.
struct Fixture {
    nes::RamCartridge ram_cart;
    nes::NesBus bus;
    nes::Ppu ppu;

    Fixture()
    {
        bus.set_ppu(&ppu);
        bus.set_oam_target(&ppu);
        bus.set_cartridge(&ram_cart);
        ppu.reset();
    }

    void set_cartridge(std::vector<u8> rom)
    {
        std::string error;
        auto cart = nes::Cartridge::from_bytes(rom, error);
        EXPECT_TRUE(cart.has_value()) << error;
        if (cart) {
            cartridge_ = std::make_unique<nes::Cartridge>(std::move(*cart));
            ppu.set_cartridge(cartridge_.get());
        }
    }

    /// Write through the CPU-facing registers the way a program does.
    void set_vram_address(u16 address)
    {
        ppu.write(0x2006, static_cast<u8>(address >> 8));
        ppu.write(0x2006, static_cast<u8>(address & 0xFF));
    }

    void write_vram(u16 address, u8 value)
    {
        set_vram_address(address);
        ppu.write(0x2007, value);
    }

    [[nodiscard]] u8 read_vram(u16 address)
    {
        set_vram_address(address);
        (void)ppu.read(0x2007);        // discard the buffered byte
        return ppu.read(0x2007);
    }

private:
    std::unique_ptr<nes::Cartridge> cartridge_;
};

/// Render scanlines 0..239 and stop before the pre-render line clears the
/// status flags, so a test can still see sprite zero hit afterwards.
void render_visible_frame(nes::Ppu& ppu)
{
    int guard = 0;
    while (ppu.scanline() != 240 && guard < 400) {
        ppu.tick(nes::Ppu::kDotsPerScanline);
        ++guard;
    }
}

/// Advance the PPU until it is at the start of the given scanline.
void run_to_scanline(nes::Ppu& ppu, int target)
{
    const int start = ppu.scanline();
    int guard = 0;
    while (ppu.scanline() != target && guard < 400) {
        ppu.tick(nes::Ppu::kDotsPerScanline);
        ++guard;
    }
    (void)start;
}

} // namespace

// ===========================================================================
// The eight registers
// ===========================================================================

TEST(Ppu, ControlRegisterSelectsTheNametableInTheTempAddress)
{
    Fixture f;

    f.ppu.write(0x2000, 0x02);   // nametable 2

    EXPECT_EQ(f.ppu.control(), 0x02);
    EXPECT_EQ((f.ppu.temp_address() >> 10) & 0x03, 0x02);
}

TEST(Ppu, MaskRegisterIsReadableThroughTheAccessor)
{
    Fixture f;
    f.ppu.write(0x2001, 0x1E);

    EXPECT_EQ(f.ppu.mask(), 0x1E);
    EXPECT_TRUE(f.ppu.rendering_enabled());
    EXPECT_TRUE((f.ppu.mask() & 0x08) != 0) << "background on";
    EXPECT_TRUE((f.ppu.mask() & 0x10) != 0) << "sprites on";

    f.ppu.write(0x2001, 0x00);
    EXPECT_FALSE(f.ppu.rendering_enabled());
}

TEST(Ppu, ReadingStatusClearsVblankAndTheWriteToggle)
{
    Fixture f;

    // Start the write toggle with one $2006 write.
    f.ppu.write(0x2006, 0x20);
    f.ppu.write(0x2000, 0x80);   // enable NMI

    run_to_scanline(f.ppu, 241);
    EXPECT_TRUE((f.ppu.status() & 0x80) != 0) << "vblank began";

    const u8 status = f.ppu.read(0x2002);
    EXPECT_TRUE((status & 0x80) != 0) << "the read still reports vblank";
    EXPECT_FALSE((f.ppu.status() & 0x80) != 0) << "but it is cleared afterwards";

    // The toggle was reset, so the next $2006 write is the high byte again.
    f.ppu.write(0x2006, 0x24);
    f.ppu.write(0x2006, 0x00);
    EXPECT_EQ(f.ppu.vram_address(), 0x2400);
}

TEST(Ppu, StatusLowBitsComeFromTheOpenBusLatch)
{
    Fixture f;

    // The PPU's low five status bits are not a register at all: they are the
    // last byte that was on the PPU's register bus.
    f.write_vram(0x2000, 0xA5);

    const u8 status = f.ppu.read(0x2002);
    EXPECT_EQ(status & 0x1F, 0x05) << "the low bits of the last value written";
}

// ===========================================================================
// The PPU's address space
// ===========================================================================

TEST(Ppu, AddressRegisterAdvancesByOneOrThirtyTwo)
{
    Fixture f;

    f.ppu.write(0x2000, 0x00);   // increment by 1
    f.set_vram_address(0x2000);
    f.ppu.write(0x2007, 0x11);
    EXPECT_EQ(f.ppu.vram_address(), 0x2001);

    f.ppu.write(0x2000, 0x04);   // increment by 32
    f.set_vram_address(0x2000);
    f.ppu.write(0x2007, 0x22);
    EXPECT_EQ(f.ppu.vram_address(), 0x2020);
}

TEST(Ppu, VramReadsAreBuffered)
{
    Fixture f;

    f.write_vram(0x2000, 0x77);
    f.write_vram(0x2001, 0x88);

    f.set_vram_address(0x2000);

    const u8 first = f.ppu.read(0x2007);
    EXPECT_NE(first, 0x77) << "the first read returns the stale buffer";

    const u8 second = f.ppu.read(0x2007);
    EXPECT_EQ(second, 0x77) << "the second returns the byte that was fetched";

    const u8 third = f.ppu.read(0x2007);
    EXPECT_EQ(third, 0x88);
}

TEST(Ppu, PaletteReadsAreImmediateAndNotBuffered)
{
    Fixture f;

    f.write_vram(0x3F00, 0x21);
    f.set_vram_address(0x3F00);

    // Palette RAM is small enough that the hardware did not buffer it.
    EXPECT_EQ(f.ppu.read(0x2007), 0x21) << "available on the first read";
}

TEST(Ppu, PaletteRamOnlyHoldsSixBits)
{
    Fixture f;
    f.write_vram(0x3F00, 0xFF);

    EXPECT_EQ(f.ppu.palette_ram(0x00), 0x3F);
}

TEST(Ppu, TheSpritePaletteMirrorsTheBackgroundPalette)
{
    Fixture f;

    // $3F10 is not a byte of its own: it IS $3F00. That is why every sprite
    // palette really has three usable colours, not four.
    f.write_vram(0x3F00, 0x15);
    EXPECT_EQ(f.ppu.read_vram(0x3F10), 0x15);

    f.write_vram(0x3F14, 0x2A);
    EXPECT_EQ(f.ppu.read_vram(0x3F04), 0x2A);

    EXPECT_EQ(f.ppu.palette_ram(0x10), f.ppu.palette_ram(0x00));
}

TEST(Ppu, ThePaletteMirrorsThroughTheRestOfTheAddressSpace)
{
    Fixture f;
    f.write_vram(0x3F05, 0x30);

    EXPECT_EQ(f.ppu.read_vram(0x3F25), 0x30);
    EXPECT_EQ(f.ppu.read_vram(0x3FE5), 0x30);
}

TEST(Ppu, NametablesMirrorVerticallyWhenTheCartridgeSaysSo)
{
    Fixture f;
    f.set_cartridge(make_ines(1, 1, 0x01));   // vertical mirroring

    f.write_vram(0x2000, 0xAA);   // nametable 0
    f.write_vram(0x2400, 0xBB);   // nametable 1

    // Vertical: $2000 and $2800 are the same, $2400 and $2C00 are the same.
    EXPECT_EQ(f.ppu.read_vram(0x2800), 0xAA);
    EXPECT_EQ(f.ppu.read_vram(0x2C00), 0xBB);

    // But 0 and 1 stay distinct.
    EXPECT_EQ(f.ppu.read_vram(0x2000), 0xAA);
    EXPECT_EQ(f.ppu.read_vram(0x2400), 0xBB);
}

TEST(Ppu, NametablesMirrorHorizontallyWhenTheCartridgeSaysSo)
{
    Fixture f;
    f.set_cartridge(make_ines(1, 1, 0x00));   // horizontal mirroring

    // Horizontal: $2000 and $2400 are the same, $2800 and $2C00 are the same.
    // Write one of each pair, then read through both mirrors.
    f.write_vram(0x2000, 0xAA);
    EXPECT_EQ(f.ppu.read_vram(0x2400), 0xAA);

    f.write_vram(0x2800, 0xCC);
    EXPECT_EQ(f.ppu.read_vram(0x2C00), 0xCC);
    EXPECT_NE(f.ppu.read_vram(0x2000), 0xCC) << "the other pair is separate";
}

TEST(Ppu, FourScreenMirroringKeepsAllFourDistinct)
{
    Fixture f;
    f.set_cartridge(make_ines(1, 1, 0x08));   // four screen

    f.write_vram(0x2000, 0x11);
    f.write_vram(0x2400, 0x22);
    f.write_vram(0x2800, 0x33);
    f.write_vram(0x2C00, 0x44);

    EXPECT_EQ(f.ppu.read_vram(0x2000), 0x11);
    EXPECT_EQ(f.ppu.read_vram(0x2400), 0x22);
    EXPECT_EQ(f.ppu.read_vram(0x2800), 0x33);
    EXPECT_EQ(f.ppu.read_vram(0x2C00), 0x44);
}

TEST(Ppu, PatternTablesGoToTheCartridge)
{
    std::vector<u8> chr(8192, 0);
    chr[0] = 0x3C;

    Fixture f;
    f.set_cartridge(make_ines_with_chr(chr));

    EXPECT_EQ(f.ppu.read_vram(0x0000), 0x3C) << "CHR ROM, via the cartridge";
    EXPECT_EQ(f.ppu.read_vram(0x1000), 0x00);
}

// ===========================================================================
// OAM
// ===========================================================================

TEST(Ppu, OamDataWritesAdvanceTheAddress)
{
    Fixture f;

    f.ppu.write(0x2003, 0x10);   // OAMADDR
    f.ppu.write(0x2004, 0xAA);   // OAMDATA
    f.ppu.write(0x2004, 0xBB);

    EXPECT_EQ(f.ppu.oam(0x10), 0xAA);
    EXPECT_EQ(f.ppu.oam(0x11), 0xBB);
    EXPECT_EQ(f.ppu.read(0x2004), f.ppu.oam(0x12)) << "OAMDATA reads the current byte";
}

TEST(Ppu, AnOamDmaFillsAllTwoHundredFiftySixBytes)
{
    Fixture f;

    for (u16 i = 0; i < 256; ++i) {
        f.bus.write(static_cast<u16>(0x0200 + i), static_cast<u8>(i ^ 0x5A));
    }

    f.bus.write(0x4014, 0x02);   // DMA from page $02

    for (int i = 0; i < 256; ++i) {
        EXPECT_EQ(f.ppu.oam(static_cast<u8>(i)), static_cast<u8>(i ^ 0x5A));
    }
}

// ===========================================================================
// Timing, vblank and NMI
// ===========================================================================

TEST(Ppu, AFrameIsTwoHundredSixtyTwoScanlines)
{
    Fixture f;
    f.ppu.write(0x2001, 0x00);   // rendering off: nothing to draw

    const int start_frame = f.ppu.frame_count();

    for (int i = 0; i < nes::Ppu::kScanlinesPerFrame * nes::Ppu::kDotsPerScanline; ++i) {
        f.ppu.tick();
    }

    EXPECT_EQ(f.ppu.frame_count(), start_frame + 1);
    EXPECT_EQ(f.ppu.scanline(), -1) << "back at the pre-render line";
}

TEST(Ppu, VblankSetsTheFlagAndRaisesNmiWhenEnabled)
{
    Fixture f;
    f.ppu.write(0x2000, 0x80);   // NMI on

    run_to_scanline(f.ppu, 241);

    EXPECT_TRUE((f.ppu.status() & 0x80) != 0);
    EXPECT_TRUE(f.ppu.consume_nmi());
    EXPECT_FALSE(f.ppu.consume_nmi()) << "an NMI is a one-shot event";
}

TEST(Ppu, NoNmiWhenTheControlBitIsClear)
{
    Fixture f;
    f.ppu.write(0x2000, 0x00);   // NMI off

    run_to_scanline(f.ppu, 241);

    EXPECT_TRUE((f.ppu.status() & 0x80) != 0) << "vblank still happens";
    EXPECT_FALSE(f.ppu.consume_nmi());
}

TEST(Ppu, ReadingStatusCancelsAPendingNmi)
{
    Fixture f;
    f.ppu.write(0x2000, 0x80);

    run_to_scanline(f.ppu, 241);
    (void)f.ppu.read(0x2002);    // software noticed on its own

    EXPECT_FALSE(f.ppu.consume_nmi()) << "the NMI was cancelled";
}

TEST(Ppu, TheFrameCounterStartsOverAtThePreRenderLine)
{
    Fixture f;
    f.ppu.write(0x2001, 0x00);

    run_to_scanline(f.ppu, 0);
    EXPECT_EQ(f.ppu.scanline(), 0);
}

// ===========================================================================
// Rendering the background
// ===========================================================================

TEST(Ppu, RendersATileFromANametable)
{
    // Tile 1 in CHR: two bit planes that together make a solid colour 1.
    //   plane 0 = 0xFF, plane 1 = 0x00  ->  every pixel is colour 1
    // Planar, not interleaved: bytes 0-7 are plane 0, bytes 8-15 are plane 1.
    std::vector<u8> chr(8192, 0);
    for (int row = 0; row < 8; ++row) {
        chr[1 * 16 + row] = 0xFF;        // plane 0
        chr[1 * 16 + 8 + row] = 0x00;    // plane 1
    }

    Fixture f;
    f.set_cartridge(make_ines_with_chr(chr));

    // Palette: $3F00 = colour $0F (black), palette 0 colour 1 = $21.
    f.write_vram(0x3F00, 0x0F);
    f.write_vram(0x3F01, 0x21);

    // Nametable 0, tile (0,0) = tile 1, everything else blank.
    f.write_vram(0x2000, 1);

    f.ppu.write(0x2001, 0x0A);   // background on, with the left column
    f.ppu.write(0x2005, 0);
    f.ppu.write(0x2005, 0);
    f.set_vram_address(0x0000);

    // Run the whole frame.
    render_visible_frame(f.ppu);

    const u32 expected = nes::Ppu::colour(0x21);
    EXPECT_EQ(f.ppu.framebuffer().at(0, 0), expected);
    EXPECT_EQ(f.ppu.framebuffer().at(7, 7), expected);

    // Tile 0 is blank, so the pixel after it falls back to $3F00.
    EXPECT_EQ(f.ppu.framebuffer().at(8, 0), nes::Ppu::colour(0x0F));
}

TEST(Ppu, BackgroundAttributesSelectThePalette)
{
    // Two tiles: both solid colour 1, but in different 2x2 attribute blocks.
    std::vector<u8> chr(8192, 0);
    for (int tile : { 1, 2 }) {
        for (int row = 0; row < 8; ++row) {
            chr[tile * 16 + row] = 0xFF;   // plane 0 only
        }
    }

    Fixture f;
    f.set_cartridge(make_ines_with_chr(chr));

    for (int i = 0; i < 4; ++i) {
        f.write_vram(static_cast<u16>(0x3F00 + i * 4), 0x0F);
        f.write_vram(static_cast<u16>(0x3F01 + i * 4), static_cast<u8>(0x20 + i));
    }

    // Tiles 0 and 4 on row 0. The attribute table block for (0,0) is $23C0.
    f.write_vram(0x2000, 1);
    f.write_vram(0x2004, 2);
    f.write_vram(0x23C0, 0x01);   // block (0,0) -> palette 1

    f.ppu.write(0x2001, 0x0A);
    f.ppu.write(0x2005, 0);
    f.ppu.write(0x2005, 0);
    f.set_vram_address(0x0000);

    render_visible_frame(f.ppu);

    EXPECT_EQ(f.ppu.framebuffer().at(0, 0), nes::Ppu::colour(0x21))
        << "tile at (0,0) uses background palette 1";
    EXPECT_EQ(f.ppu.framebuffer().at(32, 0), nes::Ppu::colour(0x20))
        << "tile 4 falls in the next attribute block, palette 0";
}

TEST(Ppu, RenderingOffLeavesTheBackgroundColour)
{
    Fixture f;
    f.write_vram(0x3F00, 0x30);
    f.ppu.write(0x2001, 0x00);   // everything off

    render_visible_frame(f.ppu);

    EXPECT_EQ(f.ppu.framebuffer().at(128, 120), nes::Ppu::colour(0x30));
}

// ===========================================================================
// Sprites
// ===========================================================================

TEST(Ppu, SpriteEvaluationSelectsSpritesInRange)
{
    Fixture f;

    // Sprite 0 covers scanline 20 (OAM Y is one above the first row).
    f.ppu.write_oam(0, 19);   // Y
    f.ppu.write_oam(1, 0x10); // tile
    f.ppu.write_oam(2, 0x00); // attributes
    f.ppu.write_oam(3, 0x40); // X

    // Sprite 1 is far away.
    f.ppu.write_oam(4, 100);
    f.ppu.write_oam(5, 0x11);
    f.ppu.write_oam(6, 0x00);
    f.ppu.write_oam(7, 0x50);

    f.ppu.write(0x2001, 0x1E);
    // Evaluation for scanline 20 runs at dot 257 of scanline 19, so by the
    // time scanline 20 begins the list is ready.
    run_to_scanline(f.ppu, 20);

    EXPECT_EQ(f.ppu.sprites_on_scanline(), 1) << "only sprite 0 is in range";
}

TEST(Ppu, OnlyEightSpritesAreDrawnPerScanline)
{
    Fixture f;

    for (int i = 0; i < 12; ++i) {
        f.ppu.write_oam(static_cast<u8>(i * 4 + 0), 19);   // all on line 20
        f.ppu.write_oam(static_cast<u8>(i * 4 + 1), 0x10);
        f.ppu.write_oam(static_cast<u8>(i * 4 + 2), 0x00);
        f.ppu.write_oam(static_cast<u8>(i * 4 + 3), static_cast<u8>(i * 8));
    }

    f.ppu.write(0x2001, 0x1E);
    run_to_scanline(f.ppu, 20);

    EXPECT_EQ(f.ppu.sprites_on_scanline(), 8);
    EXPECT_TRUE((f.ppu.status() & 0x20) != 0) << "and the overflow flag is set";
}

TEST(Ppu, ASpriteIsDrawnAtItsXPosition)
{
    // Sprite tile 1: every pixel of the top row is colour 1.
    std::vector<u8> chr(8192, 0);
    chr[1 * 16 + 0] = 0xFF;   // plane 0, row 0: all eight pixels opaque
    (void)0;
    chr[1 * 16 + 8] = 0x00;   // plane 1, row 0 lives at byte 8

    Fixture f;
    f.set_cartridge(make_ines_with_chr(chr));

    f.write_vram(0x3F00, 0x0F);   // universal background
    f.write_vram(0x3F11, 0x27);   // sprite palette 0, colour 1

    f.ppu.write_oam(0, 19);    // Y: the sprite's first row is scanline 20
    f.ppu.write_oam(1, 0x01);  // tile 1
    f.ppu.write_oam(2, 0x00);  // attributes
    f.ppu.write_oam(3, 0x20);  // X = 32

    f.ppu.write(0x2001, 0x1E);
    f.ppu.write(0x2005, 0);
    f.ppu.write(0x2005, 0);
    f.set_vram_address(0x0000);

    render_visible_frame(f.ppu);

    EXPECT_EQ(f.ppu.framebuffer().at(32, 20), nes::Ppu::colour(0x27));
    EXPECT_EQ(f.ppu.framebuffer().at(31, 20), nes::Ppu::colour(0x0F)) << "just outside";
}

TEST(Ppu, SpriteZeroHitFiresWhenAnOpaqueSpriteMeetsOpaqueBackground)
{
    // Background tile 1: every pixel is colour 1.
    std::vector<u8> chr(8192, 0);
    for (int row = 0; row < 8; ++row) {
        chr[1 * 16 + row] = 0xFF;   // background tile
        chr[2 * 16 + row] = 0xFF;   // the same shape, used as a sprite tile
    }

    Fixture f;
    f.set_cartridge(make_ines_with_chr(chr));

    f.write_vram(0x3F00, 0x0F);
    f.write_vram(0x3F01, 0x21);
    f.write_vram(0x3F11, 0x27);

    f.write_vram(0x2000, 1);   // background tile 1 across the top left

    // Y = 0 puts the sprite's first row on scanline 1, which is inside the
    // tile just written. Y = 19 would land in a blank tile row and there
    // would be nothing opaque to hit.
    f.ppu.write_oam(0, 0);     // sprite 0 on scanline 1
    f.ppu.write_oam(1, 0x02);
    f.ppu.write_oam(2, 0x00);
    f.ppu.write_oam(3, 0x00);

    f.ppu.write(0x2001, 0x1E);
    f.ppu.write(0x2005, 0);
    f.ppu.write(0x2005, 0);
    f.set_vram_address(0x0000);

    render_visible_frame(f.ppu);

    EXPECT_TRUE(f.ppu.sprite_zero_hit());
    EXPECT_TRUE((f.ppu.status() & 0x40) != 0);
}

TEST(Ppu, SpriteZeroHitDoesNotFireOnATransparentSpritePixel)
{
    std::vector<u8> chr(8192, 0);
    for (int row = 0; row < 8; ++row) {
        chr[1 * 16 + row] = 0xFF;   // background is opaque
        // tile 2 stays blank, so the sprite is transparent
    }

    Fixture f;
    f.set_cartridge(make_ines_with_chr(chr));

    f.write_vram(0x3F00, 0x0F);
    f.write_vram(0x3F01, 0x21);
    f.write_vram(0x3F11, 0x27);
    f.write_vram(0x2000, 1);

    f.ppu.write_oam(0, 0);      // scanline 1
    f.ppu.write_oam(1, 0x02);   // blank tile
    f.ppu.write_oam(2, 0x00);
    f.ppu.write_oam(3, 0x00);

    f.ppu.write(0x2001, 0x1E);
    f.ppu.write(0x2005, 0);
    f.ppu.write(0x2005, 0);
    f.set_vram_address(0x0000);

    render_visible_frame(f.ppu);

    EXPECT_FALSE(f.ppu.sprite_zero_hit()) << "transparent pixels cannot hit";
}

TEST(Ppu, SpriteZeroHitNeedsBothBackgroundAndSpritesEnabled)
{
    std::vector<u8> chr(8192, 0);
    for (int row = 0; row < 8; ++row) {
        chr[1 * 16 + row] = 0xFF;
        chr[2 * 16 + row] = 0xFF;
    }

    Fixture f;
    f.set_cartridge(make_ines_with_chr(chr));
    f.write_vram(0x3F00, 0x0F);
    f.write_vram(0x3F01, 0x21);
    f.write_vram(0x3F11, 0x27);
    f.write_vram(0x2000, 1);

    f.ppu.write_oam(0, 0);      // scanline 1
    f.ppu.write_oam(1, 0x02);
    f.ppu.write_oam(2, 0x00);
    f.ppu.write_oam(3, 0x00);

    f.ppu.write(0x2001, 0x08);   // background only, no sprites
    f.ppu.write(0x2005, 0);
    f.ppu.write(0x2005, 0);
    f.set_vram_address(0x0000);

    render_visible_frame(f.ppu);

    EXPECT_FALSE(f.ppu.sprite_zero_hit());
}

TEST(Ppu, SpritesCanBeFlippedHorizontally)
{
    // Tile 1: only the leftmost pixel of row 0 is opaque.
    std::vector<u8> chr(8192, 0);
    chr[1 * 16 + 0] = 0x80;   // plane 0, bit 7 set

    Fixture f;
    f.set_cartridge(make_ines_with_chr(chr));
    f.write_vram(0x3F00, 0x0F);
    f.write_vram(0x3F11, 0x27);

    f.ppu.write_oam(0, 19);
    f.ppu.write_oam(1, 0x01);
    f.ppu.write_oam(2, 0x40);   // horizontal flip
    f.ppu.write_oam(3, 0x20);

    f.ppu.write(0x2001, 0x1E);
    f.ppu.write(0x2005, 0);
    f.ppu.write(0x2005, 0);
    f.set_vram_address(0x0000);

    render_visible_frame(f.ppu);

    EXPECT_EQ(f.ppu.framebuffer().at(39, 20), nes::Ppu::colour(0x27))
        << "flipped to the right end of the sprite";
    EXPECT_EQ(f.ppu.framebuffer().at(32, 20), nes::Ppu::colour(0x0F));
}

// ===========================================================================
// Palette
// ===========================================================================

TEST(Ppu, ColourTableHasSixtyFourEntries)
{
    // Index 0 is a grey, index $0F is black, index $30 is white.
    EXPECT_EQ(nes::Ppu::colour(0x00), 0x666666u);
    EXPECT_EQ(nes::Ppu::colour(0x0F), 0x000000u);
    EXPECT_EQ(nes::Ppu::colour(0x30), 0xFFFEFFu);
}

TEST(Ppu, ColourIndexWrapsAtSixtyFour)
{
    EXPECT_EQ(nes::Ppu::colour(0x40), nes::Ppu::colour(0x00));
    EXPECT_EQ(nes::Ppu::colour(0xFF), nes::Ppu::colour(0x3F));
}

TEST(Ppu, GreyscaleKeepsOnlyTheTwoHighBits)
{
    // The greyscale bit does not desaturate; it collapses the palette onto
    // the four grey rows, which is what the hardware does.
    EXPECT_EQ(nes::Ppu::colour(0x21, true), nes::Ppu::colour(0x20, true));
    EXPECT_EQ(nes::Ppu::colour(0x2F, true), nes::Ppu::colour(0x20, true));
    EXPECT_NE(nes::Ppu::colour(0x21, true), nes::Ppu::colour(0x21, false));
}

// ===========================================================================
// The machine ties CPU and PPU together
// ===========================================================================

TEST(Machine, LoadsARomAndWiresUpEverything)
{
    nes::Machine machine;

    std::string error;
    ASSERT_TRUE(machine.load_rom(make_ines(2, 1), error)) << error;

    EXPECT_NE(machine.cartridge(), nullptr);
    EXPECT_EQ(machine.ppu().scanline(), -1);
    // Every PRG byte is zero, so the reset vector at $FFFC is $0000 too.
    EXPECT_EQ(machine.cpu().registers().pc, 0x0000)
        << "an all-zero PRG has an all-zero reset vector";
}

TEST(Machine, RunsAFrameAndKeepsThePpuInStep)
{
    // A program that just spins, so the frame can complete without the CPU
    // doing anything in particular.
    std::vector<u8> rom = make_ines(2, 1);
    rom[16 + 0] = 0x4C;   // JMP $8000
    rom[16 + 1] = 0x00;
    rom[16 + 2] = 0x80;
    rom[16 + 0x7FFC] = 0x00;
    rom[16 + 0x7FFD] = 0x80;

    nes::Machine machine;
    std::string error;
    ASSERT_TRUE(machine.load_rom(rom, error)) << error;

    ASSERT_TRUE(machine.run_frame());

    EXPECT_EQ(machine.ppu().frame_count(), 1);
    EXPECT_FALSE(machine.cpu().is_halted());

    // One frame is 262 * 341 = 89342 PPU dots, which is ~29781 CPU cycles.
    EXPECT_GE(machine.cpu().total_cycles(), 29000u);
    EXPECT_LE(machine.cpu().total_cycles(), 31000u);
}

TEST(Machine, ThreePpuDotsPerCpuCycle)
{
    std::vector<u8> rom = make_ines(2, 1);
    rom[16 + 0] = 0xEA;   // NOP
    rom[16 + 0x7FFC] = 0x00;
    rom[16 + 0x7FFD] = 0x80;

    nes::Machine machine;
    std::string error;
    ASSERT_TRUE(machine.load_rom(rom, error)) << error;

    const int start_dot = machine.ppu().dot();
    const int start_line = machine.ppu().scanline();
    const int dots_before = start_line * nes::Ppu::kDotsPerScanline + start_dot;

    (void)machine.run_instructions(1);   // one NOP = 2 CPU cycles = 6 dots

    const int dots_after = machine.ppu().scanline() * nes::Ppu::kDotsPerScanline
                         + machine.ppu().dot();
    EXPECT_EQ(dots_after - dots_before, 6);
}

// ===========================================================================
// The background renderer over the WHOLE screen, and with scrolling
// ===========================================================================
//
// The earlier tests only ever looked at the top-left 8x8 tile. That is not
// enough: a bug in the coarse X/Y stepping, in the nametable selection or in
// the attribute indexing would all be invisible there.

namespace {

/// A cartridge whose CHR has one solid tile at index 1 and another at 2.
std::vector<u8> make_marker_chr()
{
    std::vector<u8> chr(8192, 0);
    for (int row = 0; row < 8; ++row) {
        chr[1 * 16 + row] = 0xFF;   // tile 1: solid, colour 1
        chr[2 * 16 + row] = 0xFF;   // tile 2: solid, colour 1
    }
    return chr;
}

/// A PPU set up so that tile 1 is colour $21 and everything else is $0F.
///
/// Note the order in the constructor and in set_scroll(): the scroll must be
/// written AFTER any $2006 access. The second $2006 write does `v = t`, so
/// writing $2006 after $2005 throws the scroll away - which is a real trap,
/// not a quirk of this test.
struct PaintFixture {
    Fixture f;

    PaintFixture()
    {
        f.set_cartridge(make_ines_with_chr(make_marker_chr()));
        f.write_vram(0x3F00, 0x0F);   // universal background
        f.write_vram(0x3F01, 0x21);   // background palette 0, colour 1
        f.ppu.write(0x2001, 0x0A);    // background on, including the left column
        f.set_vram_address(0x0000);
    }

    void set_scroll(u8 x, u8 y)
    {
        f.ppu.write(0x2005, x);
        f.ppu.write(0x2005, y);
    }

    void finish()
    {
        render_visible_frame(f.ppu);
    }

    [[nodiscard]] u32 foreground() const { return nes::Ppu::colour(0x21); }
    [[nodiscard]] u32 background() const { return nes::Ppu::colour(0x0F); }
};

} // namespace

TEST(Ppu, ABackgroundTileAppearsAtItsExactScreenPosition)
{
    // Nametable column 5, row 10 is pixels x=40..47, y=80..87.
    PaintFixture p;
    p.f.write_vram(0x2000 + 10 * 32 + 5, 1);
    p.set_scroll(0, 0);
    p.finish();

    const auto& fb = p.f.ppu.framebuffer();

    EXPECT_EQ(fb.at(40, 80), p.foreground()) << "top left of the tile";
    EXPECT_EQ(fb.at(47, 87), p.foreground()) << "bottom right of the tile";

    EXPECT_EQ(fb.at(39, 80), p.background()) << "one pixel to the left";
    EXPECT_EQ(fb.at(48, 80), p.background()) << "one pixel to the right";
    EXPECT_EQ(fb.at(40, 79), p.background()) << "one pixel above";
    EXPECT_EQ(fb.at(40, 88), p.background()) << "one pixel below";
}

TEST(Ppu, AWholeRowOfMarkersLandsOnTheRightColumns)
{
    // Mark every 4th tile on nametable row 12, then check each one.
    PaintFixture p;
    for (int col = 0; col < 32; col += 4) {
        p.f.write_vram(static_cast<u16>(0x2000 + 12 * 32 + col), 1);
    }
    p.set_scroll(0, 0);
    p.finish();

    const auto& fb = p.f.ppu.framebuffer();
    for (int col = 0; col < 32; col += 4) {
        const int x = col * 8;
        const int y = 12 * 8;
        if (col == 0) {
            // The very first tile is the "left column" case, still drawn.
            EXPECT_EQ(fb.at(x, y), p.foreground()) << "col " << col;
        } else {
            EXPECT_EQ(fb.at(x, y), p.foreground()) << "col " << col;
        }
        // Only every fourth tile was marked, so the tile right after each
        // marker must be background.
        EXPECT_EQ(fb.at(x + 8, y), p.background()) << "gap after col " << col;
    }
}

TEST(Ppu, EveryMarkerOnTheLastNametableRowLandsCorrectly)
{
    // Row 29 is pixels y=232..239, the last visible row. An off-by-one in
    // the vertical stepping shows up here first.
    PaintFixture p;
    for (int col = 0; col < 32; ++col) {
        p.f.write_vram(static_cast<u16>(0x2000 + 29 * 32 + col), 1);
    }
    p.set_scroll(0, 0);
    p.finish();

    const auto& fb = p.f.ppu.framebuffer();
    for (int col = 0; col < 32; ++col) {
        EXPECT_EQ(fb.at(col * 8, 232), p.foreground()) << "col " << col;
        EXPECT_EQ(fb.at(col * 8 + 7, 239), p.foreground()) << "col " << col;
    }
}

TEST(Ppu, CoarseScrollMovesTheBackgroundByWholeTiles)
{
    PaintFixture p;
    p.f.write_vram(0x2000 + 10 * 32 + 5, 1);
    p.set_scroll(3 * 8, 2 * 8);   // three tiles right, two tiles down
    p.finish();

    const auto& fb = p.f.ppu.framebuffer();

    // The tile was at (40, 80); scrolling right and down moves it up and left.
    EXPECT_EQ(fb.at(40 - 24, 80 - 16), p.foreground());
    EXPECT_EQ(fb.at(47 - 24, 87 - 16), p.foreground());
}

TEST(Ppu, FineScrollShiftsTheBackgroundByPixels)
{
    PaintFixture p;
    p.f.write_vram(0x2000 + 10 * 32 + 5, 1);
    p.set_scroll(4, 0);   // four pixels right
    p.finish();

    const auto& fb = p.f.ppu.framebuffer();

    // The tile starts at x=40 and the view is shifted 4 pixels right, so the
    // tile now starts at x=36. The leftmost four pixels come from the tile
    // before it, which is blank.
    EXPECT_EQ(fb.at(36, 80), p.foreground());
    EXPECT_EQ(fb.at(43, 80), p.foreground());
    EXPECT_EQ(fb.at(35, 80), p.background());
    EXPECT_EQ(fb.at(44, 80), p.background());
}

TEST(Ppu, ScrollingPastTheRightEdgeUsesTheOtherNametable)
{
    // Vertical mirroring: $2000/$2800 are one nametable, $2400/$2C00 the other.
    PaintFixture p;

    // Put the marker only in nametable 1, one tile in from its left edge.
    p.f.write_vram(0x2400, 1);

    // Scroll right by 31 tiles + 8 pixels: the 32nd tile column is the first
    // column of the next nametable.
    p.set_scroll(31 * 8 + 0, 0);
    p.finish();

    const auto& fb = p.f.ppu.framebuffer();
    EXPECT_EQ(fb.at(8, 0), p.foreground())
        << "nametable 1 column 0 appears at x = 8 when scrolled 31 tiles";
}

TEST(Ppu, TheLastRealTileRowCanBeScrolledTo)
{
    PaintFixture p;

    // Nametable row 29 is the last one that is really a tile row: pixels
    // 232-239. Scrolling to it must show it, not the attribute table.
    p.f.write_vram(0x2000 + 29 * 32, 1);   // row 29, column 0
    p.set_scroll(0, 29 * 8);
    p.finish();

    const auto& fb = p.f.ppu.framebuffer();
    EXPECT_EQ(fb.at(0, 0), p.foreground()) << "row 29 lands at the top of the screen";
}

TEST(Ppu, TheVerticalWrapHappensOnlyWhileRendering)
{
    // Rows 30 and 31 of a nametable are not tile rows at all: they are where
    // the attribute table lives. Setting the scroll there directly reads
    // attributes as if they were tiles, which is why no game does it.
    //
    // The wrap to row 0 happens through increment_y while rendering, not
    // through the scroll registers, and only when coarse Y reaches 29 or 31.
    PaintFixture p;

    p.f.write_vram(0x2000 + 29 * 32, 1);   // row 29, column 0
    p.set_scroll(0, 29 * 8 + 7);           // last scanline of row 29
    p.finish();

    const auto& fb = p.f.ppu.framebuffer();

    // Scanline 0 shows the last row of tile row 29.
    EXPECT_EQ(fb.at(0, 0), p.foreground());

    // Scanline 1 is the next tile row, which wraps to row 0 of the OTHER
    // nametable (because 29 wrapped). That nametable is empty, so it is
    // background - not the attribute table, and not row 30.
    EXPECT_EQ(fb.at(0, 1), p.background());
}

// ===========================================================================
// When is it safe to write VRAM?
// ===========================================================================

TEST(Ppu, WritesDuringVisibleRenderingAreCountedSeparately)
{
    // The CPU and the rendering pipeline share the `v` address register. A
    // $2007 write during the visible part of the frame moves the PPU's own
    // fetch pointer, so the write and the fetch corrupt each other. Games
    // avoid it; this counter is how you check whether one is doing it.
    Fixture f;
    f.ppu.write(0x2001, 0x1E);   // rendering on

    // Run into the visible part of the frame.
    run_to_scanline(f.ppu, 100);
    ASSERT_GE(f.ppu.scanline(), 0);
    ASSERT_LT(f.ppu.scanline(), 240);

    f.set_vram_address(0x2000);
    f.ppu.write(0x2007, 0x11);

    EXPECT_EQ(f.ppu.vram_writes_visible(), 1u);
    EXPECT_EQ(f.ppu.vram_writes_blanking(), 0u);
}

TEST(Ppu, WritesDuringVblankAreSafe)
{
    Fixture f;
    f.ppu.write(0x2001, 0x1E);

    run_to_scanline(f.ppu, 245);   // vblank
    ASSERT_GE(f.ppu.scanline(), 241);

    f.set_vram_address(0x2000);
    f.ppu.write(0x2007, 0x22);

    EXPECT_EQ(f.ppu.vram_writes_visible(), 0u);
    EXPECT_EQ(f.ppu.vram_writes_blanking(), 1u);
}

TEST(Ppu, ReadsDuringVisibleRenderingAreAlsoDangerous)
{
    // A $2007 READ advances the same v register a write does, so it corrupts
    // the fetch pointer just as badly. Counting it separately is how a game
    // that polls VRAM mid frame gets found.
    Fixture f;
    f.ppu.write(0x2001, 0x1E);

    run_to_scanline(f.ppu, 100);
    ASSERT_GE(f.ppu.scanline(), 0);
    ASSERT_LT(f.ppu.scanline(), 240);

    f.set_vram_address(0x2000);
    (void)f.ppu.read(0x2007);

    EXPECT_EQ(f.ppu.vram_reads_visible(), 1u);
}

TEST(Ppu, ReadsWhileRenderingIsOffAreSafe)
{
    Fixture f;
    f.ppu.write(0x2001, 0x00);

    run_to_scanline(f.ppu, 100);

    f.set_vram_address(0x2000);
    (void)f.ppu.read(0x2007);

    EXPECT_EQ(f.ppu.vram_reads_visible(), 0u);
}

TEST(Ppu, WritesWhileRenderingIsOffAreSafe)
{
    Fixture f;
    f.ppu.write(0x2001, 0x00);   // rendering off, so the pipeline is idle

    run_to_scanline(f.ppu, 100);

    f.set_vram_address(0x2000);
    f.ppu.write(0x2007, 0x33);

    EXPECT_EQ(f.ppu.vram_writes_visible(), 0u)
        << "no pipeline is using v, so a visible scanline is harmless";
    EXPECT_EQ(f.ppu.vram_writes_blanking(), 1u);
}

// A block write is the other half of that promise. Forced blanking is how a
// game loads a screen: point $2006 at a nametable, then push hundreds of
// bytes through $2007 in a loop. The CPU's write address and the rendering
// pipeline's fetch pointer are the SAME register, v, so if the pipeline kept
// stepping v (increment_y at dot 256, copy_x at dot 257) while blanked, the
// block would smear across VRAM. Super Mario Bros clears its nametables this
// way before every level; with unguarded stepping the clear dies after one
// scanline and the untouched bytes stay at tile $00, so the screen fills with
// the digit "0".
TEST(Ppu, ABlockWriteDuringForcedBlankingLandsWhereItWasPointed)
{
    Fixture f;
    f.ppu.write(0x2001, 0x00);   // forced blanking: v belongs to the CPU

    // One full nametable row, with a real $2007 write and its four CPU
    // cycles (twelve PPU dots) between each byte, so the loop crosses a
    // scanline boundary - the moment the bug used to strike.
    f.set_vram_address(0x2000);
    for (u16 i = 0; i < 32; ++i) {
        f.ppu.write(0x2007, static_cast<u8>(0x40 + i));
        f.ppu.tick(12);
    }

    for (u16 i = 0; i < 32; ++i) {
        EXPECT_EQ(f.ppu.read_vram(static_cast<u16>(0x2000 + i)),
                  static_cast<u8>(0x40 + i))
            << "byte " << i << " of the block landed somewhere else";
    }
}
