//! Per-batch instance emission: glyph placements to `RasterQuad`s. A hit copies
//! origin-relative templates from the [`EncodedCache`](cache::EncodedCache); a
//! miss extracts through the shaper and fills it. Short runs (y-culled lines,
//! atlas-starved glyphs) are not cached, as the key records neither bounds nor
//! occupancy. Each encoded glyph records its slot's generation and re-checks it
//! on emit, since eviction reuses slot rectangles.

use crate::primitives::math::num::F32Px;
use crate::renderer::render_buffer::text::TextDrawRow;
use crate::text::key::TextShapeKey;
use crate::text::render::SubpixelOrigin;
use glam::Vec2;

pub(super) mod cache;
pub(super) mod encoder;

/// Cache-hit identity for an encoded run. `area_color` is in the key because
/// the colour is baked into every cached quad, which holds only while every
/// run is shaped with one uniform colour (no per-glyph `color_opt`); add a
/// colour-span fingerprint first if that changes. `extract_glyphs` asserts it.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(crate) struct EncodedKey {
    text: TextShapeKey,
    /// `(scale * 65536).round() as u32`.
    scale_q: u32,
    /// The run's [`RgbaF16`](crate::primitives::paint::color::rgba_f16::RgbaF16) colour bits.
    area_color: u64,
    /// Packed subpixel bins, from [`crate::text::render::SubpixelOrigin::bins`].
    bins: u8,
}

/// [`Self::for_row`]'s result: the key plus the integer-pixel origin.
#[derive(Clone, Copy, Debug)]
pub(super) struct EncodedRunKey {
    key: EncodedKey,
    origin_x: i32,
    origin_y: i32,
}

impl EncodedRunKey {
    /// The integer pixel plus each axis's subpixel bin; extracting from this,
    /// not the exact origin, gives every origin in a bin the template its key caches.
    pub(super) fn origin(&self) -> Vec2 {
        let bin = |bits: u8| f32::from(bits & 0b11) * 0.25;
        Vec2::new(
            self.origin_x as f32 + bin(self.key.bins >> 2),
            self.origin_y as f32 + bin(self.key.bins),
        )
    }

    /// The key for `row` at `frame_scale * row.scale`, plus its integer-pixel origin.
    #[expect(clippy::cast_sign_loss, reason = "a raster scale is positive")]
    pub(super) fn for_row(row: &TextDrawRow, frame_scale: f32) -> Self {
        let scale = frame_scale * row.scale;
        let area_color = row.color.as_u64();
        let sub = SubpixelOrigin::of(row.origin);
        Self {
            key: EncodedKey {
                text: row.text.key,
                scale_q: (scale * 65536.0).fast_round() as u32,
                area_color,
                bins: sub.bins,
            },
            origin_x: sub.x,
            origin_y: sub.y,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::common::span::Span;
    use crate::gpu::raster::text_backend::encode::EncodedRunKey;
    use crate::primitives::geometry::urect::URect;
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::paint::color::rgba_f16::RgbaF16;
    use crate::renderer::render_buffer::text::TextDrawRow;
    use crate::text::key::TextShapeKey;
    use crate::text::shaped_ref::ShapedTextRef;
    use glam::Vec2;

    fn key_at(x: f32) -> EncodedRunKey {
        let row = TextDrawRow {
            text: ShapedTextRef {
                key: TextShapeKey::fixture(),
                span: Span::default(),
            },
            origin: Vec2::new(x, 3.0),
            bounds: URect::new(0, 0, 100, 100),
            color: RgbaF16::from(RgbaF32::WHITE),
            scale: 1.0,
        };
        EncodedRunKey::for_row(&row, 1.0)
    }

    /// Every origin in one quarter-pixel bin extracts from the same point; a
    /// negative origin bins below its integer (-0.2 is -0.25).
    #[test]
    fn an_origin_extracts_at_the_bin_its_key_stands_for() {
        for (raw, snapped) in [
            (10.13, 10.25),
            (10.37, 10.25),
            (10.1, 10.0),
            (10.9, 11.0),
            (-0.2, -0.25),
        ] {
            assert_eq!(key_at(raw).origin(), Vec2::new(snapped, 3.0), "{raw}");
        }
        assert_eq!(key_at(10.13).key, key_at(10.37).key, "one bin, one key");
    }
}
