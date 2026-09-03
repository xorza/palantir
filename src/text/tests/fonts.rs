//! Font registration and family resolution: what a load adds, what an
//! unknown family falls back to, and what the axes shape against.

use super::*;
use crate::text::error::FontLoadError;
use crate::text::font_scope::test_support::{INTER, MONO};
use crate::text::font_style::FontStyle;

// A face file the bundled scope has *not* already registered is what a
// load case would rather have, and there is none — so the cases below
// start from an empty database and introduce a bundled face to it.

/// A family no face answers to shapes in the bundled default, never in
/// whatever the machine happens to have installed.
///
/// The look of an app must not depend on its host's font directory, so
/// the fallback is a family this crate ships. `font_available` is how an
/// app asks in advance rather than discovering it by eye.
#[test]
fn an_unknown_family_resolves_to_the_bundled_default() {
    let mut c = CosmicMeasure::default();
    let missing = FontFamily::named("No Such Family Exists");

    assert!(!c.font_available(missing));
    assert!(c.font_available(FontFamily::SANS));
    assert!(c.font_available(FontFamily::MONO));

    let face = GlyphFont {
        family: missing,
        ..GlyphFont::new(16.0)
    };
    assert_eq!(
        c.resolved_family("M", face).as_deref(),
        Some("Inter"),
        "an unknown family must shape in the bundled default",
    );
}

/// A load makes its family resolvable where it was not, and hands back
/// the family of the first face it registered.
#[test]
fn a_late_load_makes_its_family_resolvable() {
    let mut c = CosmicMeasure::with_no_fonts();
    // `SANS` is the case availability cannot answer by asking what a
    // family shapes under: it shapes under its own name either way,
    // because it is what everything else falls back to.
    assert!(
        !c.font_available(FontFamily::SANS),
        "the fixture starts with no faces at all",
    );

    let loaded = c.load_font(INTER.into()).expect("the bundled Inter loads");
    assert_eq!(loaded, FontFamily::SANS);
    assert_eq!(loaded.name(), "Inter");
    assert!(c.font_available(FontFamily::SANS));
    assert_eq!(
        c.resolved_family("M", GlyphFont::new(16.0)).as_deref(),
        Some("Inter"),
        "the family must shape against the face just registered",
    );
}

/// Both untrusted-input arms report which one failed.
#[test]
fn a_load_that_cannot_produce_a_face_says_which_way_it_failed() {
    let mut c = CosmicMeasure::with_no_fonts();

    let not_a_font = c.load_font(b"this is not a font file".into());
    assert!(
        matches!(not_a_font, Err(FontLoadError::NoFaces)),
        "bytes that parse to no face are NoFaces, got {not_a_font:?}",
    );

    let missing_file = c.load_font("/nonexistent/palantir-test-font.ttf".into());
    assert!(
        matches!(missing_file, Err(FontLoadError::Io { .. })),
        "an unreadable path is Io, not NoFaces, got {missing_file:?}",
    );
}

/// The renderer's encoded-run cache holds templates rasterized from
/// whatever face resolved when they were encoded, so a load has to be
/// visible to it — through an epoch it can read without borrowing the
/// shaper, which an all-hit frame must not do.
#[test]
fn a_load_bumps_the_epoch_the_renderer_watches() {
    let shaper = TextShaper::new();
    let before = shaper.font_epoch();

    shaper.load_font(INTER).expect("the bundled Inter loads");
    assert_eq!(shaper.font_epoch(), before + 1);

    assert!(
        shaper.load_font(b"still not a font").is_err(),
        "the fixture's second load must fail for the assertion below to mean anything",
    );
    assert_eq!(
        shaper.font_epoch(),
        before + 1,
        "a failed load changes no face, so it must not invalidate anything",
    );
}

/// A load changes what a run measures to **without changing its key**,
/// so the layout-side rows have to be told out of band.
///
/// A reuse row is addressed by `(WidgetId, ordinal)` and validated
/// against a [`TextShapeKey`], which carries the family *index* — never
/// the face that index resolves to. So a run that fell back to the
/// bundled default keeps a byte-identical key once a face answering to
/// its family arrives, every freshness check in `TextSystem` passes, and
/// the row goes on reporting the width it measured before the load. The
/// shaped buffers are already gone by then, so the renderer reshapes in
/// the new face and paints it inside the old box.
#[test]
fn a_load_retires_the_reuse_rows_measured_before_it() {
    // A database holding Inter alone, so the monospace family below is a
    // real fallback before the load and itself after it. `i` is where the
    // two disagree most: Inter gives it a narrow proportional advance,
    // JetBrains Mono the same fixed advance as every other glyph.
    let shaper = TextShaper::over(CosmicMeasure::with_no_fonts());
    shaper.load_font(INTER).expect("the bundled Inter loads");
    let mut text = TextSystem::new(shaper.clone());
    let run = slot(WidgetId::from_hash("label"));
    let face = shape(16.0).family(FontFamily::MONO);

    let fallback = text.shape_run(run, "iiiiiiii", face, TextWrap::SingleLine);
    assert!(text.has_entry(run.widget_id, run.ordinal));
    assert!(
        !text.sync_fonts(),
        "no load since construction: nothing to retire",
    );

    shaper
        .load_font(MONO)
        .expect("the bundled JetBrains Mono loads");
    assert!(text.sync_fonts(), "the load has to be reported once");
    assert_eq!(
        text.entry_count(),
        0,
        "a row measured against the old database answers for nothing now",
    );
    assert!(
        !text.sync_fonts(),
        "and reported once only — the next frame has nothing to retire",
    );

    let resolved = text.shape_run(run, "iiiiiiii", face, TextWrap::SingleLine);
    assert!(
        resolved.size.w > fallback.size.w,
        "the same run must remeasure in the registered face: eight fixed \
         advances are wider than eight proportional `i`s, got {} then {}",
        fallback.size.w,
        resolved.size.w,
    );
    assert_eq!(
        resolved.key, fallback.key,
        "the key is what cannot tell the two apart — were it able to, \
         this mechanism would be unnecessary rather than merely untested",
    );
}

