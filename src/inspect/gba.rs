//! GBA (LCD) resource decoders.
//!
//! The GBA's picture comes from three buffers the CPU fills directly: VRAM
//! (tiles and tilemaps), palette RAM (256 BGR555 colours) and OAM (128
//! objects). Which VRAM is tiles and which is a tilemap is decided by the
//! display registers in I/O, so the decoders take `DISPCNT` / `BG0CNT` too.
//!
//! Colours are 15-bit BGR555, expanded to 8-bit channels.

use super::InspectImage;

/// GBA VRAM size (96 KB).
pub const VRAM_SIZE: usize = 0x18000;
/// Palette RAM size (512 bytes = 256 colours).
pub const PALETTE_SIZE: usize = 0x400;
/// OAM size (1 KB = 128 objects).
pub const OAM_SIZE: usize = 0x400;
/// The object (sprite) tile base, relative to the VRAM start.
const OBJ_CHAR_BASE: usize = 0x10000;
/// The most tiles a sheet draws.
pub const MAX_TILES: usize = 1024;

/// Read a little-endian `u16`; an out-of-range read is 0.
fn u16_at(data: &[u8], offset: usize) -> u16 {
    let lo = data.get(offset).copied().unwrap_or(0) as u16;
    let hi = data.get(offset + 1).copied().unwrap_or(0) as u16;
    lo | (hi << 8)
}

/// Expand a 5-bit channel to 8 bits.
fn expand5(value: u16) -> u8 {
    let value = value as u8 & 0x1F;
    (value << 3) | (value >> 2)
}

/// A BGR555 palette word as RGB.
pub fn rgb555(word: u16) -> [u8; 3] {
    [expand5(word), expand5(word >> 5), expand5(word >> 10)]
}

/// Palette RAM entry `index` (masked to 256) as RGB.
fn palette_color(palette: &[u8], index: usize) -> [u8; 3] {
    rgb555(u16_at(palette, (index & 0xFF) * 2))
}

/// The shared backdrop colour (palette entry 0).
fn backdrop(palette: &[u8]) -> [u8; 3] {
    palette_color(palette, 0)
}

/// One 8x8 tile's colour indices from `vram` at `offset`.
///
/// 4bpp tiles are 32 bytes with two pixels per byte (low nibble left);
/// 8bpp tiles are 64 bytes with one byte per pixel.
fn tile(vram: &[u8], offset: usize, eight_bpp: bool) -> [[u8; 8]; 8] {
    let mut out = [[0u8; 8]; 8];
    if eight_bpp {
        for (y, row) in out.iter_mut().enumerate() {
            for (x, pixel) in row.iter_mut().enumerate() {
                *pixel = vram.get(offset + y * 8 + x).copied().unwrap_or(0);
            }
        }
    } else {
        for (y, row) in out.iter_mut().enumerate() {
            for (x, pixel) in row.iter_mut().enumerate() {
                let byte = vram.get(offset + y * 4 + x / 2).copied().unwrap_or(0);
                *pixel = if x % 2 == 0 { byte & 0x0F } else { byte >> 4 };
            }
        }
    }
    out
}

/// A sheet of the VRAM tiles at `char_base`, `columns` tiles wide, coloured
/// with palette bank 0 (or the full 256 colours when `eight_bpp`).
pub fn tile_sheet(
    vram: &[u8],
    palette: &[u8],
    char_base: usize,
    eight_bpp: bool,
    columns: u32,
) -> InspectImage {
    let columns = columns.max(1);
    let tile_size = if eight_bpp { 64 } else { 32 };
    let count = (vram.len().saturating_sub(char_base) / tile_size).min(MAX_TILES);
    let rows = (count as u32).div_ceil(columns).max(1);
    let mut image = InspectImage::new(columns * 8, rows * 8);
    let back = backdrop(palette);
    for index in 0..count {
        let cx = (index as u32 % columns) * 8;
        let cy = (index as u32 / columns) * 8;
        let pixels = tile(vram, char_base + index * tile_size, eight_bpp);
        for (y, row) in pixels.iter().enumerate() {
            for (x, &p) in row.iter().enumerate() {
                let p = p as usize;
                let rgb = if p == 0 {
                    back
                } else {
                    palette_color(palette, p)
                };
                image.set(cx + x as u32, cy + y as u32, rgb);
            }
        }
    }
    image
}

/// Palette RAM as a 16x16 sheet of swatches, `swatch` pixels square.
pub fn palette_sheet(palette: &[u8], swatch: u32) -> InspectImage {
    let swatch = swatch.max(1);
    let mut image = InspectImage::new(16 * swatch, 16 * swatch);
    for i in 0..256usize {
        let x = (i as u32 % 16) * swatch;
        let y = (i as u32 / 16) * swatch;
        image.fill_rect(x, y, swatch, swatch, palette_color(palette, i));
    }
    image
}

/// The tile dimensions of a text background, from `BGxCNT` size bits.
fn text_size(bgcnt: u16) -> (u32, u32) {
    match (bgcnt >> 14) & 3 {
        0 => (32, 32),
        1 => (64, 32),
        2 => (32, 64),
        _ => (64, 64),
    }
}

