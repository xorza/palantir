//! [`GlyphRaster`]: one glyph drawn to the pixels its atlas holds: a COLR color glyph or a color
//! bitmap where the face has one, as cosmic-text tries them, else its outline as a coverage mask.

use crate::primitives::math::num::unit_to_u8;
use crate::primitives::paint::content_type::ContentType;
use crate::primitives::paint::raster_image::RasterImage;
use crate::text::cosmic::bitmap_raster::BitmapRaster;
use crate::text::cosmic::coverage_raster::{CoverageRaster, Placement};
use crate::text::cosmic::glyph_face::{ColorLayer, GlyphFace};
use crate::text::cosmic::glyph_outline::GlyphOutline;
use crate::text::cosmic::stem_darkening::StemDarkening;
use glam::{IVec2, UVec2, Vec2};
use skrifa::GlyphId;
use skrifa::instance::Size;
use std::ops::Range;

/// The glyph [`GlyphRaster::render`] draws, as its cache key holds it.
#[derive(Clone, Copy, Debug)]
pub(super) struct GlyphRequest {
    pub(super) glyph: GlyphId,
    pub(super) ppem: f32,
    /// How far a plain outline grows; a color glyph's layers are artwork, not stems, and do not.
    pub(super) darkening: StemDarkening,
    /// A synthetic italic's lean, the tangent of its angle, after the dilation, as cosmic-text
    /// leans it.
    pub(super) skew: Option<f32>,
    /// The fractional pen offset the glyph was binned at.
    pub(super) offset: Vec2,
}

/// The scratch every glyph draws through, kept between glyphs so a glyph allocates nothing once
/// its buffers have grown, and the pixels of the last glyph drawn.
#[derive(Debug, Default)]
pub(super) struct GlyphRaster {
    outline: GlyphOutline,
    coverage: CoverageRaster,
    layers: Vec<ColorLayer>,
    /// Each color layer's run of the coverage raster's edges.
    layer_lines: Vec<Range<usize>>,
    mask: Vec<u8>,
    bitmap: BitmapRaster,
    pixels: Vec<u8>,
}

impl GlyphRaster {
    /// Draw `request` from `face`: its COLR v0 glyph, else its color bitmap, else its outline;
    /// `None` where the face draws none of them. The pixels stay here until the next glyph.
    pub(super) fn render(
        &mut self,
        face: &GlyphFace<'_>,
        request: GlyphRequest,
    ) -> Option<RasterImage<'_>> {
        let (content, placement) = if let Some(placement) = self.color_outline(face, request) {
            (ContentType::Color, placement)
        } else if let Some(placement) = self.color_bitmap(face, request) {
            (ContentType::Color, placement)
        } else {
            (ContentType::Mask, self.outline(face, request)?)
        };
        Some(RasterImage {
            content,
            size: UVec2::new(placement.width, placement.height),
            bearing: IVec2::new(placement.left, placement.top),
            data: &self.pixels,
        })
    }

    /// The glyph's COLR v0 layers, each filled over one grid that holds them all and laid over
    /// the last in its color, straight alpha over straight alpha.
    fn color_outline(&mut self, face: &GlyphFace<'_>, request: GlyphRequest) -> Option<Placement> {
        if !face.color_layers_into(request.glyph, &mut self.layers) {
            return None;
        }
        self.coverage.begin();
        self.layer_lines.clear();
        for layer in &self.layers {
            if !face.outline_into(layer.glyph, Size::new(request.ppem), &mut self.outline) {
                return None;
            }
            if let Some(tan) = request.skew {
                self.outline.skew(tan);
            }
            self.layer_lines
                .push(self.coverage.add(&self.outline, request.offset));
        }
        let placement = self.coverage.placement()?;
        self.pixels.clear();
        self.pixels.resize(placement.pixels() * 4, 0);
        for (layer, lines) in self.layers.iter().zip(&self.layer_lines) {
            self.coverage.fill(lines.clone(), placement, &mut self.mask);
            source_over(&mut self.pixels, &self.mask, layer.rgba);
        }
        Some(placement)
    }

    fn color_bitmap(&mut self, face: &GlyphFace<'_>, request: GlyphRequest) -> Option<Placement> {
        let bitmap = face.color_bitmap(request.glyph, request.ppem)?;
        self.bitmap
            .render(&bitmap, face.units_per_em(), request.ppem, &mut self.pixels)
    }

    /// The glyph's outline, grown by its darkening and leaned by its skew, as a coverage mask.
    fn outline(&mut self, face: &GlyphFace<'_>, request: GlyphRequest) -> Option<Placement> {
        if !face.outline_into(request.glyph, Size::new(request.ppem), &mut self.outline) {
            return None;
        }
        request.darkening.dilate(&mut self.outline);
        if let Some(tan) = request.skew {
            self.outline.skew(tan);
        }
        self.coverage.begin();
        let lines = self.coverage.add(&self.outline, request.offset);
        let placement = self.coverage.placement()?;
        self.coverage.fill(lines, placement, &mut self.pixels);
        Some(placement)
    }
}

