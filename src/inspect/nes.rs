//! NES (2C02) resource decoders.
//!
//! Everything the PPU draws comes from four small buffers: the pattern tables
//! (CHR), the nametables (which tile where), the attribute table (which of the
//! four palettes each 16x16 block uses) and palette RAM. Decoding them into
//! pictures is how you *see* the art instead of reading hex.
//!
//! The colour table matches the custom core's own
//! (`custom_nes_core/src/core/nes/ppu.cpp`), so a tile drawn here and the same
//! tile on screen use identical RGB.

use super::InspectImage;

/// The 2C02's 64 colours, as RGB (the NESdev measured palette).
pub const PALETTE: [[u8; 3]; 64] = [
    [0x66, 0x66, 0x66],
    [0x00, 0x2A, 0x88],
    [0x14, 0x12, 0xA7],
    [0x3B, 0x00, 0xA4],
    [0x5C, 0x00, 0x7E],
    [0x6E, 0x00, 0x40],
    [0x6C, 0x06, 0x00],
    [0x56, 0x1D, 0x00],
    [0x33, 0x35, 0x00],
    [0x0B, 0x48, 0x00],
    [0x00, 0x52, 0x00],
    [0x00, 0x4F, 0x08],
    [0x00, 0x40, 0x4D],
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0x00],
    [0xAD, 0xAD, 0xAD],
    [0x15, 0x5F, 0xD9],
    [0x42, 0x40, 0xFF],
    [0x75, 0x27, 0xFE],
    [0xA0, 0x1A, 0xCC],
    [0xB7, 0x1E, 0x7B],
    [0xB5, 0x31, 0x20],
    [0x99, 0x4E, 0x00],
    [0x6B, 0x6D, 0x00],
    [0x38, 0x87, 0x00],
    [0x0C, 0x93, 0x00],
    [0x00, 0x8F, 0x32],
    [0x00, 0x7C, 0x8D],
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0x00],
    [0xFF, 0xFE, 0xFF],
    [0x64, 0xB0, 0xFF],
    [0x92, 0x90, 0xFF],
    [0xC6, 0x76, 0xFF],
    [0xF3, 0x6A, 0xFF],
    [0xFE, 0x6E, 0xCC],
    [0xFE, 0x81, 0x70],
    [0xEA, 0x9E, 0x22],
    [0xBC, 0xBE, 0x00],
    [0x88, 0xD8, 0x00],
    [0x5C, 0xE4, 0x30],
    [0x45, 0xE0, 0x82],
    [0x48, 0xCD, 0xDE],
    [0x4F, 0x4F, 0x4F],
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0x00],
    [0xFF, 0xFE, 0xFF],
    [0xC0, 0xDF, 0xFF],
    [0xD3, 0xD2, 0xFF],
    [0xE8, 0xC8, 0xFF],
    [0xFB, 0xC2, 0xFF],
    [0xFE, 0xC4, 0xEA],
    [0xFE, 0xCC, 0xC5],
    [0xF7, 0xD8, 0xA5],
    [0xE4, 0xE5, 0x94],
    [0xCF, 0xEF, 0x96],
    [0xBD, 0xF4, 0xAB],
    [0xB3, 0xF3, 0xCC],
    [0xB5, 0xEB, 0xF2],
    [0xB8, 0xB8, 0xB8],
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0x00],
];

/// The most tiles a pattern sheet draws. A big CHR ROM can be hundreds of KB;
/// the first 16KB (two pattern tables) is the part a person actually studies.
pub const MAX_PATTERN_TILES: usize = 1024;

/// The RGB for a 2C02 palette index.
pub fn color(index: u8) -> [u8; 3] {
    PALETTE[(index & 0x3F) as usize]
}

/// Palette RAM with the shared backdrop applied: colour 0 of *every* palette
/// is the universal background colour at `$3F00`, which is what the PPU
/// actually outputs (and why `$3F10` reads as `$3F00`).
fn palette_entry(ram: &[u8], index: usize) -> u8 {
    let index = index & 0x1F;
    if index % 4 == 0 {
        ram.first().copied().unwrap_or(0)
    } else {
        ram.get(index).copied().unwrap_or(0)
    }
}

