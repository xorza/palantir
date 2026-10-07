//! SVG to pixels: the resident parsed-document cache and the raster call an atlas
//! miss falls through to.

use crate::icons::icon_raster_key::IconRasterKey;
use crate::icons::icon_registry::IconSetId;
use crate::icons::icon_set::IconRef;
use crate::icons::icon_table::IconTable;
use crate::icons::svg_facts;
use crate::primitives::paint::content_type::ContentType;
use crate::primitives::paint::raster_image::RasterImage;
use glam::IVec2;
use resvg::tiny_skia;
use resvg::usvg;
use rustc_hash::FxHashMap;
use std::fmt;

/// Resident parsed documents. Past this, a parse that would be the
/// `MAX_PARSED_TREES + 1`th retires the one longest unused. A ceiling rather than a
/// frame window: the atlas caches the *pixels*, so this map is consulted only when
/// both caches miss, and the cost lands on the miss already parsing an SVG.
const MAX_PARSED_TREES: usize = 128;

/// One icon's parse, with the stamp that picks which leaves when the cache is full.
#[derive(Debug)]
struct ParsedIcon {
    /// `None` marks an icon whose SVG failed to parse, so it is skipped, not
    /// retried every frame.
    tree: Option<usvg::Tree>,
    /// [`IconRasterizer::uses`] at the last rasterize; use order, so two icons
    /// drawn on one frame still rank.
    last_use: u64,
}

/// Turns a baked icon into pixels at an exact physical size, driven on atlas
/// misses. Behind it sit parsed [`usvg::Tree`]s (built on first rasterize at any
/// size, reused at every other, keyed by [`IconRef`], dropped by
/// [`Self::forget_sets`], capped at [`MAX_PARSED_TREES`]) and two scratch buffers
/// every raster renders through, so re-rasterizing allocates nothing after the
/// first frame at its largest size.
pub(crate) struct IconRasterizer {
    trees: FxHashMap<IconRef, ParsedIcon>,
    uses: u64,
    rgba: Vec<u8>,
    /// Atlas-ready bytes: coverage for a tintable icon, straight RGBA otherwise;
    /// lent out by [`Self::rasterize`].
    out: Vec<u8>,
    options: usvg::Options<'static>,
}

/// Hand-written so the parse settings come from [`svg_facts::parse_options`], the
/// ones the survey read the icon's facts under; a `Default` would be a second
/// spelling that could drift.
impl Default for IconRasterizer {
    fn default() -> Self {
        Self {
            trees: FxHashMap::default(),
            uses: 0,
            rgba: Vec::new(),
            out: Vec::new(),
            options: svg_facts::parse_options(),
        }
    }
}

/// `usvg::Options` holds a font database and is not `Debug`; the caches are
/// summarized by size.
impl fmt::Debug for IconRasterizer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IconRasterizer")
            .field("parsed", &self.trees.len())
            .field(
                "scratch_bytes",
                &(self.rgba.capacity() + self.out.capacity()),
            )
            .finish_non_exhaustive()
    }
}

impl IconRasterizer {
    /// Rasterize `key`, handing back the image over this rasterizer's retained
    /// buffer (the shape `CosmicMeasure::rasterize_glyph` answers in); the pixels
    /// live until the next rasterize. `None` means the SVG could not be parsed or
    /// the size was unrepresentable, and the failure is not retried.
    pub(crate) fn rasterize(
        &mut self,
        table: &IconTable,
        key: IconRasterKey,
    ) -> Option<RasterImage<'_>> {
        self.uses += 1;
        // Room is made before the probe so the ceiling counts the entry about to
        // land.
        if self.trees.len() >= MAX_PARSED_TREES && !self.trees.contains_key(&key.icon) {
            self.retire_least_used();
        }
        // Destructured so the parsed tree (borrowed from `trees`) and the scratch
        // buffers are held at once.
        let Self {
            trees,
            uses,
            rgba,
            out,
            options,
        } = self;
        let def = table.def(key.icon.icon);
        let entry = trees.entry(key.icon).or_insert_with(|| ParsedIcon {
            tree: usvg::Tree::from_data(table.svg_bytes(key.icon.icon), options).ok(),
            last_use: *uses,
        });
        entry.last_use = *uses;
        let tree = entry.tree.as_ref()?;

        let w = u32::from(key.size().x);
        let h = u32::from(key.size().y);
        let bytes = (w as usize).checked_mul(h as usize)?.checked_mul(4)?;
        // `clear` then `resize`: resvg composites over what it is given, and
        // `resize` zeroes only the new tail.
        rgba.clear();
        rgba.resize(bytes, 0);
        let mut pixmap = tiny_skia::PixmapMut::from_bytes(rgba.as_mut_slice(), w, h)?;

        // The tree's own size, not the def's viewBox: it is the space resvg renders
        // in.
        let size = tree.size();
        let transform =
            tiny_skia::Transform::from_scale(w as f32 / size.width(), h as f32 / size.height());
        resvg::render(tree, transform, &mut pixmap);

        out.clear();
        let rendered = pixmap.as_ref();
        let pixels = rendered.pixels();
        let content = if def.tintable {
            // Coverage only: a tintable icon has a single paint colour, which the
            // draw supplies.
            out.reserve_exact(pixels.len());
            out.extend(pixels.iter().map(|texel| texel.alpha()));
            ContentType::Mask
        } else {
            out.reserve_exact(bytes);
            for texel in pixels {
                let straight = texel.demultiply();
                out.extend_from_slice(&[
                    straight.red(),
                    straight.green(),
                    straight.blue(),
                    straight.alpha(),
                ]);
            }
            ContentType::Color
        };
        Some(RasterImage {
            content,
            size: key.size().as_uvec2(),
            bearing: IVec2::ZERO,
            data: out,
        })
    }

    /// Drop the parse that has gone longest without a rasterize. Linear in the map,
    /// but reached only by a call about to parse an SVG.
    fn retire_least_used(&mut self) {
        let Some(&coldest) = self
            .trees
            .iter()
            .min_by_key(|(_, parsed)| parsed.last_use)
            .map(|(icon, _)| icon)
        else {
            return;
        };
        self.trees.remove(&coldest);
    }

    /// Drop the parses held for every set in `sets`, whose last
    /// [`IconSet`](crate::IconSet) has gone; the scratch buffer stays. Takes every
    /// doomed set at once because `retain` walks the map's raw table, which never
    /// shrinks, so the walk is the cost.
    pub(crate) fn forget_sets(&mut self, sets: &[IconSetId]) {
        self.trees.retain(|icon, _| !sets.contains(&icon.set));
    }

    #[cfg(test)]
    pub(crate) fn parsed_count(&self) -> usize {
        self.trees.len()
    }
}

#[cfg(test)]
mod tests;
