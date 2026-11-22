//! PNG to and from straight sRGB RGBA8, through the `png` crate: an [`Image`](super::Image)'s file
//! and a color glyph's embedded bitmap. Refused: the `image` crate, a decoder of every format
//! around the one codec palantir reads.

use glam::UVec2;
#[cfg(feature = "png")]
use png::Encoder;
use png::{BitDepth, ColorType, Decoder, Transformations};
use std::io::Cursor;
#[cfg(feature = "png")]
use std::io::Write;

/// Decode the PNG in `bytes` into `out`, which it clears first, as straight RGBA8, a gray image's
/// gray in each channel and an image without alpha opaque, sixteen bits a channel cut to their
/// high byte; its size.
pub(crate) fn decode(bytes: &[u8], out: &mut Vec<u8>) -> Result<UVec2, png::DecodingError> {
    out.clear();
    let mut decoder = Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(
        Transformations::EXPAND | Transformations::STRIP_16 | Transformations::ALPHA,
    );
    let mut reader = decoder.read_info()?;
    let len = reader
        .output_buffer_size()
        .ok_or(png::DecodingError::LimitsExceeded)?;
    out.resize(len, 0);
    let frame = reader.next_frame(out)?;
    out.truncate(frame.buffer_size());
    let size = UVec2::new(frame.width, frame.height);
    match frame.color_type {
        ColorType::Rgba => {}
        ColorType::GrayscaleAlpha => {
            // Widened in place from the back, so no pixel is read after it is overwritten.
            let pixels = out.len() / 2;
            out.resize(pixels * 4, 0);
            for at in (0..pixels).rev() {
                let (gray, alpha) = (out[at * 2], out[at * 2 + 1]);
                out[at * 4..at * 4 + 4].copy_from_slice(&[gray, gray, gray, alpha]);
            }
        }
        // `ALPHA` and `EXPAND` leave an 8-bit image with an alpha channel.
        ColorType::Rgb | ColorType::Grayscale | ColorType::Indexed => {
            unreachable!("the transformations add alpha to {:?}", frame.color_type)
        }
    }
    debug_assert_eq!(frame.bit_depth, BitDepth::Eight);
    debug_assert_eq!(out.len(), size.x as usize * size.y as usize * 4);
    Ok(size)
}

/// Write `pixels`, straight RGBA8 of `size`, to `out` as an 8-bit RGBA PNG.
#[cfg(feature = "png")]
pub(crate) fn encode(
    pixels: &[u8],
    size: UVec2,
    out: impl Write,
) -> Result<(), png::EncodingError> {
    let mut encoder = Encoder::new(out, size.x, size.y);
    encoder.set_color(ColorType::Rgba);
    encoder.set_depth(BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(pixels)?;
    writer.finish()
}

#[cfg(test)]
mod tests {
    use crate::primitives::paint::image::png_rgba;
    use glam::UVec2;
    use png::{BitDepth, ColorType, Encoder};

    /// `pixels` of `color` and `depth`, 2×1, as a PNG.
    fn png_of(color: ColorType, depth: BitDepth, pixels: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut encoder = Encoder::new(&mut bytes, 2, 1);
        encoder.set_color(color);
        encoder.set_depth(depth);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(pixels).unwrap();
        writer.finish().unwrap();
        bytes
    }

    /// Every color type decodes to straight RGBA8: RGBA as it is, RGB opaque, gray and gray with
    /// alpha into each channel, sixteen-bit channels to their high byte; bytes that are no PNG are
    /// refused; RGBA encodes and decodes back to its bytes.
    #[test]
    fn every_color_type_decodes_to_rgba8() {
        let mut out = Vec::new();
        let decoded = |png: Vec<u8>, out: &mut Vec<u8>| {
            assert_eq!(png_rgba::decode(&png, out).unwrap(), UVec2::new(2, 1));
            out.clone()
        };
        let rgba = [1, 2, 3, 4, 5, 6, 7, 8];
        assert_eq!(
            decoded(png_of(ColorType::Rgba, BitDepth::Eight, &rgba), &mut out),
            rgba
        );
        assert_eq!(
            decoded(
                png_of(ColorType::Rgb, BitDepth::Eight, &[1, 2, 3, 4, 5, 6]),
                &mut out
            ),
            [1, 2, 3, 255, 4, 5, 6, 255]
        );
        assert_eq!(
            decoded(
                png_of(ColorType::Grayscale, BitDepth::Eight, &[9, 200]),
                &mut out
            ),
            [9, 9, 9, 255, 200, 200, 200, 255]
        );
        assert_eq!(
            decoded(
                png_of(
                    ColorType::GrayscaleAlpha,
                    BitDepth::Eight,
                    &[9, 10, 200, 20]
                ),
                &mut out
            ),
            [9, 9, 9, 10, 200, 200, 200, 20]
        );
        // 0x1234 and 0xABCD keep 0x12 and 0xAB.
        assert_eq!(
            decoded(
                png_of(
                    ColorType::Grayscale,
                    BitDepth::Sixteen,
                    &[0x12, 0x34, 0xAB, 0xCD]
                ),
                &mut out
            ),
            [0x12, 0x12, 0x12, 255, 0xAB, 0xAB, 0xAB, 255]
        );

        assert!(png_rgba::decode(b"not a png", &mut out).is_err());
        #[cfg(feature = "png")]
        {
            let mut bytes = Vec::new();
            png_rgba::encode(&rgba, UVec2::new(2, 1), &mut bytes).unwrap();
            assert_eq!(decoded(bytes, &mut out), rgba, "an encode decodes back");
        }
    }
}