/// The four 4-colour background palettes, resolved to RGB.
pub fn background_palettes(ram: &[u8]) -> [[[u8; 3]; 4]; 4] {
    let mut out = [[[0u8; 3]; 4]; 4];
    for (n, palette) in out.iter_mut().enumerate() {
        for (c, slot) in palette.iter_mut().enumerate() {
            *slot = color(palette_entry(ram, n * 4 + c));
        }
    }
    out
}

/// The four 4-colour sprite palettes (colour 0 is transparent).
pub fn sprite_palettes(ram: &[u8]) -> [[[u8; 3]; 4]; 4] {
    let mut out = [[[0u8; 3]; 4]; 4];
    for (n, palette) in out.iter_mut().enumerate() {
        for (c, slot) in palette.iter_mut().enumerate() {
            *slot = color(palette_entry(ram, 0x10 + n * 4 + c));
        }
    }
    out
}

/// Decode one 8x8 tile from `chr` at tile index `tile`: the 2-bit colour index
/// of every pixel, plane-lo first.
fn tile(chr: &[u8], tile: usize) -> [[u8; 8]; 8] {
    let base = tile.saturating_mul(16);
    let mut out = [[0u8; 8]; 8];
    for (y, row) in out.iter_mut().enumerate() {
        let lo = chr.get(base + y).copied().unwrap_or(0) as u16;
        let hi = chr.get(base + 8 + y).copied().unwrap_or(0) as u16;
        for (x, pixel) in row.iter_mut().enumerate() {
            let bit = 7 - x;
            *pixel = (((hi >> bit) & 1) << 1 | ((lo >> bit) & 1)) as u8;
        }
    }
    out
}

/// A sheet of every pattern-table tile in `chr`, `columns` tiles wide, coloured
/// with the 4-colour `palette`.
pub fn pattern_sheet(chr: &[u8], palette: &[[u8; 3]; 4], columns: u32) -> InspectImage {
    let columns = columns.max(1);
    let count = (chr.len() / 16).min(MAX_PATTERN_TILES);
    let rows = (count as u32).div_ceil(columns).max(1);
    let mut image = InspectImage::new(columns * 8, rows * 8);
    for index in 0..count {
        let cx = (index as u32 % columns) * 8;
        let cy = (index as u32 / columns) * 8;
        for (y, row) in tile(chr, index).iter().enumerate() {
            for (x, &c) in row.iter().enumerate() {
                image.set(cx + x as u32, cy + y as u32, palette[(c & 3) as usize]);
            }
        }
    }
    image
}

/// Palette RAM as a 16x2 sheet of swatches, `swatch` pixels square.
pub fn palette_sheet(ram: &[u8], swatch: u32) -> InspectImage {
    let swatch = swatch.max(1);
    let mut image = InspectImage::new(16 * swatch, 2 * swatch);
    for i in 0..32usize {
        let x = (i as u32 % 16) * swatch;
        let y = (i as u32 / 16) * swatch;
        image.fill_rect(
            x,
            y,
            swatch,
            swatch,
            color(ram.get(i).copied().unwrap_or(0)),
        );
    }
    image
}

/// The attribute-table byte that colours the 4x4-tile block containing
/// `(tile_x, tile_y)`.
fn attribute_index(tile_x: u32, tile_y: u32) -> usize {
    ((tile_y / 4) * 8 + (tile_x / 4)) as usize
}

/// Which two bits of that byte name the palette for `(tile_x, tile_y)`.
fn attribute_shift(tile_x: u32, tile_y: u32) -> u32 {
    ((tile_y & 2) << 1) | (tile_x & 2)
}