/// Render a text background (BG0–BG3 in modes 0/1) as its full tilemap.
pub fn text_background(vram: &[u8], palette: &[u8], bgcnt: u16) -> InspectImage {
    let char_base = ((bgcnt >> 2) & 3) as usize * 0x4000;
    let eight_bpp = bgcnt & 0x80 != 0;
    let screen_base = ((bgcnt >> 8) & 0x1F) as usize * 0x800;
    let tile_size = if eight_bpp { 64 } else { 32 };
    let (tiles_w, tiles_h) = text_size(bgcnt);
    let mut image = InspectImage::new(tiles_w * 8, tiles_h * 8);
    let back = backdrop(palette);
    for ty in 0..tiles_h {
        for tx in 0..tiles_w {
            let entry = u16_at(vram, screen_base + ((ty * tiles_w + tx) as usize) * 2);
            let index = (entry & 0x03FF) as usize;
            let hflip = entry & 0x0400 != 0;
            let vflip = entry & 0x0800 != 0;
            let bank = ((entry >> 12) & 0xF) as usize;
            let pixels = tile(vram, char_base + index * tile_size, eight_bpp);
            for y in 0..8u32 {
                for x in 0..8u32 {
                    let sx = if hflip { 7 - x } else { x };
                    let sy = if vflip { 7 - y } else { y };
                    let p = pixels[sy as usize][sx as usize] as usize;
                    let color_index = if eight_bpp { p } else { bank * 16 + p };
                    let rgb = if color_index == 0 {
                        back
                    } else {
                        palette_color(palette, color_index)
                    };
                    image.set(tx * 8 + x, ty * 8 + y, rgb);
                }
            }
        }
    }
    image
}

/// The pixel size of an object, indexed `shape * 4 + size`.
const OBJ_SIZES: [(u32, u32); 16] = [
    (8, 8),
    (16, 16),
    (32, 32),
    (64, 64),
    (16, 8),
    (32, 8),
    (32, 16),
    (64, 32),
    (8, 16),
    (8, 32),
    (16, 32),
    (32, 64),
    (8, 8),
    (8, 8),
    (8, 8),
    (8, 8),
];

/// The pixel size of object `index` in OAM.
pub fn obj_size(oam: &[u8], index: usize) -> (u32, u32) {
    let attr0 = u16_at(oam, index * 8);
    let attr1 = u16_at(oam, index * 8 + 2);
    let shape = ((attr0 >> 14) & 3) as usize;
    let size = ((attr1 >> 14) & 3) as usize;
    OBJ_SIZES[shape * 4 + size]
}

/// The most objects the sprite view lays out.
pub const MAX_SPRITES: usize = 128;

/// The widest the packed sprite sheet is allowed to get, in pixels.
const SPRITE_SHEET_WIDTH: u32 = 512;

/// Render the objects in OAM, packed into rows (a sprite sheet). Colour 0 is
/// transparent, left black. `obj_1d` is `DISPCNT`'s object character mapping:
/// the common 1D layout, else the 2D one.
pub fn sprites(vram: &[u8], palette: &[u8], oam: &[u8], obj_1d: bool) -> InspectImage {
    let count = (oam.len() / 8).min(MAX_SPRITES);

    // First pass: place every object, wrapping a new row when it would run
    // past the sheet width.
    let mut placements = Vec::with_capacity(count);
    let (mut x, mut y, mut row_height, mut width) = (0u32, 0u32, 0u32, 0u32);
    for index in 0..count {
        let (w, h) = obj_size(oam, index);
        if x > 0 && x + w > SPRITE_SHEET_WIDTH {
            y += row_height;
            x = 0;
            row_height = 0;
        }
        placements.push((index, x, y, w, h));
        x += w;
        row_height = row_height.max(h);
        width = width.max(x);
    }
    let height = y + row_height;

    let mut image = InspectImage::new(width.max(1), height.max(1));
    for (index, ox, oy, w, h) in placements {
        let attr0 = u16_at(oam, index * 8);
        let attr1 = u16_at(oam, index * 8 + 2);
        let attr2 = u16_at(oam, index * 8 + 4);
        let eight_bpp = attr0 & 0x2000 != 0;
        let hflip = attr1 & 0x1000 != 0;
        let vflip = attr1 & 0x2000 != 0;
        let tile_index = (attr2 & 0x03FF) as usize;
        let bank = ((attr2 >> 12) & 0xF) as usize;
        let cols = w / 8;
        let tile_size = if eight_bpp { 64 } else { 32 };
        for ty in 0..h / 8 {
            for tx in 0..cols {
                let number = if obj_1d {
                    tile_index + (ty * cols + tx) as usize
                } else {
                    tile_index + (ty * 32 + tx) as usize
                };
                let pixels = tile(vram, OBJ_CHAR_BASE + number * tile_size, eight_bpp);
                for py in 0..8u32 {
                    for px in 0..8u32 {
                        let sx = if hflip { 7 - px } else { px };
                        let sy = if vflip { 7 - py } else { py };
                        let p = pixels[sy as usize][sx as usize] as usize;
                        let color_index = if eight_bpp { p } else { bank * 16 + p };
                        if color_index == 0 {
                            continue; // transparent
                        }
                        image.set(
                            ox + tx * 8 + px,
                            oy + ty * 8 + py,
                            palette_color(palette, color_index),
                        );
                    }
                }
            }
        }
    }
    image
}

