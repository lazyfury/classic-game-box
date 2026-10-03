//! Offline decoders that turn raw core memory into pictures.
//!
//! The resource inspector reads live buffers out of a running core (see
//! [`crate::session::Session::memory_regions`]) and hands the bytes to one of
//! these decoder modules. Each decoder is a pure function from bytes to an
//! [`InspectImage`], so it can be tested without a core, a GPU or a window.
//!
//! These are *viewers*, not emulators: they decode the hardware's own storage
//! formats (pattern tables, palette RAM, nametables, OAM) so a person can see
//! and study the art. They never advance a machine.

pub mod gba;
pub mod hex;
pub mod nes;

/// An RGBA8 image produced by a decoder, in the same layout the texture backend
/// wants (`width * height * 4` bytes, straight alpha).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InspectImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl InspectImage {
    /// A fully opaque black image.
    pub fn new(width: u32, height: u32) -> Self {
        let mut rgba = vec![0u8; width as usize * height as usize * 4];
        for pixel in rgba.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        Self {
            width,
            height,
            rgba,
        }
    }

    /// Set one pixel. Out-of-range coordinates are ignored, so decoders can
    /// draw without bounds checks of their own.
    pub fn set(&mut self, x: u32, y: u32, rgb: [u8; 3]) {
        if x >= self.width || y >= self.height {
            return;
        }
        let index = (y as usize * self.width as usize + x as usize) * 4;
        self.rgba[index] = rgb[0];
        self.rgba[index + 1] = rgb[1];
        self.rgba[index + 2] = rgb[2];
        self.rgba[index + 3] = 255;
    }

    /// Fill an axis-aligned rectangle. Clipped to the image.
    pub fn fill_rect(&mut self, x: u32, y: u32, width: u32, height: u32, rgb: [u8; 3]) {
        for yy in y..y.saturating_add(height).min(self.height) {
            for xx in x..x.saturating_add(width).min(self.width) {
                self.set(xx, yy, rgb);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_image_is_opaque_black() {
        let image = InspectImage::new(2, 2);
        assert_eq!(
            image.rgba,
            vec![0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255]
        );
    }

    #[test]
    fn writes_are_bounds_checked() {
        let mut image = InspectImage::new(2, 2);
        image.set(5, 5, [1, 2, 3]);
        image.fill_rect(1, 1, 100, 100, [4, 5, 6]);
        assert_eq!(&image.rgba[0..4], &[0, 0, 0, 255]);
        assert_eq!(&image.rgba[12..16], &[4, 5, 6, 255]);
    }
}
