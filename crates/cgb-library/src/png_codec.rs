//! PNG encode/decode for screenshots, in the one place that owns the files.
//!
//! Screenshots are written by the app from a frame's RGBA8 pixels and read
//! back as a card cover. Both directions stay here so `cgb-app` never touches
//! the image format, and only RGBA8 is handled — the app is the only producer.

use crate::error::LibraryError;

/// Encode RGBA8 pixels as a PNG.
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, LibraryError> {
    if width == 0 || height == 0 {
        return Err(LibraryError::Image("zero width or height".to_string()));
    }
    let expected = width as usize * height as usize * 4;
    if rgba.len() < expected {
        return Err(LibraryError::Image(format!(
            "expected {expected} bytes, got {}",
            rgba.len()
        )));
    }
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(image_error)?;
        writer
            .write_image_data(&rgba[..expected])
            .map_err(image_error)?;
    }
    Ok(out)
}

/// Decode a PNG back into RGBA8 pixels.
pub fn decode_png(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), LibraryError> {
    let decoder = png::Decoder::new(bytes);
    let mut reader = decoder.read_info().map_err(image_error)?;
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buffer).map_err(image_error)?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Err(LibraryError::Image(format!(
            "unsupported png: {:?}/{:?}",
            info.color_type, info.bit_depth
        )));
    }
    buffer.truncate(info.buffer_size());
    Ok((info.width, info.height, buffer))
}

fn image_error(error: impl std::fmt::Display) -> LibraryError {
    LibraryError::Image(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_round_trips_through_png() {
        let (width, height) = (4u32, 3u32);
        let mut rgba = vec![0u8; width as usize * height as usize * 4];
        for (index, byte) in rgba.iter_mut().enumerate() {
            *byte = (index % 251) as u8;
        }
        let png = encode_png(width, height, &rgba).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "a real PNG signature");
        let (w, h, decoded) = decode_png(&png).unwrap();
        assert_eq!((w, h), (width, height));
        assert_eq!(decoded, rgba);
    }

    #[test]
    fn a_short_buffer_is_rejected() {
        assert!(encode_png(2, 2, &[0, 0, 0, 0]).is_err());
    }
}
