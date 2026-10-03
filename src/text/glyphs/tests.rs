use super::*;
use crate::primitives::paint::content_type::ContentType;
use crate::text::shaper::TextShaper;
use glam::UVec2;

/// A real shaper, because the mono fallback shapes no buffers and has no
/// faces to rasterize from — this API is the render side, and there is
/// nothing of it to test against a measurer that only measures.
fn shaper() -> TextShaper {
    TextShaper::new()
}

/// A run lays out one glyph per character, left to right, and lays out the
/// same run identically twice.
///
/// The repeat is the half an external atlas rests on: raster keys are what
/// a caller's atlas keys its entries by, so a run whose keys moved between
/// two identical calls would miss its own cache every frame.
#[test]
fn a_line_lays_out_left_to_right_and_repeats_itself() {
    let shaper = shaper();
    let mut glyphs = shaper.glyphs();
    let font = GlyphFont::new(16.0);

    let mut out = Vec::new();
    glyphs.line("abc", font, 1.0, &mut out);
    assert_eq!(out.len(), 3, "{out:?}");
    // Whole-pixel pen positions: Inter's `a` and `b` at 16 px advance the
    // pen past 9 and then 18, and the fraction goes to the raster key's
    // subpixel bin.
    let xs: Vec<_> = out.iter().map(|glyph| glyph.x).collect();
    assert_eq!(xs, [0, 9, 18], "{out:?}");

    let first: Vec<_> = out.iter().map(|glyph| glyph.raster_key).collect();
    // Into a buffer that already holds the answer: rewritten, not appended.
    glyphs.line("abc", font, 1.0, &mut out);
    let again: Vec<_> = out.iter().map(|glyph| glyph.raster_key).collect();
    assert_eq!(first, again);

    // Different text is different glyphs — the keys are about the glyph and
    // not about the request having been made.
    glyphs.line("xyz", font, 1.0, &mut out);
    let other: Vec<_> = out.iter().map(|glyph| glyph.raster_key).collect();
    assert_ne!(first, other);
}

/// Nothing to lay out lays out nothing, and reaches nowhere.
#[test]
fn an_empty_line_has_no_glyphs_and_no_extent() {
    let shaper = shaper();
    let mut glyphs = shaper.glyphs();
    let font = GlyphFont::new(16.0);

    let mut out = vec![];
    glyphs.line("a", font, 1.0, &mut out);
    assert_eq!(out.len(), 1);

    glyphs.line("", font, 1.0, &mut out);
    assert!(out.is_empty(), "an empty run left glyphs behind: {out:?}");
    assert_eq!(glyphs.measure("", font), Size::ZERO);
}

/// The raster scale changes what is rasterized, not what is laid out.
///
/// Both halves matter to a caller drawing on a HiDPI surface: it wants the
/// same glyphs in the same order, rasterized bigger — and it wants the two
/// sizes kept apart in its atlas, which is what the keys differing says.
#[test]
fn scale_changes_the_raster_and_not_the_run() {
    let shaper = shaper();
    let mut glyphs = shaper.glyphs();
    let font = GlyphFont::new(16.0);

    let mut single = Vec::new();
    glyphs.line("abc", font, 1.0, &mut single);
    let mut double = Vec::new();
    glyphs.line("abc", font, 2.0, &mut double);

    assert_eq!(single.len(), double.len());
    for (one, two) in single.iter().zip(&double) {
        assert_ne!(
            one.raster_key, two.raster_key,
            "two scales shared one raster"
        );
    }
    // Laid out twice as far across, because every advance is. The 1× subpixel
    // bins put the pen at 9 + ~0 and 18 + ~3/4, so doubled it is 18 and
    // 37 + ~1/2 — the bin the 2× key carries.
    let xs: Vec<_> = double.iter().map(|glyph| glyph.x).collect();
    assert_eq!(xs, [0, 18, 37], "{double:?}");

    // The measured extent is the run's own, in logical pixels — the raster
    // scale cannot reach it, which is why it takes none. What it *is* is
    // the advance, so the last glyph starts inside it: that is the whole
    // reason a caller anchors from `measure` and positions from `line`,
    // and the pair is only usable together while it holds.
    let measured = glyphs.measure("abc", font);
    assert_eq!(
        measured,
        Size::new(28.0, 16.0),
        "\"abc\" ceiled to whole pixels, one 16 px line tall",
    );
    assert!(
        (single[2].x as f32) < measured.w,
        "the last glyph starts inside the run's 28 px",
    );
}

/// A laid-out glyph rasterizes to a bitmap of exactly the size it claims.
///
/// The end-to-end check on the pair: a key that came out of a layout goes
/// back in and produces ink, which is the whole of what a caller's atlas
/// needs and the one thing neither half can be asked on its own.
#[test]
fn a_placed_glyph_rasterizes_to_the_bitmap_it_describes() {
    let shaper = shaper();
    let mut glyphs = shaper.glyphs();

    let mut out = Vec::new();
    glyphs.line("A", GlyphFont::new(32.0), 1.0, &mut out);
    let [placed] = out[..] else {
        panic!("one letter laid out as {out:?}");
    };

    let image = glyphs
        .rasterize(placed.raster_key)
        .expect("a capital A has an image");
    assert_eq!(image.content, ContentType::Mask);
    // Inter's capital A at 32 px: a 22 × 24 ink box.
    assert_eq!(image.size, UVec2::new(22, 24));
    // One byte of coverage per pixel, and the rows tightly packed — which
    // is what a caller blits into its atlas on.
    assert_eq!(image.data.len(), (image.size.x * image.size.y) as usize);
    assert_eq!(
        image.data.iter().max(),
        Some(&255),
        "the glyph's interior is fully covered"
    );
}
