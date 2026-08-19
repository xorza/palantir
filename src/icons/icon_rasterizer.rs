use crate::icons::icon_atlas::IconAtlas;
use crate::icons::icon_raster_key::IconRasterKey;
use crate::icons::icon_registry::IconSetId;
use crate::icons::icon_set::IconRef;
use resvg::tiny_skia;
use resvg::usvg;
use rustc_hash::FxHashMap;

/// Which of the atlas's two sides a rasterized icon belongs on, and so what
/// `out` holds after [`IconRasterizer::rasterize`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IconRasterKind {
    /// One coverage byte per pixel. The draw multiplies it by the shape's
    /// full tint, so one baked icon serves every theme colour.
    Mask,
    /// Straight (non-premultiplied) sRGB RGBA, four bytes per pixel — what
    /// the colour atlas side stores and what the glyph shader premultiplies
    /// in linear at output.
    Color,
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
/// [`IconRef`] and dropped by [`Self::forget_sets`] when the set unloads. The
/// scratch buffer belongs to nobody and stays.
#[derive(Default)]
pub(crate) struct IconRasterizer {
    /// `None` marks an icon whose SVG failed to parse, so a broken icon is
    /// parsed once and skipped thereafter rather than retried every frame.
    trees: FxHashMap<IconRef, Option<usvg::Tree>>,
    /// Premultiplied RGBA that resvg renders into, whatever the icon's kind —
    /// a mask is the alpha channel of this, extracted after the render.
    rgba: Vec<u8>,
    options: usvg::Options<'static>,
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
        atlas: &IconAtlas,
        key: IconRasterKey,
        out: &mut Vec<u8>,
    ) -> Option<IconRasterKind> {
        // Destructured so the parsed tree (borrowed from `trees`) and the
        // scratch buffer are held at once — disjoint fields that method calls
        // on `&mut self` could not express.
        let Self {
            trees,
            rgba,
            options,
        } = self;
        let def = atlas.def(key.icon.icon);
        let tree = trees
            .entry(key.icon)
            .or_insert_with(|| usvg::Tree::from_data(atlas.svg_bytes(key.icon.icon), options).ok())
            .as_ref()?;

        let w = u32::from(key.size.x);
        let h = u32::from(key.size.y);
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
        if def.tintable {
            // Coverage only. The colour the artwork was drawn in is discarded
            // — a tintable icon is defined as one whose paint is a single
            // colour, and the draw supplies that colour.
            out.reserve_exact(bytes / 4);
            out.extend(rgba.chunks_exact(4).map(|texel| texel[3]));
            Some(IconRasterKind::Mask)
        } else {
            out.reserve_exact(bytes);
            for texel in pixmap.as_ref().pixels() {
                let straight = texel.demultiply();
                out.extend_from_slice(&[
                    straight.red(),
                    straight.green(),
                    straight.blue(),
                    straight.alpha(),
                ]);
            }
            Some(IconRasterKind::Color)
        }
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
mod tests {
    use crate::icons::icon_atlas::{IconAtlas, IconDef, IconId};
    use crate::icons::icon_raster_key::IconRasterKey;
    use crate::icons::icon_rasterizer::{IconRasterKind, IconRasterizer};
    use crate::icons::icon_registry::IconSetId;
    use crate::icons::icon_set::IconRef;
    use crate::primitives::span::Span;
    use glam::{U16Vec2, Vec2};

    /// A solid black square filling its whole 8x8 viewBox: every pixel is
    /// fully covered, so coverage is exactly 255 everywhere and the expected
    /// output can be written down rather than eyeballed.
    const SOLID: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8"><rect width="8" height="8" fill="#000"/></svg>"##;
    /// Left half opaque red, right half empty — a hand-checkable split, and
    /// the colour path's fixture.
    const HALF: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8"><rect width="4" height="8" fill="#ff0000"/><rect x="4" width="4" height="8" fill="#0000ff" fill-opacity="0.5"/></svg>"##;
    /// Broken markup — the parse must fail once and stay failed.
    const BROKEN: &str = "<svg";

    /// The three fixtures as a set. `leak_from_svgs` derives each one's
    /// viewBox and tintability by parsing it, which is also what makes the
    /// ids below the *name-sorted* order rather than the listed one.
    fn fixtures() -> IconAtlas {
        IconAtlas::from_svgs([("half", HALF), ("solid", SOLID)])
    }

    // Name-sorted, not listed-order.
    const HALF_ID: IconId = IconId(0);
    const SOLID_ID: IconId = IconId(1);

    fn key(icon: IconId, w: u16, h: u16) -> IconRasterKey {
        IconRasterKey {
            icon: IconRef {
                set: IconSetId::new(0, 0),
                icon,
            },
            size: U16Vec2::new(w, h),
        }
    }

    #[test]
    fn tintable_icon_rasterizes_to_full_coverage_at_the_exact_size() {
        let (mut r, atlas) = (IconRasterizer::default(), fixtures());
        let mut out = Vec::new();
        assert_eq!(
            r.rasterize(&atlas, key(SOLID_ID, 5, 5), &mut out),
            Some(IconRasterKind::Mask),
        );
        // One coverage byte per pixel of the box asked for — not of the
        // 8x8 viewBox, which is the whole point of rasterizing on demand.
        assert_eq!(out.len(), 25);
        assert!(
            out.iter().all(|&c| c == 255),
            "a rect covering its whole viewBox is fully opaque everywhere, got {out:?}",
        );

        // A second, larger size reuses the parse and produces that many px.
        out.clear();
        r.rasterize(&atlas, key(SOLID_ID, 40, 40), &mut out);
        assert_eq!(out.len(), 1600);
        assert_eq!(r.parsed_count(), 1, "one parse serves every size");
    }

    #[test]
    fn colour_icon_rasterizes_to_straight_srgb_rgba() {
        let (mut r, atlas) = (IconRasterizer::default(), fixtures());
        let mut out = Vec::new();
        assert_eq!(
            r.rasterize(&atlas, key(HALF_ID, 8, 2), &mut out),
            Some(IconRasterKind::Color),
        );
        assert_eq!(out.len(), 8 * 2 * 4);
        // Left half: opaque red. Right half: blue at 50% — and stored
        // *straight*, so blue reads 255 rather than the 128 a premultiplied
        // buffer would hold. That distinction is the whole reason the
        // rasterizer demultiplies before handing pixels to the atlas.
        assert_eq!(&out[0..4], &[255, 0, 0, 255], "left half is opaque red");
        assert_eq!(&out[12..16], &[255, 0, 0, 255]);
        let right = &out[16..20];
        assert_eq!(right[0], 0, "no red on the right");
        assert_eq!(right[2], 255, "blue is straight, not premultiplied by 0.5");
        assert!(
            (127..=128).contains(&right[3]),
            "fill-opacity 0.5 is alpha 127 or 128, got {}",
            right[3],
        );
    }

    /// `leak_from_svgs` drops a source that will not parse, so reaching this
    /// path takes a hand-built set — which a baked one effectively is. The
    /// rasterizer still has to fail *once* rather than once per frame.
    #[test]
    fn unparseable_icon_fails_once_and_is_not_retried() {
        static BROKEN_ICONS: [IconDef; 1] = [IconDef {
            name: "broken",
            view_box: Vec2::splat(8.0),
            svg: Span::new(0, BROKEN.len() as u32),
            tintable: true,
            filtered: false,
        }];
        let atlas = IconAtlas::baked(&BROKEN_ICONS, BROKEN.as_bytes());

        let mut r = IconRasterizer::default();
        let mut out = Vec::new();
        assert_eq!(r.rasterize(&atlas, key(IconId(0), 8, 8), &mut out), None);
        assert_eq!(r.rasterize(&atlas, key(IconId(0), 9, 9), &mut out), None);
        assert_eq!(
            r.parsed_count(),
            1,
            "the failure is cached, so a broken icon costs one parse, not one per frame",
        );
    }

    /// Unloading a set drops its parses and nothing else's. This is the
    /// expensive half of the unload: one parsed document per icon the
    /// session drew, held for as long as the set was loaded.
    #[test]
    fn forgetting_a_set_drops_its_parses_and_leaves_its_neighbours() {
        let (mut r, atlas) = (IconRasterizer::default(), fixtures());
        let mut out = Vec::new();
        for set in [0u16, 1] {
            for icon in [HALF_ID, SOLID_ID] {
                let mut k = key(icon, 8, 8);
                k.icon.set = IconSetId::new(set, 0);
                r.rasterize(&atlas, k, &mut out);
            }
        }
        assert_eq!(r.parsed_count(), 4, "two icons in each of two sets");

        r.forget_sets(&[IconSetId::new(0, 0)]);
        assert_eq!(r.parsed_count(), 2, "only set 0's parses go");

        // The generation is part of the identity: the slot's next occupant
        // is not the set that was forgotten.
        r.forget_sets(&[IconSetId::new(1, 1)]);
        assert_eq!(
            r.parsed_count(),
            2,
            "a different generation is a different set"
        );
        r.forget_sets(&[IconSetId::new(1, 0)]);
        assert_eq!(r.parsed_count(), 0);
    }

    /// Several sets released on one frame are forgotten in one walk, and
    /// the batch is what makes that possible: `retain` costs the map's
    /// whole raw table, so a per-set call paid that once per set.
    #[test]
    fn forgetting_a_batch_drops_exactly_its_members() {
        let (mut r, atlas) = (IconRasterizer::default(), fixtures());
        let mut out = Vec::new();
        for set in [0u16, 1, 2] {
            let mut k = key(SOLID_ID, 8, 8);
            k.icon.set = IconSetId::new(set, 0);
            r.rasterize(&atlas, k, &mut out);
        }
        assert_eq!(r.parsed_count(), 3);

        r.forget_sets(&[IconSetId::new(0, 0), IconSetId::new(2, 0)]);
        assert_eq!(r.parsed_count(), 1, "both named sets go, in one pass");
        // And it is set 1 that survived, not whichever was cheapest to keep.
        let mut survivor = key(SOLID_ID, 8, 8);
        survivor.icon.set = IconSetId::new(1, 0);
        r.forget_sets(&[survivor.icon.set]);
        assert_eq!(r.parsed_count(), 0);
    }

    /// Non-square boxes must render the artwork stretched to fill them, not
    /// letterboxed — the fit decision was already made upstream, and the
    /// rasterizer's contract is "fill exactly this many pixels".
    #[test]
    fn non_square_box_stretches_rather_than_letterboxing() {
        let (mut r, atlas) = (IconRasterizer::default(), fixtures());
        let mut out = Vec::new();
        r.rasterize(&atlas, key(SOLID_ID, 16, 4), &mut out);
        assert_eq!(out.len(), 64);
        assert!(out.iter().all(|&c| c == 255), "no transparent margin");
    }
}
