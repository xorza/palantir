//! SVG to pixels: the resident parsed-document cache, and the raster call an
//! atlas miss falls through to.

use crate::icons::icon_raster_key::IconRasterKey;
use crate::icons::icon_registry::IconSetId;
use crate::icons::icon_set::IconRef;
use crate::icons::icon_table::IconTable;
use crate::icons::svg_facts;
use crate::renderer::backend::raster_atlas::content_type::ContentType;
use resvg::tiny_skia;
use resvg::usvg;
use rustc_hash::FxHashMap;

/// Resident parsed documents. Past this, a parse that would be the
/// `MAX_PARSED_TREES + 1`th retires the one longest unused.
///
/// A ceiling rather than a frame window, because a parse is not per-frame
/// work: the atlas caches the *pixels*, so this map is only consulted when
/// both caches miss. Sweeping it on a clock would walk a table sized by the
/// session's peak parse count every frame to find nothing; sizing it instead
/// puts the whole cost on the miss that is already about to parse an SVG.
/// 128 is generous against a screenful of distinct icons and bounded against
/// a session that browses a thousand-icon set once.
const MAX_PARSED_TREES: usize = 128;

/// One icon's parse, with the stamp that decides which parse leaves when the
/// cache is full.
#[derive(Debug)]
struct ParsedIcon {
    /// `None` marks an icon whose SVG failed to parse, so a broken icon is
    /// parsed once and skipped thereafter rather than retried every frame.
    tree: Option<usvg::Tree>,
    /// [`IconRasterizer::uses`] at the last rasterize through this entry.
    /// Use order, not frame order — two icons drawn on the same frame still
    /// rank, which is what a screenful of icons over the ceiling needs.
    last_use: u64,
}

/// Turns a baked icon into pixels at an exact physical size.
///
/// Owned by the icon backend and driven on atlas misses. Two caches sit
/// behind it: parsed [`usvg::Tree`]s, built the first time an icon is
/// rasterized at any size and reused at every other size, and one RGBA
/// scratch buffer that every raster renders through — so a steady state that
/// re-rasterizes (a zoom gesture) allocates nothing after the first frame at
/// its largest size.
///
/// The parses are the set's, not the rasterizer's: they are keyed by
/// [`IconRef`] and dropped by [`Self::forget_sets`] when the set unloads —
/// and capped at [`MAX_PARSED_TREES`] before that, so a session that draws
/// its way through a large set keeps a working set rather than all of it.
/// The scratch buffer belongs to nobody and stays.
pub(crate) struct IconRasterizer {
    trees: FxHashMap<IconRef, ParsedIcon>,
    /// Monotonic rasterize counter — the clock `ParsedIcon::last_use` reads.
    uses: u64,
    /// Premultiplied RGBA that resvg renders into, whatever the icon's kind —
    /// a mask is the alpha channel of this, extracted after the render.
    rgba: Vec<u8>,
    options: usvg::Options<'static>,
}

/// Hand-written rather than derived so the parse settings come from
/// [`svg_facts::parse_options`] — the same ones the survey read the icon's
/// `tintable` / `filtered` / `view_box` under. A `Default` here would be a
/// second, independent spelling of them, and the two trees would describe
/// the same artwork only for as long as nobody changed one.
impl Default for IconRasterizer {
    fn default() -> Self {
        Self {
            trees: FxHashMap::default(),
            uses: 0,
            rgba: Vec::new(),
            options: svg_facts::parse_options(),
        }
    }
}

/// `usvg::Options` holds a font database and is not `Debug`; the caches are
/// summarized by size, since printing parsed SVG trees would be useless.
impl std::fmt::Debug for IconRasterizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IconRasterizer")
            .field("parsed", &self.trees.len())
            .field("scratch_bytes", &self.rgba.capacity())
            .finish_non_exhaustive()
    }
}

impl IconRasterizer {
    /// Rasterize `key` into `out`, returning which atlas side it belongs on.
    ///
    /// `out` is cleared and refilled — pass the backend's retained staging
    /// buffer, not a fresh `Vec`. `None` means the icon's SVG could not be
    /// parsed or the size was unrepresentable; the caller skips the draw, and
    /// the failure is not retried.
    pub(crate) fn rasterize(
        &mut self,
        table: &IconTable,
        key: IconRasterKey,
        out: &mut Vec<u8>,
    ) -> Option<ContentType> {
        // Destructured so the parsed tree (borrowed from `trees`) and the
        // scratch buffer are held at once — disjoint fields that method calls
        // on `&mut self` could not express.
        self.uses += 1;
        // Room made before the probe, so the ceiling counts the entry about
        // to land rather than the one after it. The extra lookup rides the
        // miss path of the atlas, which is already parsing or scaling an SVG.
        if self.trees.len() >= MAX_PARSED_TREES && !self.trees.contains_key(&key.icon) {
            self.retire_least_used();
        }
        let Self {
            trees,
            uses,
            rgba,
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
        // `clear` then `resize` rather than `resize` alone: resvg composites
        // over what it is handed, so the buffer has to start transparent, and
        // only the newly added tail of a `resize` is zeroed.
        rgba.clear();
        rgba.resize(bytes, 0);
        let mut pixmap = tiny_skia::PixmapMut::from_bytes(rgba.as_mut_slice(), w, h)?;

        // The tree's own size, not the def's viewBox: they agree, but this is
        // the space resvg actually renders in. Non-uniform when the caller
        // asked for a box off the icon's aspect ratio.
        let size = tree.size();
        let transform =
            tiny_skia::Transform::from_scale(w as f32 / size.width(), h as f32 / size.height());
        resvg::render(tree, transform, &mut pixmap);

        out.clear();
        // Both arms read the render through the same typed view. Alpha is
        // not premultiplied by itself, so the mask takes its byte
        // straight off the premultiplied texel.
        let rendered = pixmap.as_ref();
        let pixels = rendered.pixels();
        if def.tintable {
            // Coverage only. The colour the artwork was drawn in is discarded
            // — a tintable icon is defined as one whose paint is a single
            // colour, and the draw supplies that colour.
            out.reserve_exact(pixels.len());
            out.extend(pixels.iter().map(|texel| texel.alpha()));
            Some(ContentType::Mask)
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
            Some(ContentType::Color)
        }
    }

    /// Drop the parse that has gone longest without a rasterize.
    ///
    /// Linear in the map, and only ever reached by a call that is about to
    /// parse an SVG — orders of magnitude more work than the scan — so the
    /// cache needs no second structure to keep an order in.
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
    /// [`IconSet`](crate::IconSet) has gone.
    ///
    /// This is the expensive half of unloading: a `usvg::Tree` is a parsed
    /// document, and one is retained per icon the session ever drew. The
    /// scratch buffer stays — it belongs to no set and the next raster
    /// wants it.
    ///
    /// Takes every doomed set at once because `retain` walks the map's
    /// raw table, which is sized by the session's peak parse count and
    /// never shrinks — so the walk is the cost, not the matching, and
    /// paying it per released set was the waste. `sets` is a handful, so
    /// the linear membership test per entry is free beside the walk.
    pub(crate) fn forget_sets(&mut self, sets: &[IconSetId]) {
        self.trees.retain(|icon, _| !sets.contains(&icon.set));
    }

    /// Icons whose parse has already been paid for. Lets a test assert that
    /// re-rastering one icon at many sizes parses it once.
    #[cfg(test)]
    pub(crate) fn parsed_count(&self) -> usize {
        self.trees.len()
    }
}

#[cfg(test)]
mod tests;
