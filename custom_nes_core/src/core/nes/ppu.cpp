#include "core/nes/ppu.hpp"

namespace fc::nes {

namespace {

/// The 2C02's 64 colours, as 0x00RRGGBB.
///
/// The real chip outputs an NTSC composite signal, so "the" colours are a
/// matter of which TV you plugged into. These are the widely used values from
/// the NESdev wiki's measured palette. Emulators differ in the last few bits;
/// they agree on the shape.
constexpr u32 kPalette[64] = {
    0x666666, 0x002A88, 0x1412A7, 0x3B00A4, 0x5C007E, 0x6E0040, 0x6C0600, 0x561D00,
    0x333500, 0x0B4800, 0x005200, 0x004F08, 0x00404D, 0x000000, 0x000000, 0x000000,
    0xADADAD, 0x155FD9, 0x4240FF, 0x7527FE, 0xA01ACC, 0xB71E7B, 0xB53120, 0x994E00,
    0x6B6D00, 0x388700, 0x0C9300, 0x008F32, 0x007C8D, 0x000000, 0x000000, 0x000000,
    0xFFFEFF, 0x64B0FF, 0x9290FF, 0xC676FF, 0xF36AFF, 0xFE6ECC, 0xFE8170, 0xEA9E22,
    0xBCBE00, 0x88D800, 0x5CE430, 0x45E082, 0x48CDDE, 0x4F4F4F, 0x000000, 0x000000,
    0xFFFEFF, 0xC0DFFF, 0xD3D2FF, 0xE8C8FF, 0xFBC2FF, 0xFEC4EA, 0xFECCC5, 0xF7D8A5,
    0xE4E594, 0xCFEF96, 0xBDF4AB, 0xB3F3CC, 0xB5EBF2, 0xB8B8B8, 0x000000, 0x000000,
};

/// Reverse the eight bits of a byte. Used to flip a sprite horizontally:
/// rather than reading the pattern backwards, read it forwards and mirror it.
[[nodiscard]] constexpr u8 reverse_bits(u8 value) noexcept
{
    value = static_cast<u8>((value & 0xF0u) >> 4 | (value & 0x0Fu) << 4);
    value = static_cast<u8>((value & 0xCCu) >> 2 | (value & 0x33u) << 2);
    value = static_cast<u8>((value & 0xAAu) >> 1 | (value & 0x55u) << 1);
    return value;
}

} // namespace

// ===========================================================================
// Colour
// ===========================================================================

u32 Ppu::colour(u8 palette_index, bool greyscale) noexcept
{
    // The greyscale bit keeps bits 4 and 5 and throws the rest away, so it
    // collapses the palette to one row of greys rather than desaturating.
    if (greyscale) {
        palette_index = static_cast<u8>(palette_index & 0x30u);
    }
    return kPalette[palette_index & 0x3Fu];
}

// ===========================================================================
// Reset
// ===========================================================================

void Ppu::reset() noexcept
{
    ctrl_ = 0;
    mask_ = 0;
    status_ = 0;
    oam_addr_ = 0;
    read_buffer_ = 0;
    io_latch_ = 0;

    v_ = 0;
    t_ = 0;
    fine_x_ = 0;
    write_toggle_ = false;

    scanline_ = -1;
    dot_ = 0;
    frame_count_ = 0;

    nmi_occurred_ = false;

    nametables_.fill(0);
    palette_ram_.fill(0);
    oam_.fill(0);

    next_tile_id_ = 0;
    next_tile_attr_ = 0;
    next_tile_lsb_ = 0;
    next_tile_msb_ = 0;
    shifter_lo_ = 0;
    shifter_hi_ = 0;
    attr_lo_ = 0;
    attr_hi_ = 0;

    vram_writes_visible_ = 0;
    vram_writes_prerender_ = 0;
    vram_reads_visible_ = 0;
    vram_writes_blanking_ = 0;

    sprite_line_ = {};
    sprite_count_ = 0;
    sprite_zero_in_range_ = false;
    sprite_zero_hit_ = false;
    sprite_overflow_ = false;

    framebuffer_.clear(colour(0x0F));
}

// ===========================================================================
// The PPU's own address space
// ===========================================================================

u16 Ppu::nametable_index(u16 address) const noexcept
{
    // $2000-$2FFF is four 1KB nametables: $2000, $2400, $2800, $2C00.
    // How they are wired is decided by the CARTRIDGE, because the extra VRAM
    // (or its absence) is on the cartridge board.
    const u16 offset = static_cast<u16>((address - 0x2000u) & 0x0FFFu);

    // The mapper, not the header, decides this. A mapper can change it while
    // the game runs: MMC1 games flip between horizontal and vertical (and
    // single screen) to get more than two nametables out of 2KB of VRAM.
    const Mirroring mode = (cartridge_ != nullptr) ? cartridge_->mapper().mirroring()
                                                   : Mirroring::Horizontal;

    switch (mode) {
    case Mirroring::Vertical:
        // $2000 and $2800 are the same, $2400 and $2C00 are the same.
        // Bit 11 is the one that distinguishes left from right, and it is
        // simply not decoded.
        return static_cast<u16>(offset & 0x07FFu);

    case Mirroring::Horizontal:
        // $2000 and $2400 are the same, $2800 and $2C00 are the same.
        // Bit 10 selects the pair, bit 11 is dropped.
        return static_cast<u16>(((offset >> 1) & 0x0400u) | (offset & 0x03FFu));

    case Mirroring::FourScreen:
        // The cartridge supplies 2KB of its own VRAM, so all four are distinct.
        return offset;

    case Mirroring::SingleScreenLower:
        // Every nametable address answers the first 1KB.
        return static_cast<u16>(offset & 0x03FFu);

    case Mirroring::SingleScreenUpper:
        // Every nametable address answers the second 1KB.
        return static_cast<u16>(0x0400u | (offset & 0x03FFu));
    }
    return static_cast<u16>(offset & 0x03FFu);
}

u8 Ppu::palette_ram(u8 index) const noexcept
{
    return palette_ram_[palette_index(static_cast<u16>(0x3F00u + (index & 0x1Fu)))];
}

u8 Ppu::palette_index(u16 address) const noexcept
{
    u8 index = static_cast<u8>((address - 0x3F00u) & 0x001Fu);

    // $3F10/$3F14/$3F18/$3F1C are not separate bytes: they ARE $3F00 and
    // friends. That is why the first colour of every sprite palette is
    // unreachable and every sprite palette actually has three usable colours.
    if (index == 0x10 || index == 0x14 || index == 0x18 || index == 0x1C) {
        index = static_cast<u8>(index - 0x10u);
    }
    return index;
}

u8 Ppu::read_vram(u16 address) noexcept
{
    address &= 0x3FFFu;

    if (address < 0x2000) {
        if (cartridge_ == nullptr) {
            return 0;
        }
        // Tell the mapper what the PPU just put on its address bus. The MMC3
        // watches bit 12 here and clocks its scanline counter on the rising
        // edge; the MMC2/MMC4 watch for a particular tile. Every other mapper
        // ignores the message, which is why it is safe to always send.
        cartridge_->mapper().on_ppu_address(address);
        return cartridge_->read_chr(address);
    }
    if (address < 0x3F00) {
        return nametables_[nametable_index(address) % nametables_.size()];
    }
    return palette_ram_[palette_index(address)];
}

void Ppu::write_vram(u16 address, u8 value) noexcept
{
    address &= 0x3FFFu;

    if (address < 0x2000) {
        if (cartridge_ != nullptr) {
            cartridge_->write_chr(address, value);
        }
        return;
    }
    if (address < 0x3F00) {
        nametables_[nametable_index(address) % nametables_.size()] = value;
        return;
    }
    // Palette RAM only holds six bits per entry.
    palette_ram_[palette_index(address)] = static_cast<u8>(value & 0x3Fu);
}

// ===========================================================================
// The CPU's view: eight registers
// ===========================================================================

u8 Ppu::read(u16 address)
{
    const u8 reg = static_cast<u8>(address & 0x0007u);

    switch (reg) {
    case 2: {   // PPUSTATUS
        // Bits 4-0 are not stored in the register. They come from the last
        // byte that was on the PPU's data bus, so reading $2002 gives you
        // whatever the latch happened to hold.
        const u8 result = static_cast<u8>((status_ & 0xE0u) | (io_latch_ & 0x1Fu));

        // Reading the status clears vblank AND cancels a not-yet-taken NMI.
        status_ = static_cast<u8>(status_ & static_cast<u8>(~0x80u));
        nmi_occurred_ = false;
        write_toggle_ = false;
        return result;
    }

    case 4:     // OAMDATA
        return oam_[oam_addr_];

    case 7: {   // PPUDATA - buffered, and different for palettes
        if (rendering_enabled() && scanline_ >= 0 && scanline_ < kVisibleScanlines) {
            ++vram_reads_visible_;
        }

        const u16 addr = static_cast<u16>(v_ & 0x3FFFu);

        u8 result;
        if (addr >= 0x3F00) {
            // Palette reads are immediate: a palette is small enough that the
            // hardware did not bother buffering it.
            result = read_vram(addr);

            // But the buffer still gets filled, with the nametable byte
            // "underneath" that address. Games that care about this are rare
            // and strange.
            read_buffer_ = read_vram(static_cast<u16>(addr - 0x1000u));
        } else {
            result = read_buffer_;
            read_buffer_ = read_vram(addr);
        }

        io_latch_ = result;
        v_ = static_cast<u16>(v_ + ((ctrl_ & 0x04u) ? 32u : 1u));
        return result;
    }

    default:
        // $2000, $2001, $2003, $2005 and $2006 are write only. Reading one
        // returns the open bus latch.
        return read_buffer_;
    }
}

void Ppu::write(u16 address, u8 value)
{
    // Every register write puts the value on the PPU's internal bus, which is
    // where $2002's low five bits come from.
    io_latch_ = value;

    const u8 reg = static_cast<u8>(address & 0x0007u);

    switch (reg) {
    case 0:     // PPUCTRL
        ctrl_ = value;
        // The low two bits are the nametable select, which lives in t.
        t_ = static_cast<u16>((t_ & 0xF3FFu) | (static_cast<u16>(value & 0x03u) << 10));
        break;

    case 1:     // PPUMASK
        mask_ = value;
        break;

    case 3:     // OAMADDR
        oam_addr_ = value;
        break;

    case 4:     // OAMDATA
        oam_[oam_addr_] = value;
        ++oam_addr_;
        break;

    case 5:     // PPUSCROLL - two writes, sharing the toggle with $2006
        if (!write_toggle_) {
            fine_x_ = static_cast<u8>(value & 0x07u);
            // coarse X: bits 3-7 of the value become bits 0-4 of t
            t_ = static_cast<u16>((t_ & 0xFFE0u) | (value >> 3));
            write_toggle_ = true;
        } else {
            // fine Y: the low three bits, shifted up to bits 12-14
            t_ = static_cast<u16>((t_ & 0x8FFFu) | (static_cast<u16>(value & 0x07u) << 12));
            // coarse Y: the high five bits, at bits 5-9
            t_ = static_cast<u16>((t_ & 0xFC1Fu) | (static_cast<u16>(value & 0xF8u) << 2));
            write_toggle_ = false;
        }
        break;

    case 6:     // PPUADDR
        if (!write_toggle_) {
            // high byte, six bits only: the PPU bus is 14 bits wide
            t_ = static_cast<u16>((t_ & 0x00FFu) | (static_cast<u16>(value & 0x3Fu) << 8));
            write_toggle_ = true;
        } else {
            t_ = static_cast<u16>((t_ & 0xFF00u) | value);
            // The second write also copies t into v, which is how a program
            // points the PPU at a place in VRAM.
            v_ = t_;
            write_toggle_ = false;
        }
        break;

    case 7: {   // PPUDATA
        // The CPU and the rendering pipeline share `v`. Writing here while
        // the picture is being drawn moves the PPU's own fetch pointer, so
        // the write and the fetch corrupt each other. vram_writes_visible()
        // counts how often a game does that.
        if (rendering_enabled() && scanline_ >= 0 && scanline_ < kVisibleScanlines) {
            ++vram_writes_visible_;
        } else if (rendering_enabled() && scanline_ == -1) {
            ++vram_writes_prerender_;
        } else {
            ++vram_writes_blanking_;
        }
        write_vram(static_cast<u16>(v_ & 0x3FFFu), value);
        v_ = static_cast<u16>((v_ + ((ctrl_ & 0x04u) ? 32u : 1u)) & 0x7FFFu);
        break;
    }

    default:
        break;
    }
}

void Ppu::write_oam(u8 index, u8 value)
{
    oam_[index] = value;
}

// ===========================================================================
// NMI
// ===========================================================================

bool Ppu::consume_nmi() noexcept
{
    if (!nmi_occurred_) {
        return false;
    }
    nmi_occurred_ = false;
    return true;
}

// ===========================================================================
// Timing
// ===========================================================================

void Ppu::tick(int count) noexcept
{
    for (int i = 0; i < count; ++i) {
        tick();
    }
}

void Ppu::tick() noexcept
{
    render_dot();

    ++dot_;
    if (dot_ >= kDotsPerScanline) {
        dot_ = 0;
        ++scanline_;
        if (scanline_ > kLastScanline) {
            scanline_ = -1;
            ++frame_count_;
        }
        on_new_scanline();
    }
}

void Ppu::on_new_scanline() noexcept
{
    if (scanline_ == 241) {
        // Vblank. The one moment of the frame when the CPU is allowed to
        // touch the PPU, and the reason games have a vblank routine at all.
        status_ = static_cast<u8>(status_ | 0x80u);
        if ((ctrl_ & 0x80u) != 0) {
            nmi_occurred_ = true;
        }
    }

    if (scanline_ == -1) {
        // Pre-render line: clear last frame's flags so software cannot see
        // them twice.
        status_ = static_cast<u8>(status_ & static_cast<u8>(~0xE0u));
        sprite_zero_hit_ = false;
        sprite_overflow_ = false;
    }

    // Tell the cartridge where the beam is. Most mappers do not care, but
    // the Nanjing board's automatic 4KB CHR-RAM switch is wired to PPU
    // A13/A9, and a scanline boundary is the closest this PPU gets to it.
    if (cartridge_ != nullptr) {
        cartridge_->mapper().on_scanline(scanline_);
    }
}

void Ppu::render_dot() noexcept
{
    const bool rendering = rendering_enabled();

    // Scanline -1 (pre-render) and 0..239 (visible) both run the fetch
    // pipeline. The pre-render line fetches the first two tiles of the frame
    // so that scanline 0 starts with full shift registers.
    const bool active_line = (scanline_ >= -1) && (scanline_ < kVisibleScanlines);
    if (!active_line) {
        return;
    }

    const bool fetch_window = (dot_ >= 1 && dot_ <= 256) || (dot_ >= 321 && dot_ <= 336);

    if (rendering && fetch_window) {
        update_shifters();

        // The eight dot fetch cycle of the 2C02.
        switch ((dot_ - 1) & 7) {
        case 0:
            load_shifters();
            fetch_tile_id();
            break;
        case 2:
            fetch_tile_attr();
            break;
        case 4:
            fetch_tile_lsb();
            break;
        case 6:
            fetch_tile_msb();
            break;
        case 7:
            increment_x();
            break;
        default:
            break;
        }
    }

    // The scroll counters only move while the PPU is actually drawing.
    //
    // This guard is not a detail: "forced blanking" - both background and
    // sprites off - is exactly when a game is expected to point v at a
    // nametable and pour a screen into it through $2007. If the pipeline
    // kept incrementing v behind the CPU's back, every long write would
    // scatter itself across VRAM. Super Mario Bros clears its nametables
    // this way before every level, and without the guard the clear stops
    // after one scanline and leaves the digit '0' (tile $00) everywhere it
    // did not reach.
    if (rendering) {
        // Vertical movement happens once per scanline, at dot 256.
        if (dot_ == 256) {
            increment_y();
        }

        // Horizontal movement happens once per scanline, at dot 257.
        if (dot_ == 257) {
            load_shifters();
            copy_x();
        }

        // The vertical half of scrolling is copied back at the end of the
        // pre-render line, so the next frame starts where t says.
        if (scanline_ == -1 && dot_ >= 280 && dot_ <= 304) {
            copy_y();
        }

        // Sprite evaluation for the NEXT scanline happens here, while the
        // current one is still being shifted out.
        if (dot_ == 257 && scanline_ >= 0) {
            evaluate_sprites(scanline_ + 1);
        }
    }

    // With rendering off the PPU still shows the backdrop colour, so the
    // pixel loop must keep running: render_pixel picks $3F00 when neither
    // background nor sprites are enabled.
    if (dot_ >= 1 && dot_ <= 256) {
        render_pixel();
    }
}

// ===========================================================================
// The background pipeline
// ===========================================================================

void Ppu::update_shifters() noexcept
{
    if ((mask_ & 0x08u) == 0) {
        return;
    }
    shifter_lo_ = static_cast<u16>(shifter_lo_ << 1);
    shifter_hi_ = static_cast<u16>(shifter_hi_ << 1);
    attr_lo_ = static_cast<u16>(attr_lo_ << 1);
    attr_hi_ = static_cast<u16>(attr_hi_ << 1);
}

void Ppu::load_shifters() noexcept
{
    // The low eight bits are the tile being fetched; the high eight are the
    // tile being drawn. That one-tile delay is what the pipeline is.
    shifter_lo_ = static_cast<u16>((shifter_lo_ & 0xFF00u) | next_tile_lsb_);
    shifter_hi_ = static_cast<u16>((shifter_hi_ & 0xFF00u) | next_tile_msb_);

    // Attribute bits are one per tile, so they are spread across all eight
    // pixels of the tile.
    attr_lo_ = static_cast<u16>((attr_lo_ & 0xFF00u) | ((next_tile_attr_ & 0x01u) ? 0x00FFu : 0x0000u));
    attr_hi_ = static_cast<u16>((attr_hi_ & 0xFF00u) | ((next_tile_attr_ & 0x02u) ? 0x00FFu : 0x0000u));
}

void Ppu::fetch_tile_id() noexcept
{
    // The nametable always lives at $2000-$2FFF; the low 12 bits of v pick
    // which byte of it.
    next_tile_id_ = read_vram(static_cast<u16>(0x2000u | (v_ & 0x0FFFu)));
}

void Ppu::fetch_tile_attr() noexcept
{
    // Attributes live in the last 64 bytes of each nametable, 2 bits per
    // 2x2 tile block.
    const u16 address = static_cast<u16>(0x23C0u | (v_ & 0x0C00u) |
                                          ((v_ >> 4) & 0x0038u) |
                                          ((v_ >> 2) & 0x0007u));

    u8 attr = read_vram(address);

    if ((v_ & 0x0040u) != 0) {
        attr = static_cast<u8>(attr >> 4);   // bottom half of the block
    }
    if ((v_ & 0x0002u) != 0) {
        attr = static_cast<u8>(attr >> 2);   // right half of the block
    }

    next_tile_attr_ = static_cast<u8>(attr & 0x03u);
}

void Ppu::fetch_tile_lsb() noexcept
{
    const u16 table = ((ctrl_ & 0x10u) != 0) ? 0x1000u : 0x0000u;
    const u16 fine_y = static_cast<u16>((v_ >> 12) & 0x07u);

    next_tile_lsb_ = read_vram(static_cast<u16>(table + next_tile_id_ * 16u + fine_y));
}

void Ppu::fetch_tile_msb() noexcept
{
    const u16 table = ((ctrl_ & 0x10u) != 0) ? 0x1000u : 0x0000u;
    const u16 fine_y = static_cast<u16>((v_ >> 12) & 0x07u);

    next_tile_msb_ = read_vram(static_cast<u16>(table + next_tile_id_ * 16u + fine_y + 8u));
}

// ===========================================================================
// Scrolling: copying pieces of t into v
// ===========================================================================

void Ppu::increment_x() noexcept
{
    if ((v_ & 0x001Fu) == 31) {
        v_ = static_cast<u16>(v_ & static_cast<u16>(~0x001Fu));
        v_ = static_cast<u16>(v_ ^ 0x0400u);   // switch horizontal nametable
    } else {
        v_ = static_cast<u16>(v_ + 1u);
    }
}

void Ppu::increment_y() noexcept
{
    if ((v_ & 0x7000u) != 0x7000u) {
        v_ = static_cast<u16>(v_ + 0x1000u);   // next fine Y row
        return;
    }

    v_ = static_cast<u16>(v_ & static_cast<u16>(~0x7000u));
    u16 coarse_y = static_cast<u16>((v_ & 0x03E0u) >> 5);

    if (coarse_y == 29) {
        coarse_y = 0;
        v_ = static_cast<u16>(v_ ^ 0x0800u);   // switch vertical nametable
    } else if (coarse_y == 31) {
        // Rows 30 and 31 do not exist; wrap without switching nametables.
        coarse_y = 0;
    } else {
        ++coarse_y;
    }

    v_ = static_cast<u16>((v_ & static_cast<u16>(~0x03E0u)) | (coarse_y << 5));
}

void Ppu::copy_x() noexcept
{
    // Everything horizontal, including the nametable bit.
    v_ = static_cast<u16>((v_ & 0xFBE0u) | (t_ & 0x041Fu));
}

void Ppu::copy_y() noexcept
{
    // Everything vertical, including the nametable bit.
    v_ = static_cast<u16>((v_ & 0x841Fu) | (t_ & 0x7BE0u));
}

// ===========================================================================
// Drawing one pixel
// ===========================================================================

void Ppu::render_pixel() noexcept
{
    const int x = dot_ - 1;
    const int y = scanline_;
    if (x < 0 || x >= Framebuffer::kWidth || y < 0 || y >= Framebuffer::kHeight) {
        return;
    }

    // -- background ---------------------------------------------------------
    u8 bg_colour = 0;
    u8 bg_palette = 0;

    const bool bg_on = (mask_ & 0x08u) != 0;
    const bool bg_in_left_column = (mask_ & 0x02u) != 0;

    if (bg_on && (x >= 8 || bg_in_left_column)) {
        const int bit = 15 - fine_x_;
        const u8 lo = static_cast<u8>((shifter_lo_ >> bit) & 1u);
        const u8 hi = static_cast<u8>((shifter_hi_ >> bit) & 1u);
        bg_colour = static_cast<u8>((hi << 1) | lo);

        const u8 alo = static_cast<u8>((attr_lo_ >> bit) & 1u);
        const u8 ahi = static_cast<u8>((attr_hi_ >> bit) & 1u);
        bg_palette = static_cast<u8>((ahi << 1) | alo);
    }

    // -- sprites ------------------------------------------------------------
    u8 fg_colour = 0;
    u8 fg_palette = 0;
    bool fg_in_front = false;
    bool fg_is_zero = false;

    const bool sprites_on = (mask_ & 0x10u) != 0;
    const bool sprites_in_left_column = (mask_ & 0x04u) != 0;

    if (sprites_on && (x >= 8 || sprites_in_left_column)) {
        for (int i = 0; i < sprite_count_; ++i) {
            const SpriteLine& s = sprite_line_[static_cast<std::size_t>(i)];
            const int offset = x - static_cast<int>(s.x);
            if (offset < 0 || offset > 7) {
                continue;
            }

            const int bit = 7 - offset;
            const u8 lo = static_cast<u8>((s.pattern_lo >> bit) & 1u);
            const u8 hi = static_cast<u8>((s.pattern_hi >> bit) & 1u);
            const u8 colour = static_cast<u8>((hi << 1) | lo);
            if (colour == 0) {
                continue;   // transparent: try the next sprite
            }

            fg_colour = colour;
            fg_palette = static_cast<u8>(s.attr & 0x03u);
            fg_in_front = (s.attr & 0x20u) == 0;   // bit 5 set means BEHIND
            fg_is_zero = s.is_sprite_zero;
            break;
        }
    }

    // -- sprite zero hit ----------------------------------------------------
    //
    // Sprite 0 hit is the only way software can find out where the PPU is
    // mid-frame, so it is how a status bar stays fixed while the world
    // scrolls. It fires when an opaque sprite 0 pixel overlaps an opaque
    // background pixel.
    if (fg_is_zero && sprite_zero_in_range_ && bg_colour != 0 && fg_colour != 0 &&
        bg_on && sprites_on && x != 255) {
        sprite_zero_hit_ = true;
        status_ = static_cast<u8>(status_ | 0x40u);
    }

    // -- combine ------------------------------------------------------------
    u8 palette_addr = 0;
    if (fg_colour != 0 && (bg_colour == 0 || fg_in_front)) {
        palette_addr = static_cast<u8>(0x10u + (fg_palette << 2) + fg_colour);
    } else if (bg_colour != 0) {
        palette_addr = static_cast<u8>((bg_palette << 2) + bg_colour);
    }
    // Otherwise both are zero and the universal background colour at $3F00
    // is used, which palette_addr already is.

    const u8 index = palette_ram_[palette_addr & 0x1Fu];
    framebuffer_.set(x, y, colour(index, (mask_ & 0x01u) != 0));
}

// ===========================================================================
// Sprites
// ===========================================================================

void Ppu::evaluate_sprites(int line) noexcept
{
    const int height = ((ctrl_ & 0x20u) != 0) ? 16 : 8;
    sprite_count_ = 0;
    sprite_zero_in_range_ = false;

    for (int i = 0; i < 64; ++i) {
        const u8 sprite_y = oam_[static_cast<std::size_t>(i) * 4 + 0];

        // OAM's Y is the scanline ABOVE the sprite: a sprite with Y = 0
        // shows its first row on scanline 1. So the row drawn on `line` is
        // line - Y - 1, and the sprite covers Y+1 .. Y+height.
        const int row = line - static_cast<int>(sprite_y) - 1;
        if (row < 0 || row >= height) {
            continue;
        }

        if (sprite_count_ >= 8) {
            // The hardware only draws eight per scanline, but it keeps looking
            // for a ninth and sets a flag when it finds one.
            sprite_overflow_ = true;
            status_ = static_cast<u8>(status_ | 0x20u);
            break;
        }

        SpriteLine s{};
        s.x = oam_[static_cast<std::size_t>(i) * 4 + 3];
        s.attr = oam_[static_cast<std::size_t>(i) * 4 + 2];
        s.is_sprite_zero = (i == 0);

        const u8 id = oam_[static_cast<std::size_t>(i) * 4 + 1];

        u16 table = 0;
        u8 tile = 0;
        int fine_y = 0;

        if (height == 16) {
            // 8x16 sprites pick their table from bit 0 of the tile id, and
            // the two halves are consecutive tile numbers.
            int r = row;
            if ((s.attr & 0x80u) != 0) {
                r = 15 - r;   // vertical flip swaps the halves too
            }
            table = ((id & 0x01u) != 0) ? 0x1000u : 0x0000u;
            tile = static_cast<u8>((id & 0xFEu) + (r / 8));
            fine_y = r & 7;
        } else {
            int r = row;
            if ((s.attr & 0x80u) != 0) {
                r = 7 - r;
            }
            table = ((ctrl_ & 0x08u) != 0) ? 0x1000u : 0x0000u;
            tile = id;
            fine_y = r & 7;
        }

        const u16 address = static_cast<u16>(table + tile * 16u + static_cast<u16>(fine_y));
        u8 lo = read_vram(address);
        u8 hi = read_vram(static_cast<u16>(address + 8u));

        if ((s.attr & 0x40u) != 0) {
            // Horizontal flip is cheaper done once here than per pixel.
            lo = reverse_bits(lo);
            hi = reverse_bits(hi);
        }

        s.pattern_lo = lo;
        s.pattern_hi = hi;

        sprite_line_[static_cast<std::size_t>(sprite_count_)] = s;
        if (s.is_sprite_zero) {
            sprite_zero_in_range_ = true;
        }
        ++sprite_count_;
    }

    // The PPU performs eight sprite pattern fetches on every scanline, even
    // when fewer than eight sprites are on it, filling the empty slots with
    // whatever is left in OAM. Those fetches are not decoration: a mapper
    // watching A12 (the MMC3's scanline counter, most famously) is clocked by
    // exactly one of them per line. Skipping the empty slots would leave A12
    // low all the way through a quiet scanline, the counter would never
    // reach zero, and IRQ-driven splits - Super Mario Bros. 3's status bar -
    // would never fire. So read and discard them.
    for (int i = sprite_count_; i < 8; ++i) {
        u16 table = 0;
        if (height == 16) {
            const u8 id = oam_[static_cast<std::size_t>(i & 63) * 4u + 1u];
            table = ((id & 0x01u) != 0) ? 0x1000u : 0x0000u;
        } else {
            table = ((ctrl_ & 0x08u) != 0) ? 0x1000u : 0x0000u;
        }
        (void)read_vram(table);
        (void)read_vram(static_cast<u16>(table + 8u));
    }
}

} // namespace fc::nes