/// Render one 1KB nametable as 256x240, reading its 32x30 tilemap and the
/// 64-byte attribute table that follows it. Pattern data comes from the first
/// pattern table in `chr`.
pub fn nametable(chr: &[u8], nametable: &[u8], palette_ram: &[u8]) -> InspectImage {
    let palettes = background_palettes(palette_ram);
    let mut image = InspectImage::new(256, 240);
    for tile_y in 0..30u32 {
        for tile_x in 0..32u32 {
            let id = nametable
                .get((tile_y * 32 + tile_x) as usize)
                .copied()
                .unwrap_or(0) as usize;
            let attribute = nametable
                .get(0x3C0 + attribute_index(tile_x, tile_y))
                .copied()
                .unwrap_or(0);
            let palette = ((attribute >> attribute_shift(tile_x, tile_y)) & 3) as usize;
            let pixels = tile(chr, id);
            for (y, row) in pixels.iter().enumerate() {
                for (x, &c) in row.iter().enumerate() {
                    image.set(
                        tile_x * 8 + x as u32,
                        tile_y * 8 + y as u32,
                        palettes[palette][(c & 3) as usize],
                    );
                }
            }
        }
    }
    image
}

/// Render the sprites in OAM as a 16-wide grid of 8x8 cells, honouring each
/// sprite's palette and horizontal / vertical flip. Colour 0 is drawn as
/// `backdrop` (a sprite pixel is transparent on hardware).
pub fn sprites(chr: &[u8], oam: &[u8], palette_ram: &[u8], backdrop: [u8; 3]) -> InspectImage {
    let palettes = sprite_palettes(palette_ram);
    let columns = 16u32;
    let count = (oam.len() / 4).min(64);
    let rows = (count as u32).div_ceil(columns).max(1);
    let mut image = InspectImage::new(columns * 8, rows * 8);
    for index in 0..count {
        let base = index * 4;
        let id = oam.get(base + 1).copied().unwrap_or(0) as usize;
        let attribute = oam.get(base + 2).copied().unwrap_or(0);
        let palette = (attribute & 3) as usize;
        let flip_h = attribute & 0x40 != 0;
        let flip_v = attribute & 0x80 != 0;
        let cx = (index as u32 % columns) * 8;
        let cy = (index as u32 / columns) * 8;
        let pixels = tile(chr, id);
        for y in 0..8u32 {
            for x in 0..8u32 {
                let sx = if flip_h { 7 - x } else { x };
                let sy = if flip_v { 7 - y } else { y };
                let c = pixels[sy as usize][sx as usize] as usize;
                let rgb = if c == 0 {
                    backdrop
                } else {
                    palettes[palette][c]
                };
                image.set(cx + x, cy + y, rgb);
            }
        }
    }
    image
}