/// Decode one inspector view, in the order of [`crate::ui::INSPECTOR_VIEWS`]
/// (`0` pattern tables, `1` palette, `2` background, `3` sprites). Returns the
/// image and a one-line caption.
pub fn view(
    index: usize,
    vram: &[u8],
    palette: &[u8],
    oam: &[u8],
    io: &[u8],
) -> (InspectImage, String) {
    let dispcnt = u16_at(io, 0);
    let mode = dispcnt & 7;
    let obj_1d = dispcnt & 0x0040 != 0;
    let bg0cnt = u16_at(io, 8);
    match index {
        0 => {
            let char_base = ((bg0cnt >> 2) & 3) as usize * 0x4000;
            let eight_bpp = bg0cnt & 0x80 != 0;
            let tile_size = if eight_bpp { 64 } else { 32 };
            let count = (vram.len().saturating_sub(char_base) / tile_size).min(MAX_TILES);
            (
                tile_sheet(vram, palette, char_base, eight_bpp, 16),
                format!(
                    "{}bpp · {} 个 tile · mode {mode}",
                    if eight_bpp { 8 } else { 4 },
                    count
                ),
            )
        }
        1 => (palette_sheet(palette, 16), "256 色 RGB555".to_string()),
        2 => {
            let (w, h) = text_size(bg0cnt);
            (
                text_background(vram, palette, bg0cnt),
                format!("BG0 · {w}×{h} tile · mode {mode}"),
            )
        }
        _ => {
            let count = (oam.len() / 8).min(MAX_SPRITES);
            (
                sprites(vram, palette, oam, obj_1d),
                format!("{count} 个精灵"),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb555_expands_each_channel() {
        // Red only, full 5 bits.
        assert_eq!(rgb555(0x001F), [255, 0, 0]);
        // Green only.
        assert_eq!(rgb555(0x03E0), [0, 255, 0]);
        // Blue only.
        assert_eq!(rgb555(0x7C00), [0, 0, 255]);
    }

    #[test]
    fn a_4bpp_tile_has_two_pixels_per_byte() {
        let mut vram = vec![0u8; 32];
        vram[0] = 0x21; // row 0: left pixel 1, right pixel 2
        let pixels = tile(&vram, 0, false);
        assert_eq!(pixels[0][0], 1);
        assert_eq!(pixels[0][1], 2);
        assert_eq!(pixels[1][0], 0);
    }

    #[test]
    fn an_8bpp_tile_is_one_byte_per_pixel() {
        let mut vram = vec![0u8; 64];
        vram[0] = 0xAB; // row 0, x = 0
        let pixels = tile(&vram, 0, true);
        assert_eq!(pixels[0][0], 0xAB);
        assert_eq!(pixels[0][1], 0);
    }

    #[test]
    fn palette_sheet_is_16_by_16() {
        let image = palette_sheet(&[0u8; PALETTE_SIZE], 10);
        assert_eq!((image.width, image.height), (160, 160));
    }

    #[test]
    fn text_background_follows_the_size_bits() {
        // 64x32 tiles.
        let bgcnt = 1u16 << 14;
        let image = text_background(&[0u8; VRAM_SIZE], &[0u8; PALETTE_SIZE], bgcnt);
        assert_eq!((image.width, image.height), (64 * 8, 32 * 8));
    }

    #[test]
    fn object_sizes_come_from_shape_and_size() {
        let mut oam = vec![0u8; OAM_SIZE];
        // Object 0: shape 0 (square), size 1 -> 16x16. Size is attr1 bits 14-15.
        oam[3] = 0x40;
        assert_eq!(obj_size(&oam, 0), (16, 16));
        // Shape 1 (wide), size 0 -> 16x8. Shape is attr0 bits 14-15.
        oam[1] = 0x40;
        oam[3] = 0x00;
        assert_eq!(obj_size(&oam, 0), (16, 8));
    }

    #[test]
    fn view_dispatch_covers_every_inspector_tab() {
        let vram = vec![0u8; VRAM_SIZE];
        let palette = vec![0u8; PALETTE_SIZE];
        let oam = vec![0u8; OAM_SIZE];
        // DISPCNT: 1D object mapping; BG0CNT: 4bpp, char base 0, screen base 0.
        let mut io = vec![0u8; 0x400];
        io[0] = 0x40;
        for index in 0..crate::ui::INSPECTOR_VIEWS.len() {
            let (image, caption) = view(index, &vram, &palette, &oam, &io);
            assert!(image.width > 0 && image.height > 0, "view {index}");
            assert!(!caption.is_empty(), "view {index}");
            assert_eq!(
                image.rgba.len(),
                image.width as usize * image.height as usize * 4,
                "view {index}"
            );
        }
    }
}
