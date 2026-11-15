//! Per-batch instance emission: glyph placements to `RasterQuad`s. A hit copies
//! origin-relative templates from the [`EncodedCache`](cache::EncodedCache); a
//! miss extracts through the shaper and fills it. Short runs (y-culled lines,
//! atlas-starved glyphs) are not cached, as the key records neither bounds nor
//! occupancy. Each encoded glyph records its raster's pool handle and re-checks it
//! on emit, since eviction hands its rectangle to another raster.

use crate::primitives::math::num::F32Px;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::render_buffer::text_draw_row::TextDrawRow;
use crate::text::key::TextShapeKey;
use crate::text::render::SubpixelOrigin;
use glam::{IVec2, Vec2};

pub(super) mod cache;
pub(super) mod encoder;

/// Cache-hit identity for an encoded run: what places its glyphs. The color is not in it, as a
/// template holds no color: each replay paints the run's own, so a color transition replays one
/// template. That holds while every run is shaped in one color (no per-glyph `color_opt`), which
/// `extract_glyphs` asserts, and no glyph's raster key reads a color, as cosmic's `CacheKey`
/// does not.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(crate) struct EncodedKey {
    text: TextShapeKey,
    /// `(scale * 65536).round() as u32`.
    scale_q: u32,
    /// Packed subpixel bins, from [`crate::text::render::SubpixelOrigin::bins`].
    bins: u8,
}

/// [`Self::for_row`]'s result: the key, the integer-pixel origin and the run's color.
#[derive(Clone, Copy, Debug)]
pub(super) struct EncodedRunKey {
    key: EncodedKey,
    pixel: IVec2,
    /// The color every glyph quad of the run takes, the miss's and the replay's alike.
    color: RgbaF16,
}

impl EncodedRunKey {
    /// The run's color, which every glyph quad of it takes, the miss's and the replay's alike.
    pub(super) const fn color(&self) -> RgbaF16 {
        self.color
    }

    /// The integer pixel plus each axis's subpixel bin; extracting from this,
    /// not the exact origin, gives every origin in a bin the template its key caches.
    pub(super) fn origin(&self) -> Vec2 {
        let bin = |bits: u8| f32::from(bits & 0b11) * 0.25;
        self.pixel.as_vec2() + Vec2::new(bin(self.key.bins >> 2), bin(self.key.bins))
    }

    /// The key for `row` at `frame_scale * row.scale`, plus its integer-pixel origin.
    #[expect(clippy::cast_sign_loss, reason = "a raster scale is positive")]
    pub(super) fn for_row(row: &TextDrawRow, frame_scale: f32) -> Self {
        let scale = frame_scale * row.scale;
        let sub = SubpixelOrigin::of(row.origin);
        Self {
            key: EncodedKey {
                text: row.text.key,
                scale_q: (scale * 65536.0).fast_round() as u32,
                bins: sub.bins,
            },
            pixel: sub.pixel,
            color: row.color,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::common::span::Span;
    use crate::gpu::raster::text_backend::encode::EncodedRunKey;
    use crate::primitives::geometry::urect::URect;
    use crate::primitives::paint::color::RgbaF32;
    use crate::renderer::render_buffer::clip_chain::ClipChain;
    use crate::renderer::render_buffer::text_draw_row::{TextClip, TextDrawRow};
    use crate::text::key::TextShapeKey;
    use crate::text::shaped_ref::ShapedTextRef;
    use glam::Vec2;

    fn key_at(x: f32) -> EncodedRunKey {
        key_of(x, RgbaF32::WHITE)
    }

    fn key_of(x: f32, color: RgbaF32) -> EncodedRunKey {
        let row = TextDrawRow {
            text: ShapedTextRef {
                key: TextShapeKey::fixture(),
                span: Span::default(),
            },
            origin: Vec2::new(x, 3.0),
            bounds: URect::new(0, 0, 100, 100),
            color: color.to_f16(),
            scale: 1.0,
            spin: None,
            clip: ClipChain::NONE,
            reach: TextClip::Whole,
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

    /// A run's color is no part of its template's key, so a color transition replays one
    /// template, and each replay paints the run's own color.
    #[test]
    fn a_runs_color_keys_no_template() {
        let (white, black) = (key_of(10.0, RgbaF32::WHITE), key_of(10.0, RgbaF32::BLACK));
        assert_eq!(white.key, black.key);
        assert_eq!(white.color(), RgbaF32::WHITE.to_f16());
        assert_eq!(black.color(), RgbaF32::BLACK.to_f16());
    }
}
