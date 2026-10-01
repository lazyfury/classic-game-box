#pragma once

// ---------------------------------------------------------------------------
// The PPU - Picture Processing Unit (Ricoh 2C02).
//
// The PPU is a separate computer from the CPU. It has its own address space,
// its own memory, its own clock, and it runs three times faster. The CPU
// cannot touch any of it directly; it can only read and write eight registers,
// and everything the PPU does is a consequence of those eight bytes.
//
//     CPU $2000-$2007  ->  eight registers  ->  the whole picture
//
// Two address spaces
// ------------------
// The PPU has its OWN 14 bit bus ($0000-$3FFF), reachable only through the
// $2006/$2007 register pair:
//
//     $0000-$1FFF   pattern tables   (CHR ROM, on the cartridge)
//     $2000-$2FFF   nametables       (2KB VRAM on the motherboard)
//     $3000-$3EFF   mirror of the nametables
//     $3F00-$3F1F   palette RAM      (32 bytes)
//     $3F20-$3FFF   mirror of the palette
//
// This is why `read_chr` on the Cartridge is not part of the Device interface:
// pattern tables are not in the CPU's address space at all.
//
// How a picture is made
// ---------------------
// The PPU draws one pixel per dot (cycle), 341 dots per scanline, 262
// scanlines per frame:
//
//     scanline -1        pre-render: clear flags, copy the scroll registers
//     scanline 0-239     visible: draw 256 pixels each
//     scanline 240       post-render: idle
//     scanline 241-260   vblank: tell the CPU, and wait
//
// It never stores a picture. It streams pixels out of two 16 bit shift
// registers, reloaded from memory every eight pixels. We store the result in a
// Framebuffer only because the modern world expects an image.
//
// The scroll registers
// --------------------
// There is no "scroll X" byte. Scrolling is done with a 15 bit VRAM address
// split into five fields, duplicated into two registers (v and t) plus a fine
// X counter. The names below are the ones from the hardware's own schematic,
// because everyone who has ever written a PPU uses them:
//
//     yyy NN YYYYY XXXXX
//     ||| || ||||| +++++-- coarse X   (which of 32 tiles across)
//     ||| || +++++-------- coarse Y   (which of 30 tiles down)
//     ||| ++-------------- nametable select
//     +++----------------- fine Y      (which of 8 pixel rows of a tile)
//
//     v = the address the PPU is currently fetching from
//     t = where the next frame/scanline should start
//     x = fine X scroll, 0-7
//     w = which half of a two-write register is next
//
// Writing $2005 or $2006 sets t, and then the hardware copies pieces of t into
// v at fixed moments. That copying IS scrolling. It is the single hardest part
// of the PPU, and it is why this file is long.
// ---------------------------------------------------------------------------

#include "core/state_fwd.hpp"
#include "core/nes/cartridge.hpp"
#include "core/nes/device.hpp"
#include "core/nes/framebuffer.hpp"
#include "core/types.hpp"

#include <array>

namespace fc::nes {

class Ppu : public Device, public OamTarget {
public:
    static constexpr int kDotsPerScanline = 341;
    static constexpr int kScanlinesPerFrame = 262;
    static constexpr int kVisibleScanlines = 240;
    static constexpr int kLastScanline = 260;      // -1 .. 260 is 262 lines
    static constexpr int kPpuCyclesPerCpuCycle = 3;

    Ppu() noexcept = default;

    void reset() noexcept;

    void set_cartridge(Cartridge* cartridge) noexcept { cartridge_ = cartridge; }

    // -- Device: the CPU's view, $2000-$3FFF --------------------------------

    [[nodiscard]] u8 read(u16 address) override;
    void write(u16 address, u8 value) override;

    // -- OamTarget: the 256 bytes an OAM DMA pushes in ----------------------

    void write_oam(u8 index, u8 value) override;

    // -- timing --------------------------------------------------------------

    /// Advance one dot. Call this three times per CPU cycle.
    void tick() noexcept;

    /// Advance `count` dots. The machine loop calls this with 3 * cpu cycles.
    void tick(int count) noexcept;

    /// True once per vblank, cleared when read. The CPU's NMI input.
    [[nodiscard]] bool consume_nmi() noexcept;

    [[nodiscard]] int scanline() const noexcept { return scanline_; }
    [[nodiscard]] int dot() const noexcept { return dot_; }
    [[nodiscard]] int frame_count() const noexcept { return frame_count_; }

    /// True when either background or sprites are on.
    [[nodiscard]] bool rendering_enabled() const noexcept { return (mask_ & 0x18) != 0; }

    // -- output --------------------------------------------------------------

    [[nodiscard]] const Framebuffer& framebuffer() const noexcept { return framebuffer_; }

    // -- inspection, for tests and for tools ---------------------------------

    /// The PPU's own address space, with the mirroring applied.
    [[nodiscard]] u8 read_vram(u16 address) noexcept;

    /// The raw nametable byte at `address`, before mirroring, for tools.
    void write_vram(u16 address, u8 value) noexcept;

    /// Palette RAM with the $3F10 -> $3F00 mirroring applied, so index
    /// $10 really does read slot $00.
    [[nodiscard]] u8 palette_ram(u8 index) const noexcept;

