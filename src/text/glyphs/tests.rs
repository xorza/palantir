use super::*;
use crate::primitives::paint::content_type::ContentType;
use crate::text::shaper::TextShaper;
use glam::UVec2;

/// A real shaper: the mono fallback shapes no buffers and has no faces to rasterize from.
fn shaper() -> TextShaper {
    TextShaper::new()
}

/// A run lays out one glyph per character, left to right, identically on repeat (an atlas keys by raster key).
#[test]
fn a_line_lays_out_left_to_right_and_repeats_itself() {
    let shaper = shaper();
    let mut glyphs = shaper.glyphs();
    let font = GlyphFont::new(16.0);

    let mut out = Vec::new();
    glyphs.line("abc", font, 1.0, &mut out);
    assert_eq!(out.len(), 3, "{out:?}");
    // Whole-pixel pen positions; the fraction goes to the raster key's subpixel bin.
    let xs: Vec<_> = out.iter().map(|glyph| glyph.x).collect();
    assert_eq!(xs, [0, 9, 18], "{out:?}");

    let first: Vec<_> = out.iter().map(|glyph| glyph.raster_key).collect();
    glyphs.line("abc", font, 1.0, &mut out);
    let again: Vec<_> = out.iter().map(|glyph| glyph.raster_key).collect();
    assert_eq!(first, again);

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

/// The raster scale changes what is rasterized, not what is laid out; the keys differ so an atlas keeps sizes apart.
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
    // Doubled advances: the 1× subpixel bins put the pen at 9 + ~0 and 18 + ~3/4, so 18 and 37 + ~1/2 at 2×.
    let xs: Vec<_> = double.iter().map(|glyph| glyph.x).collect();
    assert_eq!(xs, [0, 18, 37], "{double:?}");

    // The measured extent is in logical pixels, untouched by raster scale, and the last glyph starts inside it.
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

/// A laid-out glyph rasterizes to a bitmap of exactly the size it claims (layout key in, ink out).
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
    assert_eq!(image.size, UVec2::new(22, 24));
    assert_eq!(image.data.len(), (image.size.x * image.size.y) as usize);
    assert_eq!(
        image.data.iter().max(),
        Some(&255),
        "the glyph's interior is fully covered"
    );
}
