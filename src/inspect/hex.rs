//! A hex-dump view rendered to an image.
//!
//! The UI stack has no monospace font, so a text dump would not line up. This
//! instead draws each hex digit with a tiny 5x7 bitmap font into an
//! [`InspectImage`], which the inspector already knows how to display. One
//! page is [`PAGE_BYTES`] bytes; the caller pages by changing `offset`.

use super::InspectImage;

/// 5x7 glyphs for the hex digits `0..=F`, each row's bit 4 is the leftmost
/// pixel. Index 0 is `0`, 15 is `F`.
const FONT: [[u8; 7]; 16] = [
    [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E], // 0
    [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E], // 1
    [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F], // 2
    [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E], // 3
    [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02], // 4
    [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E], // 5
    [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E], // 6
    [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08], // 7
    [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E], // 8
    [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C], // 9
    [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11], // A
    [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E], // B
    [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E], // C
    [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E], // D
    [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F], // E
    [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10], // F
];

/// Glyph cell width, advance and line height in unscaled pixels.
const GLYPH_W: u32 = 5;
const GLYPH_H: u32 = 7;
const ADVANCE: u32 = GLYPH_W + 1;
const LINE: u32 = GLYPH_H + 3;
/// Pixel scale, so the sheet is readable when the panel contains it.
const SCALE: u32 = 2;

/// Bytes shown per row.
pub const BYTES_PER_ROW: usize = 16;
/// Rows per page.
pub const ROWS: usize = 32;
/// Bytes per page (512).
pub const PAGE_BYTES: usize = BYTES_PER_ROW * ROWS;
/// Hex digits in an offset.
const OFFSET_DIGITS: usize = 6;

/// Draw one hex digit (`0..=F`) or nothing for `None`.
fn draw_char(image: &mut InspectImage, x: u32, y: u32, digit: Option<u8>, color: [u8; 3]) {
    let Some(digit) = digit else {
        return;
    };
    let glyph = FONT[(digit & 0xF) as usize];
    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..GLYPH_W {
            let bit = GLYPH_W - 1 - col;
            if bits & (1 << bit) == 0 {
                continue;
            }
            let px = x + col * SCALE;
            let py = y + row as u32 * SCALE;
            image.fill_rect(px, py, SCALE, SCALE, color);
        }
    }
}

/// Render `data` starting at `offset` as a hex dump of at most [`PAGE_BYTES`].
///
/// The offset shown is absolute (the start of the whole region plus `offset`),
/// so a paged viewer always names the real address.
pub fn dump(data: &[u8], offset: usize) -> InspectImage {
    let available = data.len().saturating_sub(offset);
    let rows = available.div_ceil(BYTES_PER_ROW).clamp(1, ROWS);
    let columns = OFFSET_DIGITS + 2 + BYTES_PER_ROW * 3 - 1;
    let mut image = InspectImage::new(columns as u32 * ADVANCE * SCALE, rows as u32 * LINE * SCALE);
    let color = [0xD8, 0xD8, 0xD8];

    for row in 0..rows {
        let base = offset + row * BYTES_PER_ROW;
        let y = row as u32 * LINE * SCALE;
        let mut cursor = 0u32;
        let mut emit = |digit: Option<u8>, image: &mut InspectImage| {
            draw_char(image, cursor, y, digit, color);
            cursor += ADVANCE * SCALE;
        };
        // The address: high nibble first, then a two-space gutter.
        for shift in (0..OFFSET_DIGITS).rev() {
            emit(Some(((base >> (shift * 4)) & 0xF) as u8), &mut image);
        }
        emit(None, &mut image);
        emit(None, &mut image);
        for i in 0..BYTES_PER_ROW {
            match data.get(base + i) {
                Some(byte) => {
                    emit(Some(byte >> 4), &mut image);
                    emit(Some(byte & 0xF), &mut image);
                }
                None => {
                    emit(None, &mut image);
                    emit(None, &mut image);
                }
            }
            if i + 1 < BYTES_PER_ROW {
                emit(None, &mut image);
            }
        }
    }
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit(image: &InspectImage) -> usize {
        image
            .rgba
            .chunks_exact(4)
            .filter(|pixel| pixel[0] > 0)
            .count()
    }

    #[test]
    fn a_page_has_a_fixed_shape() {
        let image = dump(&[0u8; PAGE_BYTES], 0);
        let columns = OFFSET_DIGITS + 2 + BYTES_PER_ROW * 3 - 1;
        assert_eq!(image.width, columns as u32 * ADVANCE * SCALE);
        assert_eq!(image.height, ROWS as u32 * LINE * SCALE);
    }

    #[test]
    fn a_short_buffer_still_yields_one_row() {
        let image = dump(&[0xAB], 0);
        assert_eq!(image.height, LINE * SCALE);
        assert!(lit(&image) > 0, "the one byte is drawn");
    }

    #[test]
    fn bytes_are_drawn_in_the_byte_area() {
        let zero = dump(&[0x00], 0);
        let full = dump(&[0xFF], 0);
        assert!(lit(&zero) > 0, "the address and zero digits are drawn");
        assert_ne!(zero.rgba, full.rgba, "0x00 and 0xFF render differently");
    }
}
