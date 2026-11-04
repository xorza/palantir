//! How far a shaped run's glyphs reach past the block it measured to.

use crate::primitives::geometry::spacing::Spacing;
use crate::text::cosmic::geometry::ShapedGeometry;
use crate::text::cosmic::stem_darkening::StemDarkening;
use crate::text::cosmic::{FAKE_ITALIC_SKEW_DEGREES, Hinting, ScalerSpec};
use crate::text::extent::TextExtent;
use cosmic_text::{Buffer, CacheKeyFlags, FontSystem, LayoutGlyph, fontdb};
use glam::Vec2;
use rustc_hash::FxHashMap;
use std::fmt;
use swash::scale::ScaleContext;
use swash::scale::outline::Outline;

/// Ink bounds of every glyph a run has shaped, read once per glyph and face. Ink can reach past the measured
/// advance block (italic overhang, negative bearing); [`Self::extent`] answers by how much. Stored in ems from
/// the unhinted outline (`attrs_named` sets `DISABLE_HINTING`), so one entry serves every size, and
/// grown by [`StemDarkening::bound`], the most the rasterizer grows it at any size and scale.
///
/// Bounded at [`MAX_INKED_GLYPHS`]: past it the table starts over, and each glyph's ink is read again
/// from its outline as shaping meets it. A glyph's bounds follow from its face alone, so the
/// table is a cache, and starting over answers the same.
#[derive(Default)]
pub(super) struct GlyphInk {
    bounds: FxHashMap<GlyphInkKey, EmBox>,
    /// Swash's per-face scratch, apart from the rasterizer's so shaping can read ink while holding the measurer.
    ctx: ScaleContext,
    outline: Outline,
}

impl fmt::Debug for GlyphInk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GlyphInk")
            .field("bounds", &self.bounds.len())
            .finish_non_exhaustive()
    }
}

/// The glyphs [`GlyphInk`] holds the ink of: a CJK face's twenty thousand at three weights, about
/// 4 MiB.
const MAX_INKED_GLYPHS: usize = 1 << 16;

/// What a glyph's ink depends on beyond size: face, glyph, variable weight, synthetic skew.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct GlyphInkKey {
    font_id: fontdb::ID,
    glyph_id: u16,
    weight: u16,
    fake_italic: bool,
}

/// A glyph's ink in ems, relative to its baseline pen position, y down.
#[derive(Clone, Copy, Debug, Default)]
struct EmBox {
    min: Vec2,
    max: Vec2,
}

impl GlyphInk {
    /// How far glyph ink reaches past `buffer`'s measured block, per side, rounded out to whole pixels.
    pub(super) fn extent(
        &mut self,
        buffer: &Buffer,
        font_system: &mut FontSystem,
        geometry: &ShapedGeometry,
    ) -> TextExtent {
        let ShapedGeometry { size, left, .. } = *geometry;
        let mut min = Vec2::INFINITY;
        let mut max = Vec2::NEG_INFINITY;
        for run in buffer.layout_runs() {
            for glyph in run.glyphs {
                let em = self.em_box(font_system, glyph);
                if em.min.x >= em.max.x || em.min.y >= em.max.y {
                    continue;
                }
                let pen = Vec2::new(
                    glyph.x + glyph.x_offset * glyph.font_size,
                    run.line_y + glyph.y - glyph.y_offset * glyph.font_size,
                );
                min = min.min(pen + em.min * glyph.font_size);
                max = max.max(pen + em.max * glyph.font_size);
            }
        }
        if min.x > max.x {
            return TextExtent::inked_within(size);
        }
        let past = |reach: f32| reach.max(0.0).ceil();
        TextExtent {
            size,
            ink: Spacing::saturating([
                past(left - min.x),
                past(-min.y),
                past(max.x - (left + size.w)),
                past(max.y - size.h),
            ]),
        }
    }

    fn em_box(&mut self, font_system: &mut FontSystem, glyph: &LayoutGlyph) -> EmBox {
        let key = GlyphInkKey {
            font_id: glyph.font_id,
            glyph_id: glyph.glyph_id,
            weight: glyph.font_weight.0,
            fake_italic: glyph.cache_key_flags.contains(CacheKeyFlags::FAKE_ITALIC),
        };
        if let Some(&em) = self.bounds.get(&key) {
            return em;
        }
        let em = self.read_em_box(font_system, key);
        self.remember(key, em);
        em
    }