    /// Palette RAM as stored, without the mirroring. For tools that want to
    /// see the raw bytes.
    [[nodiscard]] u8 raw_palette_ram(u8 index) const noexcept { return palette_ram_[index & 0x1F]; }
    [[nodiscard]] u8 oam(u8 index) const noexcept { return oam_[index]; }
    [[nodiscard]] u8 status() const noexcept { return status_; }
    [[nodiscard]] u8 control() const noexcept { return ctrl_; }
    [[nodiscard]] u8 mask() const noexcept { return mask_; }
    [[nodiscard]] u16 vram_address() const noexcept { return v_; }
    [[nodiscard]] u16 temp_address() const noexcept { return t_; }
    [[nodiscard]] u8 fine_x() const noexcept { return fine_x_; }
    [[nodiscard]] bool sprite_zero_hit() const noexcept { return sprite_zero_hit_; }

    /// How many sprites were selected for the scanline currently being drawn.
    [[nodiscard]] int sprites_on_scanline() const noexcept { return sprite_count_; }

    /// Counts of $2007 accesses, split by whether the PPU was in the visible
    /// part of the frame. A game that writes VRAM while the picture is being
    /// drawn is fighting the PPU's own use of `v`, and the writes land in the
    /// wrong place. Watching this counter is how you find out whether that is
    /// happening, instead of guessing from a torn picture.
    [[nodiscard]] u64 vram_writes_visible() const noexcept { return vram_writes_visible_; }
    [[nodiscard]] u64 vram_writes_blanking() const noexcept { return vram_writes_blanking_; }

    /// Writes during the pre-render line, where the fetch pipeline is also
    /// running. Dangerous for the same reason as the visible range.
    [[nodiscard]] u64 vram_writes_prerender() const noexcept { return vram_writes_prerender_; }

    /// $2007 READS during the visible frame. A read also advances `v`, so it
    /// corrupts the fetch pointer exactly the same way a write does.
    [[nodiscard]] u64 vram_reads_visible() const noexcept { return vram_reads_visible_; }

    /// Resolve a palette index to RGB, applying the greyscale bit.
    [[nodiscard]] static u32 colour(u8 palette_index, bool greyscale = false) noexcept;

private:
    // -- the eight registers -------------------------------------------------
    u8 ctrl_ = 0;        // $2000 PPUCTRL
    u8 mask_ = 0;        // $2001 PPUMASK
    u8 status_ = 0x00;   // $2002 PPUSTATUS
    u8 oam_addr_ = 0;    // $2003 OAMADDR
    u8 read_buffer_ = 0; // the $2007 read latch
    u8 io_latch_ = 0;    // the last byte on the PPU's register bus

    // -- scrolling -----------------------------------------------------------
    u16 v_ = 0;          // current VRAM address
    u16 t_ = 0;          // temporary VRAM address
    u8 fine_x_ = 0;      // fine X scroll, 0-7
    bool write_toggle_ = false;   // w: which half of a two write register

    // -- timing --------------------------------------------------------------
    int scanline_ = -1;
    int dot_ = 0;
    int frame_count_ = 0;

    // -- NMI -----------------------------------------------------------------
    bool nmi_occurred_ = false;

    // -- memory --------------------------------------------------------------
    std::array<u8, 0x1000> nametables_{};   // 4KB, for four screen carts
    std::array<u8, 32> palette_ram_{};
    std::array<u8, 256> oam_{};

    // -- the background pipeline --------------------------------------------
    u8 next_tile_id_ = 0;
    u8 next_tile_attr_ = 0;
    u8 next_tile_lsb_ = 0;
    u8 next_tile_msb_ = 0;
    u16 shifter_lo_ = 0;
    u16 shifter_hi_ = 0;
    u16 attr_lo_ = 0;
    u16 attr_hi_ = 0;

    // -- the sprites selected for the scanline being drawn -------------------
    struct SpriteLine {
        u8 x = 0;
        u8 attr = 0;
        u8 pattern_lo = 0;
        u8 pattern_hi = 0;
        bool is_sprite_zero = false;
    };
    std::array<SpriteLine, 8> sprite_line_{};
    int sprite_count_ = 0;
    bool sprite_zero_in_range_ = false;
    bool sprite_zero_hit_ = false;
    bool sprite_overflow_ = false;

    u64 vram_writes_visible_ = 0;
    u64 vram_writes_prerender_ = 0;
    u64 vram_reads_visible_ = 0;
    u64 vram_writes_blanking_ = 0;

    Framebuffer framebuffer_{};
    Cartridge* cartridge_ = nullptr;

    // -- helpers -------------------------------------------------------------
    [[nodiscard]] u16 nametable_index(u16 address) const noexcept;
    [[nodiscard]] u8 palette_index(u16 address) const noexcept;

    void render_dot() noexcept;
    void on_new_scanline() noexcept;

    void update_shifters() noexcept;
    void load_shifters() noexcept;
    void fetch_tile_id() noexcept;
    void fetch_tile_attr() noexcept;
    void fetch_tile_lsb() noexcept;
    void fetch_tile_msb() noexcept;

    void increment_x() noexcept;
    void increment_y() noexcept;
    void copy_x() noexcept;
    void copy_y() noexcept;

    void render_pixel() noexcept;
    void evaluate_sprites(int line) noexcept;

    friend struct fc::StateAccess;
};

} // namespace fc::nes
