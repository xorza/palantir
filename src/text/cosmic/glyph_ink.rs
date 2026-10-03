//! How far a shaped run's glyphs reach past the block it measured to.

use crate::primitives::spacing::Spacing;
use crate::text::cosmic::geometry::ShapedGeometry;
use crate::text::cosmic::{FAKE_ITALIC_SKEW_DEGREES, glyph_scaler};
use crate::text::extent::TextExtent;
use cosmic_text::{Buffer, CacheKeyFlags, FontSystem, LayoutGlyph, fontdb};
use glam::Vec2;
use rustc_hash::FxHashMap;
use swash::scale::ScaleContext;
use swash::scale::outline::Outline;

/// Ink bounds of every glyph a run has shaped, read once per glyph and
/// face and kept for the life of the measurer.
///
/// The measured block spans the glyphs' advances, and a glyph's ink is
/// not bound to its advance: an italic's overhang, a negative left side
/// bearing, a mark above the line box all reach past it. What reaches
/// past is what [`Self::outsets`] answers, so damage and the text scissor
/// can cover it.
///
/// **In ems, from the unhinted outline.** Every glyph rasterizes unhinted
/// (`attrs_named` sets `DISABLE_HINTING`), so its ink scales linearly
/// with size and one entry per glyph and face serves every size it is
/// drawn at. Bounded by the glyphs of the faces a session shapes, so
/// nothing is evicted.
#[derive(Default)]
pub(super) struct GlyphInk {
    bounds: FxHashMap<GlyphInkKey, EmBox>,
    /// Swash's per-face scratch, kept apart from the rasterizer's so the
    /// shaping paths can read ink while they hold the measurer's fields.
    context: ScaleContext,
    /// Retained outline, so a miss scales into it without allocating.
    outline: Outline,
}

impl std::fmt::Debug for GlyphInk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlyphInk")
            .field("bounds", &self.bounds.len())
            .finish_non_exhaustive()
    }
}

/// What a glyph's ink depends on beyond its size: the face, the glyph,
/// the weight a variable face is instanced at, and the synthetic skew.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct GlyphInkKey {
    font_id: fontdb::ID,
    glyph_id: u16,
    weight: u16,
    fake_italic: bool,
}

/// A glyph's ink in ems, relative to its pen position on the baseline,
/// y down like the buffer it is placed in.
#[derive(Clone, Copy, Debug, Default)]
struct EmBox {
    min: Vec2,
    max: Vec2,
}

impl GlyphInk {
    /// `buffer`'s block, as `geometry` measured it, and how far its
    /// glyphs' ink reaches past it — `x` from `left` to `left + size.w`,
    /// `y` from 0 to `size.h` — per side, rounded out to whole pixels, and
    /// zero on a side it stays inside.
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
                // Where `LayoutGlyph::physical` puts the pen, unscaled.
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
            ink: Spacing::new(
                past(left - min.x),
                past(-min.y),
                past(max.x - (left + size.w)),
                past(max.y - size.h),
            ),
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
        self.bounds.insert(key, em);
        em
    }

    /// The glyph's bounds from the source the rasterizer draws it from
    /// (`GLYPH_SOURCES`): a colour outline, else — in a face with colour
    /// bitmaps, whose strikes have no outline to measure — the face's
    /// own bounds, else the plain outline, else the face's bounds. Each
    /// is the box of the outline's control points, which holds its curves.
    /// Skewed as the rasterizer skews a synthetic italic, in y-up font
    /// units where the skew leans the top right.
    fn read_em_box(&mut self, font_system: &mut FontSystem, key: GlyphInkKey) -> EmBox {
        let Some(font) = font_system.get_font(key.font_id, fontdb::Weight(key.weight)) else {
            return EmBox::default();
        };
        // A face that claims no em is malformed, and has no scale to
        // read its outlines at.
        let units_per_em = font.as_swash().metrics(&[]).units_per_em;
        if units_per_em == 0 {
            return EmBox::default();
        }
        let units_per_em = f32::from(units_per_em);
        // Size zero reads the outline in font units.
        let mut scaler = glyph_scaler(
            &mut self.context,
            &font,
            0.0,
            false,
            fontdb::Weight(key.weight),
        );
        let outlined = scaler.scale_color_outline_into(key.glyph_id, &mut self.outline)
            || (!scaler.has_color_bitmaps()
                && scaler.scale_outline_into(key.glyph_id, &mut self.outline));
        let (lo, hi) = if outlined {
            let bounds = self.outline.bounds();
            (
                Vec2::new(bounds.min.x, bounds.min.y),
                Vec2::new(bounds.max.x, bounds.max.y),
            )
        } else {
            match font.metrics().bounds {
                Some(b) => (Vec2::new(b.x_min, b.y_min), Vec2::new(b.x_max, b.y_max)),
                None => return EmBox::default(),
            }
        };
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