/// Lay `mask`'s coverage of `rgba` over `pixels`, both straight sRGB RGBA8, as source-over
/// composites straight alpha: `a = aₛ + a_d(1 − aₛ)` and each channel the alphas' weighted mean.
fn source_over(pixels: &mut [u8], mask: &[u8], rgba: [u8; 4]) {
    let color = rgba.map(|c| f32::from(c) / 255.0);
    for (px, &coverage) in pixels.as_chunks_mut::<4>().0.iter_mut().zip(mask) {
        let src_a = f32::from(coverage) / 255.0 * color[3];
        if src_a == 0.0 {
            continue;
        }
        let dst_a = f32::from(px[3]) / 255.0;
        let under = dst_a * (1.0 - src_a);
        let out_a = src_a + under;
        for c in 0..3 {
            px[c] = unit_to_u8((color[c] * src_a + f32::from(px[c]) / 255.0 * under) / out_a);
        }
        px[3] = unit_to_u8(out_a);
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::paint::content_type::ContentType;
    use crate::text::cosmic::glyph_face::{ColorLayer, GlyphFace};
    use crate::text::cosmic::glyph_raster::{GlyphRaster, GlyphRequest, source_over};
    use crate::text::cosmic::stem_darkening::StemDarkening;
    use crate::text::cosmic::tests::{
        color_font_with_second_layer_in, mono, mono_face, two_layer_color_font,
    };
    use cosmic_text::Weight;
    use glam::Vec2;
    use glam::{IVec2, UVec2};
    use skrifa::GlyphId;

    /// The coverage of JetBrains Mono's `n` at `ppem`, darkened as text at that size or not.
    fn coverage(ppem: f32, darken: bool) -> Vec<u8> {
        let font = mono();
        let face = mono_face(&font, Weight::NORMAL);
        let darkening = if darken {
            StemDarkening::at_ppem(ppem)
        } else {
            StemDarkening::NONE
        };
        let request = GlyphRequest {
            glyph: face.glyph_of('n'),
            ppem,
            darkening,
            skew: None,
            offset: Vec2::ZERO,
        };
        GlyphRaster::default()
            .render(&face, request)
            .expect("`n` has an outline")
            .data
            .to_vec()
    }

    /// At 12 ppem the stems grow by 0.1452 px a side, so the darkened `n` holds more coverage
    /// than the plain one; past 72 ppem nothing grows, and the two masks are the same bytes.
    #[test]
    fn small_text_darkens_and_large_text_draws_as_its_outline_stands() {
        let total = |mask: &[u8]| mask.iter().map(|&a| u32::from(a)).sum::<u32>();
        assert!(total(&coverage(12.0, true)) > total(&coverage(12.0, false)));
        assert_eq!(coverage(80.0, true), coverage(80.0, false));
    }

    /// A full layer replaces what lies under it; a half-covered one over transparent keeps its
    /// own color at half alpha, straight; over an opaque pixel it mixes half and half, opaque:
    /// red over blue at 128/255 coverage gives (128, 0, 127, 255).
    #[test]
    fn a_layer_lays_over_straight_alpha() {
        let mut pixels = [0, 0, 255, 255, 0, 0, 0, 0, 0, 0, 255, 255];
        source_over(&mut pixels, &[255, 128, 128], [255, 0, 0, 255]);
        assert_eq!(pixels, [255, 0, 0, 255, 255, 0, 0, 128, 128, 0, 127, 255]);

        let mut untouched = [10, 20, 30, 40];
        source_over(&mut untouched, &[0], [255, 0, 0, 255]);
        assert_eq!(untouched, [10, 20, 30, 40], "no coverage leaves the pixel");
    }

    /// A COLR v0 glyph draws its layers in their palette colors over one grid: at 10 ppem the
    /// 1000-unit em square is 10 × 10 px, its left five columns opaque red and its right five
    /// the half-transparent blue alone, as the layers meet at x = 5 and do not overlap.
    #[test]
    fn a_color_glyph_draws_each_layer_in_its_palette_color() {
        let font = two_layer_color_font();
        let face = GlyphFace::new(&font, 0, Weight::NORMAL).expect("the face parses");
        let mut layers = Vec::new();
        assert!(face.color_layers_into(GlyphId::new(0), &mut layers));
        assert_eq!(
            layers,
            [
                ColorLayer {
                    glyph: GlyphId::new(1),
                    rgba: [255, 0, 0, 255],
                },
                ColorLayer {
                    glyph: GlyphId::new(2),
                    rgba: [0, 0, 255, 128],
                },
            ]
        );
        assert!(
            !face.color_layers_into(GlyphId::new(1), &mut layers),
            "a layer is no color glyph"
        );
        // Index 0xFFFF, the text's color, and index 5, past the two-color palette, both take the
        // neutral gray a shared raster draws for the text's color.
        for index in [0xFFFF, 5] {
            let font = color_font_with_second_layer_in(index);
            let face = GlyphFace::new(&font, 0, Weight::NORMAL).expect("the face parses");
            assert!(face.color_layers_into(GlyphId::new(0), &mut layers));
            assert_eq!(layers[1].rgba, [128, 128, 128, 255], "index {index}");
        }

        let mut raster = GlyphRaster::default();
        let image = raster
            .render(
                &face,
                GlyphRequest {
                    glyph: GlyphId::new(0),
                    ppem: 10.0,
                    darkening: StemDarkening::at_ppem(10.0),
                    skew: None,
                    offset: Vec2::ZERO,
                },
            )
            .expect("a color glyph");
        assert_eq!(image.content, ContentType::Color);
        assert_eq!(image.size, UVec2::new(10, 10));
        assert_eq!(image.bearing, IVec2::new(0, 10));
        let row = [[255, 0, 0, 255]; 5]
            .into_iter()
            .chain([[0, 0, 255, 128]; 5])
            .flatten()
            .collect::<Vec<u8>>();
        assert_eq!(image.data, row.repeat(10), "undarkened, as artwork");

        // Glyph 1 alone draws as a plain outline, a coverage mask of its five columns, darkened.
        let mask = raster
            .render(
                &face,
                GlyphRequest {
                    glyph: GlyphId::new(1),
                    ppem: 10.0,
                    darkening: StemDarkening::NONE,
                    skew: None,
                    offset: Vec2::ZERO,
                },
            )
            .expect("an outline");
        assert_eq!(mask.content, ContentType::Mask);
        assert_eq!(mask.size, UVec2::new(5, 10));
        assert_eq!(mask.data, [255; 50]);
    }
}