/// Weight is an axis, not a pair: Inter is one variable file, and each
/// step must instantiate a visibly different `wght`.
#[test]
fn the_weight_axis_is_monotonic_on_a_variable_face() {
    let mut c = CosmicMeasure::default();
    // A long run at a large size: an extent is ceiled to whole pixels, and
    // one glyph of one weight step does not always cross a pixel.
    let width = |c: &mut CosmicMeasure, weight| {
        c.measure(
            "MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM",
            shape(64.0).weight(weight),
        )
        .size
        .w
    };

    let light = width(&mut c, FontWeight::LIGHT);
    let regular = width(&mut c, FontWeight::REGULAR);
    let bold = width(&mut c, FontWeight::BOLD);
    assert!(
        light < regular && regular < bold,
        "300 < 400 < 700 must widen: {light} / {regular} / {bold}",
    );
}

/// Italic is a separate axis from weight, and it reaches a different
/// physical file — which only the PostScript name can show, since both
/// files answer to the family name "Inter".
#[test]
fn italic_reaches_the_italic_file_at_every_weight() {
    let mut c = CosmicMeasure::default();
    for weight in [FontWeight::REGULAR, FontWeight::BOLD] {
        let upright = GlyphFont {
            weight,
            ..GlyphFont::new(16.0)
        };
        let italic = GlyphFont {
            style: FontStyle::Italic,
            ..upright
        };
        let name = c
            .resolved_post_script_name("M", italic)
            .expect("italic must resolve to a face");
        assert!(
            name.contains("Italic"),
            "{weight:?} italic must reach an italic file, got {name}",
        );
        let upright_name = c
            .resolved_post_script_name("M", upright)
            .expect("upright must resolve to a face");
        assert!(
            !upright_name.contains("Italic"),
            "{weight:?} upright must not, got {upright_name}",
        );
    }
}

/// The family index round-trips through the packed key untouched, over
/// the whole range the field holds.
///
/// `u16::MAX` is past anything the name table has interned, which is the
/// point: the key is a carrier, and the resolution that decides what an
/// index *means* happens later, at `font_available`.
#[test]
fn the_key_carries_any_family_index() {
    for raw in [0, 1, 2, u16::MAX] {
        let face = GlyphFont {
            family: FontFamily::from_raw(raw),
            ..GlyphFont::new(16.0)
        };
        let key = TextShapeKey::for_text("hi", face);
        assert_eq!(key.family_q, raw);
        assert_eq!(key.family().raw(), raw);
        // The neighbours in the packed word must survive it.
        assert_eq!(key.weight(), FontWeight::REGULAR);
        assert_eq!(key.style(), FontStyle::Normal);
        assert_eq!(key.halign(), HAlign::Auto);
        assert_eq!(key.fit(), LineFit::Wrap);
    }
}

/// Every axis the packed face word holds survives being written beside
/// the others, including the two a committed width rewrites.
#[test]
fn the_packed_face_word_keeps_every_axis_apart() {
    let face = GlyphFont {
        family: FontFamily::MONO,
        weight: FontWeight::new(950),
        style: FontStyle::Italic,
        ..GlyphFont::new(16.0)
    };
    let unbounded = TextShapeKey::for_text("hi", face);
    let bound = unbounded.with_bound(WrapBound::new(120.0, HAlign::Right, LineFit::Wrap));

    for (label, key) in [("unbounded", unbounded), ("bound", bound)] {
        assert_eq!(key.family_q, FontFamily::MONO.raw(), "{label}");
        assert_eq!(key.weight(), FontWeight::new(950), "{label}");
        assert_eq!(key.style(), FontStyle::Italic, "{label}");
    }
    assert_eq!(unbounded.halign(), HAlign::Auto);
    assert_eq!(unbounded.fit(), LineFit::Wrap);
    assert_eq!(bound.halign(), HAlign::Right);
    assert_eq!(bound.fit(), LineFit::Wrap);
    assert_eq!(
        bound.unbounded_version(),
        unbounded,
        "dropping the bound must restore the key the face alone mints",
    );
}