/// Decode one inspector view, in the order of [`crate::ui::INSPECTOR_VIEWS`]
/// (`0` pattern tables, `1` palette, `2` background, `3` sprites). Returns the
/// image and a one-line caption.
///
/// Missing buffers are tolerated: a core that publishes no CHR still gets a
/// palette sheet, and so on.
pub fn view(
    index: usize,
    chr: &[u8],
    palette_ram: &[u8],
    nametables: &[u8],
    oam: &[u8],
) -> (InspectImage, String) {
    let backdrop = color(palette_ram.first().copied().unwrap_or(0x0F));
    match index {
        0 => {
            let palettes = background_palettes(palette_ram);
            let image = pattern_sheet(chr, &palettes[0], 16);
            let tiles = (chr.len() / 16).min(MAX_PATTERN_TILES);
            (
                image,
                format!("{tiles} 个 tile · CHR {} KB", chr.len() / 1024),
            )
        }
        1 => (palette_sheet(palette_ram, 24), "32 个调色板项".to_string()),
        2 => {
            let screen = nametables.get(0..0x400).unwrap_or(nametables);
            (
                nametable(chr, screen, palette_ram),
                "第一屏 32×30 tile".to_string(),
            )
        }
        _ => (
            sprites(chr, oam, palette_ram, backdrop),
            format!("{} 个精灵", oam.len() / 4),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tile_packs_two_bitplanes() {
        // Column 0 has both planes set (colour 3); the rest are 0.
        let mut chr = vec![0u8; 16];
        chr[0] = 0x80; // lo plane, y = 0
        chr[8] = 0x80; // hi plane, y = 0
        let pixels = tile(&chr, 0);
        assert_eq!(pixels[0][0], 3);
        assert_eq!(pixels[0][1], 0);
        // Only the top row is set.
        assert_eq!(pixels[1][0], 0);
    }

    #[test]
    fn the_palette_backdrop_mirrors() {
        let mut ram = [0u8; 32];
        ram[0] = 0x0F; // shared backdrop
        ram[0x10] = 0x21; // $3F10 is the shared backdrop (colour 0)
        ram[0x11] = 0x21; // sprite palette 0, colour 1
        ram[1] = 0x21; // background palette 0, colour 1
        assert_eq!(palette_entry(&ram, 0x00), 0x0F);
        assert_eq!(palette_entry(&ram, 0x10), 0x0F);
        assert_eq!(palette_entry(&ram, 0x11), 0x21);
        // Background palette 1's colour 0 is the backdrop too.
        let palettes = background_palettes(&ram);
        assert_eq!(palettes[1][0], color(0x0F));
        assert_eq!(palettes[0][1], color(0x21));
    }

    #[test]
    fn pattern_sheet_dimensions_follow_the_chr_size() {
        let chr = vec![0u8; 16 * 40];
        let image = pattern_sheet(&chr, &background_palettes(&[0u8; 32])[0], 16);
        assert_eq!((image.width, image.height), (16 * 8, 3 * 8));
        assert_eq!(
            image.rgba.len(),
            image.width as usize * image.height as usize * 4
        );
    }

    #[test]
    fn nametable_is_one_screen() {
        let mut nt = vec![0u8; 0x400];
        nt[0] = 1; // top-left tile uses tile 1
        let mut chr = vec![0u8; 16 * 2];
        // Tile 1 is solid colour 1: the low plane set, the high plane clear.
        chr[16..24].fill(0xFF);
        let mut ram = [0u8; 32];
        ram[0] = 0x0F;
        ram[1] = 0x01;
        let image = nametable(&chr, &nt, &ram);
        assert_eq!((image.width, image.height), (256, 240));
        // Pixel (0,0) comes from tile 1, colour 1.
        assert_eq!(&image.rgba[0..3], &color(0x01));
    }

    #[test]
    fn sprites_honour_horizontal_flip() {
        let mut chr = vec![0u8; 16];
        // y = 0 row, lo plane: leftmost pixel set (colour 1).
        chr[0] = 0x80;
        let mut oam = vec![0u8; 4];
        oam[1] = 0; // tile 0
        oam[2] = 0x40; // flip horizontally
        let mut ram = [0u8; 32];
        ram[0] = 0x0F; // backdrop
        ram[0x11] = 0x01; // sprite palette 0, colour 1
        let sprite = sprites(&chr, &oam, &ram, color(0x0F));
        // The set pixel moved from x=0 to x=7.
        assert_eq!(&sprite.rgba[0..3], &color(0x0F));
        let last = 7 * 4;
        assert_eq!(&sprite.rgba[last..last + 3], &color(0x01));
    }

    #[test]
    fn palette_sheet_is_16_by_2() {
        let image = palette_sheet(&[0u8; 32], 10);
        assert_eq!((image.width, image.height), (160, 20));
    }

    #[test]
    fn view_dispatch_covers_every_inspector_tab() {
        let chr = vec![0u8; 16 * 4];
        let palette = [0u8; 32];
        let nametables = vec![0u8; 0x400];
        let oam = vec![0u8; 4];
        let mut sizes = Vec::new();
        for index in 0..crate::ui::INSPECTOR_VIEWS.len() {
            let (image, caption) = view(index, &chr, &palette, &nametables, &oam);
            assert!(image.width > 0 && image.height > 0, "view {index}");
            assert!(!caption.is_empty(), "view {index}");
            assert_eq!(
                image.rgba.len(),
                image.width as usize * image.height as usize * 4,
                "view {index}"
            );
            sizes.push((image.width, image.height));
        }
        assert_eq!(sizes[0], (128, 8)); // 4 tiles, 16 per row
        assert_eq!(sizes[1], (384, 48)); // 16x2 swatches of 24px
        assert_eq!(sizes[2], (256, 240)); // one nametable
        assert_eq!(sizes[3], (128, 8)); // 4 sprite cells, 16 per row
    }
}
