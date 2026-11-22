//! [`GlyphFace`]: one cosmic face read through skrifa at one weight, for what the rasterizer and
//! the ink table draw from it: its outlines, its COLR layers, its color bitmaps and its em.

use crate::text::cosmic::axis_range::AxisRange;
use crate::text::cosmic::glyph_outline::GlyphOutline;
use cosmic_text::Weight;
use skrifa::bitmap::{BitmapFormat, BitmapGlyph, BitmapStrikes};
use skrifa::instance::{Location, Size};
use skrifa::outline::DrawSettings;
use skrifa::raw::TableProvider as _;
use skrifa::{FontRef, GlyphId, MetadataProvider as _, Tag};
use std::fmt;

/// The OpenType weight axis, for variable faces: the one axis set. Every other axis, Inter's
/// `opsz` among them, stays at the face's default, as cosmic-text 0.19 instances a face for
/// shaping and advances on `wght` alone (`font/mod.rs`); a raster at another optical size would
/// draw glyphs the shaper did not measure. Refused: a fork of cosmic-text for `opsz`.
pub(super) const WGHT_AXIS: Tag = Tag::new(b"wght");

/// The color a layer of palette index `0xFFFF` takes, which COLR reserves for the text's own
/// color: a color glyph's raster is shared by every run that draws it, whatever its color, so
/// the layer takes swash's neutral gray, as it did before.
const FOREGROUND: [u8; 4] = [128, 128, 128, 255];

/// One layer of a COLR v0 glyph: the glyph whose outline it fills and the straight sRGB color it
/// fills it with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ColorLayer {
    pub(super) glyph: GlyphId,
    pub(super) rgba: [u8; 4],
}

/// A face's tables and its place on the weight axis.
#[derive(Clone)]
pub(super) struct GlyphFace<'a> {
    font: FontRef<'a>,
    location: Location,
}

// Manual: skrifa's `FontRef` is not `Debug`.
impl fmt::Debug for GlyphFace<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GlyphFace")
            .field("location", &self.location)
            .finish_non_exhaustive()
    }
}

impl<'a> GlyphFace<'a> {
    /// Face `index` of `data` at `weight`, on [`WGHT_AXIS`] where the face varies on it and its
    /// axis is in order, else at its default instance; `None` where the bytes hold no such face.
    pub(super) fn new(data: &'a [u8], index: u32, weight: Weight) -> Option<Self> {
        let font = FontRef::from_index(data, index).ok()?;
        let axes = font.axes();
        let wght = axes
            .iter()
            .find(|axis| axis.tag() == WGHT_AXIS)
            .and_then(|axis| {
                AxisRange::new(axis.min_value(), axis.default_value(), axis.max_value())
                    .coordinate(f32::from(weight.0))
            });
        let location = match wght {
            Some(wght) => axes.location([(WGHT_AXIS, wght)]),
            None => Location::default(),
        };
        Some(Self { font, location })
    }

    /// Units per em, zero where the `head` table is missing.
    pub(super) fn units_per_em(&self) -> u16 {
        self.font.head().map_or(0, |head| head.units_per_em())
    }

    /// Draw `glyph`'s outline at `size`, unhinted, into `outline`, which it clears first; `false`
    /// where the face has no outline for it or the outline does not parse.
    pub(super) fn outline_into(
        &self,
        glyph: GlyphId,
        size: Size,
        outline: &mut GlyphOutline,
    ) -> bool {
        outline.clear();
        let Some(drawn) = self.font.outline_glyphs().get(glyph) else {
            return false;
        };
        drawn
            .draw(DrawSettings::unhinted(size, &self.location), outline)
            .is_ok()
    }

    /// The layers of `glyph`'s COLR v0 color glyph, bottom first, colored from palette 0, into
    /// `layers`, which it clears first; `false` where the face has no such glyph or its tables do
    /// not read. COLR v1 is left to the plain outline, as swash left it.
    pub(super) fn color_layers_into(&self, glyph: GlyphId, layers: &mut Vec<ColorLayer>) -> bool {
        layers.clear();
        let Ok(colr) = self.font.colr() else {
            return false;
        };
        let Ok(Some(range)) = colr.v0_base_glyph(glyph) else {
            return false;
        };
        // Palette 0: the color records from the first palette's start.
        let cpal = self.font.cpal().ok();
        let palette = cpal.as_ref().and_then(|cpal| {
            let first = usize::from(cpal.color_record_indices().first()?.get());
            cpal.color_records_array()?.ok()?.get(first..)
        });
        for at in range {
            let Ok((layer, palette_index)) = colr.v0_layer(at) else {
                layers.clear();
                return false;
            };
            // Index `0xFFFF` is the text's color, and an index past the palette names no color;
            // COLR's readers draw the text color for both.
            let rgba = palette
                .and_then(|records| records.get(usize::from(palette_index)))
                .filter(|_| palette_index != 0xFFFF)
                .map_or(FOREGROUND, |c| [c.red(), c.green(), c.blue(), c.alpha()]);
            layers.push(ColorLayer {
                glyph: layer.into(),
                rgba,
            });
        }
        !layers.is_empty()
    }

    /// The color strike's bitmap of `glyph` best for `ppem`: an `sbix` strike, else a `CBDT` one,
    /// of that size, the nearest larger, else the nearest smaller.
    pub(super) fn color_bitmap(&self, glyph: GlyphId, ppem: f32) -> Option<BitmapGlyph<'a>> {
        self.color_strikes()?.glyph_for_size(Size::new(ppem), glyph)
    }

    /// Whether the face holds color bitmap strikes.
    pub(super) fn has_color_bitmaps(&self) -> bool {
        self.color_strikes()
            .is_some_and(|strikes| !strikes.is_empty())
    }

    fn color_strikes(&self) -> Option<BitmapStrikes<'a>> {
        BitmapStrikes::with_format(&self.font, BitmapFormat::Sbix)
            .or_else(|| BitmapStrikes::with_format(&self.font, BitmapFormat::Cbdt))
    }
}

#[cfg(test)]
impl GlyphFace<'_> {
    /// The glyph `ch` maps to, `.notdef` where the face has none.
    pub(super) fn glyph_of(&self, ch: char) -> GlyphId {
        self.font.charmap().map(ch).unwrap_or(GlyphId::NOTDEF)
    }
}
