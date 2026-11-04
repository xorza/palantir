//! [`OutlineRaster`]: a glyph's coverage mask from its outline, grown by [`StemDarkening`].

use crate::text::cosmic::stem_darkening::StemDarkening;
use std::fmt;
use swash::GlyphId;
use swash::scale::image::{Content, Image};
use swash::scale::outline::Outline;
use swash::scale::{Scaler, Source};
use swash::zeno::{Format, Mask, Origin, Scratch, Transform, Vector};

/// Swash's outline path of `Render::render_into`, with [`StemDarkening::dilate`] between the
/// scale and the transform in place of `Render::embolden`, which grows both axes alike, off
/// center, and the wrong way on some glyphs (see [`StemDarkening::dilate`]). Holds the outline
/// and the rasterizer's scratch, so a glyph allocates nothing.
#[derive(Default)]
pub(super) struct OutlineRaster {
    outline: Outline,
    scratch: Scratch,
}

impl fmt::Debug for OutlineRaster {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OutlineRaster").finish_non_exhaustive()
    }
}

/// The glyph [`OutlineRaster::render`] draws, as its cache key holds it.
#[derive(Clone, Copy, Debug)]
pub(super) struct OutlineGlyph {
    pub(super) id: GlyphId,
    pub(super) darkening: StemDarkening,
    /// A synthetic italic's lean, after the dilation, as cosmic-text leans it.
    pub(super) skew: Option<Transform>,
    /// The fractional pen offset the glyph was binned at.
    pub(super) offset: Vector,
}

impl OutlineRaster {
    /// Draw `glyph`'s coverage into `image`, whose data is empty; `false` when the face has no
    /// outline for it.
    pub(super) fn render(
        &mut self,
        scaler: &mut Scaler<'_>,
        glyph: OutlineGlyph,
        image: &mut Image,
    ) -> bool {
        debug_assert!(image.data.is_empty(), "a mask fills an emptied image");
        if !scaler.scale_outline_into(glyph.id, &mut self.outline) {
            return false;
        }
        glyph.darkening.dilate(&mut self.outline);
        if let Some(skew) = &glyph.skew {
            self.outline.transform(skew);
        }
        let data = &mut image.data;
        image.placement = Mask::with_scratch(self.outline.path(), &mut self.scratch)
            .format(Format::Alpha)
            .origin(Origin::BottomLeft)
            .offset(glyph.offset)
            .render_offset(glyph.offset)
            .inspect(|format, w, h| data.resize(format.buffer_size(w, h), 0))
            .render_into(data, None);
        image.content = Content::Mask;
        image.source = Source::Outline;
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::text::cosmic::outline_raster::{OutlineGlyph, OutlineRaster};
    use crate::text::cosmic::stem_darkening::StemDarkening;
    use crate::text::cosmic::tests::mono;
    use crate::text::cosmic::{Hinting, ScalerSpec};
    use cosmic_text::Weight;
    use swash::scale::image::Image;
    use swash::scale::{Render, ScaleContext, Source};
    use swash::zeno::{Format, Vector};

    /// The coverage of JetBrains Mono's `n` at `ppem`: through [`OutlineRaster`] when `darken`,
    /// else through swash's own outline path, which grows nothing.
    fn coverage(ppem: f32, darken: bool) -> Vec<u8> {
        let font = mono();
        let mut ctx = ScaleContext::new();
        let mut scaler = ScalerSpec {
            size: ppem,
            hinting: Hinting::Off,
            weight: Weight::NORMAL,
        }
        .scaler(&mut ctx, &font);
        let id = font.as_swash().charmap().map('n');
        let mut image = Image::new();
        let rendered = if darken {
            OutlineRaster::default().render(
                &mut scaler,
                OutlineGlyph {
                    id,
                    darkening: StemDarkening::at_ppem(ppem),
                    skew: None,
                    offset: Vector::ZERO,
                },
                &mut image,
            )
        } else {
            Render::new(&[Source::Outline])
                .format(Format::Alpha)
                .render_into(&mut scaler, id, &mut image)
        };
        assert!(rendered);
        image.data
    }

    /// At 12 ppem the stems grow by 0.1452 px a side, so the darkened `n` holds more coverage
    /// than swash's own; past 72 ppem nothing grows, and the two masks are the same bytes, so the
    /// path itself matches swash's.
    #[test]
    fn small_text_darkens_and_large_text_draws_as_swash_does() {
        let total = |mask: &[u8]| mask.iter().map(|&a| u32::from(a)).sum::<u32>();
        assert!(total(&coverage(12.0, true)) > total(&coverage(12.0, false)));
        assert_eq!(coverage(80.0, true), coverage(80.0, false));
    }
}