    /// Hold `em` for `key`, starting the table over at [`MAX_INKED_GLYPHS`]; `clear` keeps the
    /// allocation, so the table never grows past it.
    fn remember(&mut self, key: GlyphInkKey, em: EmBox) {
        if self.bounds.len() >= MAX_INKED_GLYPHS {
            self.bounds.clear();
        }
        self.bounds.insert(key, em);
    }

    /// The glyph's bounds from each source the rasterizer can draw it from: its color outline, else
    /// its plain outline grown as the raster darkens it and, for a face with bitmap strikes, the
    /// face's box. Skewed as it skews a synthetic italic, from y-up font units.
    fn read_em_box(&mut self, font_system: &mut FontSystem, key: GlyphInkKey) -> EmBox {
        let Some(font) = font_system.get_font(key.font_id, fontdb::Weight(key.weight)) else {
            return EmBox::default();
        };
        let units_per_em = font.as_swash().metrics(&[]).units_per_em;
        if units_per_em == 0 {
            return EmBox::default();
        }
        let units_per_em = f32::from(units_per_em);
        let mut scaler = ScalerSpec {
            size: 0.0,
            hinting: Hinting::Off,
            weight: fontdb::Weight(key.weight),
        }
        .scaler(&mut self.ctx, &font);
        let mut lo = Vec2::INFINITY;
        let mut hi = Vec2::NEG_INFINITY;
        let mut take = |min: Vec2, max: Vec2| {
            lo = lo.min(min);
            hi = hi.max(max);
        };
        if scaler.scale_color_outline_into(key.glyph_id, &mut self.outline) {
            let b = self.outline.bounds();
            take(Vec2::new(b.min.x, b.min.y), Vec2::new(b.max.x, b.max.y));
        } else {
            let outlined = scaler.scale_outline_into(key.glyph_id, &mut self.outline);
            if outlined {
                StemDarkening::bound(units_per_em).dilate(&mut self.outline);
                let b = self.outline.bounds();
                take(Vec2::new(b.min.x, b.min.y), Vec2::new(b.max.x, b.max.y));
            }
            // A bitmap strike draws in place of the outline where it holds the glyph, and only the
            // face's box bounds it, so the box joins the darkened outline the raster falls back to.
            if (scaler.has_color_bitmaps() || !outlined)
                && let Some(b) = font.metrics().bounds
            {
                take(Vec2::new(b.x_min, b.y_min), Vec2::new(b.x_max, b.y_max));
            }
        }
        if lo.x > hi.x {
            return EmBox::default();
        }
        let skew = if key.fake_italic {
            FAKE_ITALIC_SKEW_DEGREES.to_radians().tan()
        } else {
            0.0
        };
        EmBox {
            min: Vec2::new(lo.x + skew * lo.y, -hi.y) / units_per_em,
            max: Vec2::new(hi.x + skew * hi.y, -lo.y) / units_per_em,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::text::cosmic::glyph_ink::{EmBox, GlyphInk, GlyphInkKey, MAX_INKED_GLYPHS};
    use cosmic_text::fontdb;

    /// The table holds [`MAX_INKED_GLYPHS`] glyphs; the next starts it over, holding that glyph
    /// alone.
    #[test]
    fn the_ink_table_starts_over_at_its_bound() {
        let mut ink = GlyphInk::default();
        let key = |i: usize| GlyphInkKey {
            font_id: fontdb::ID::dummy(),
            glyph_id: (i % (1 << 16)) as u16,
            weight: (i / (1 << 16)) as u16,
            fake_italic: false,
        };
        for i in 0..MAX_INKED_GLYPHS {
            ink.remember(key(i), EmBox::default());
        }
        assert_eq!(ink.bounds.len(), MAX_INKED_GLYPHS);
        ink.remember(key(MAX_INKED_GLYPHS), EmBox::default());
        assert_eq!(ink.bounds.len(), 1);
        assert!(ink.bounds.contains_key(&key(MAX_INKED_GLYPHS)));
    }
}
