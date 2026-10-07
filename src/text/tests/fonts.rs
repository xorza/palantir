//! Font registration and family resolution: what a load adds, what an
//! unknown family falls back to, and what the axes shape against.

use super::*;
use crate::Ui;
use crate::common::clipboard::Clipboard;
use crate::internals::harness::UiHarness;
use crate::renderer::texture_limit::TextureLimit;
use crate::text::error::FontLoadError;
use crate::text::font_scope::internals::{INTER, MONO};
use crate::ui::frame_report::FramePaint;
use crate::ui::resources::UiResources;
use crate::widget_core::configure::Configure;
use crate::widgets::text::Text;
use crate::widgets::theme::text_style::TextStyle;
use glam::UVec2;

// No face file is unregistered by the bundled scope, so the load cases start
// from an empty database and introduce a bundled face.

/// An unanswered family shapes in the bundled default, never in whatever the
/// machine has installed; `has_font` lets an app ask in advance.
#[test]
fn an_unknown_family_resolves_to_the_bundled_default() {
    let mut c = CosmicMeasure::default();
    let missing = FontFamily::named("No Such Family Exists").unwrap();

    assert!(!c.has_font(missing));
    assert!(c.has_font(FontFamily::SANS));
    assert!(c.has_font(FontFamily::MONO));

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

/// A load makes its family resolvable and returns the family of the first face
/// it registered.
#[test]
fn a_late_load_makes_its_family_resolvable() {
    let mut c = CosmicMeasure::with_no_fonts();
    // `SANS` can't be tested by what a family shapes under: it shapes under its own
    // name either way, being the fallback.
    assert!(
        !c.has_font(FontFamily::SANS),
        "the fixture starts with no faces at all",
    );

    let loaded = c.load_font(INTER.into()).expect("the bundled Inter loads");
    assert_eq!(loaded, FontFamily::SANS);
    assert_eq!(loaded.name(), "Inter");
    assert!(c.has_font(FontFamily::SANS));
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

/// The encoded-run cache holds templates rasterized from whichever face
/// resolved, so a load must be visible to it through an epoch readable without
/// borrowing the shaper.
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

/// A load reaches the frame, not only the text caches. Two facts: `response_for`
/// takes the entry rect from the cascade and the arranged rect from `Layout`,
/// assuming the cascade rebuilds whenever an arranged rect moves, so a load
/// that moves one must reach the cascade key; and a glyph redrawn in a new face
/// inside an unmoved rect is invisible to per-widget diffs, so the load frame
/// repaints in full.
#[test]
fn a_load_reaches_the_cascade_and_the_screen() {
    let shaper = TextShaper::over(CosmicMeasure::with_no_fonts());
    let mut h = UiHarness::from_resources(
        UiResources::new(shaper, Clipboard::memory(), TextureLimit::default()),
        UVec2::new(400, 300),
    );
    h.ui.load_font(INTER).expect("the bundled Inter loads");
    // `i` is where a proportional and a fixed-advance face disagree most; monospace
    // resolves to Inter until the load below.
    let id = WidgetId::from_hash("label");
    let record = |ui: &mut Ui| {
        Text::new("iiiiiiii")
            .id(id)
            .style(&TextStyle {
                family: FontFamily::MONO,
                ..TextStyle::default().with_font_size(16.0)
            })
            .show(ui);
    };
    h.prime(2, record);
    let fallback = h.rect(id).expect("the label arranged");
    assert_eq!(
        h.layout_rect(id),
        Some(fallback),
        "premise: a warm frame's two rects describe one arrangement",
    );

    h.ui.load_font(MONO)
        .expect("the bundled JetBrains Mono loads");
    let report = h.frame(record);
    let resolved = h.rect(id).expect("the label arranged");
    assert!(
        resolved.size.w > fallback.size.w,
        "the run must re-measure in the face that just arrived: \
         {} wide before, {} after",
        fallback.size.w,
        resolved.size.w,
    );
    assert_eq!(
        h.layout_rect(id),
        Some(resolved),
        "the cascade skipped a load that moved an arranged rect, so the \
         response geometry and the layout disagree",
    );
    assert_eq!(
        report.paint(),
        FramePaint::Full,
        "a load repaints every glyph it may have changed",
    );
}

/// A load changes what a run measures to **without changing its key**, so
/// layout-side rows must be told out of band. A reuse row is validated against a
/// [`TextShapeKey`] carrying the family *index*, not the resolved face, so a run
/// that fell back keeps a byte-identical key when a face arrives, every
/// freshness check passes, and the row keeps its pre-load width while the
/// renderer paints the new face in the old box.
#[test]
fn a_load_retires_the_reuse_rows_measured_before_it() {
    // A database holding Inter alone, so monospace is a real fallback before the
    // load and itself after. `i` differs most: narrow proportional in Inter, the
    // same fixed advance as other glyphs in JetBrains Mono.
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

/// Weight is an axis: Inter is one variable file, and each step must
/// instantiate a visibly different `wght`.
#[test]
fn the_weight_axis_is_monotonic_on_a_variable_face() {
    let mut c = CosmicMeasure::default();
    // A long run at a large size: extents are ceiled, and one glyph of one weight
    // step may not cross a pixel.
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

/// Italic is a separate axis from weight and reaches a different physical file,
/// shown only by PostScript name since both answer to "Inter".
#[test]
fn italic_reaches_the_italic_file_at_every_weight() {
    let mut c = CosmicMeasure::default();
    for weight in [FontWeight::REGULAR, FontWeight::BOLD] {
        let upright = GlyphFont {
            weight,
            ..GlyphFont::new(16.0)
        };
        let italic = GlyphFont {
            slant: FontSlant::Italic,
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

/// The family index round-trips through the packed key untouched over the whole
/// range. `u16::MAX` is past anything interned: the key is a carrier, and
/// resolution happens later at `has_font`.
#[test]
fn the_key_carries_any_family_index() {
    for raw in [0, 1, 2, u16::MAX] {
        let face = GlyphFont {
            family: FontFamily::from_raw(raw),
            ..GlyphFont::new(16.0)
        };
        let key = TextShapeKey::for_text("hi", face).expect("a fixture face is usable");
        assert_eq!(key.family().raw(), raw);
        // The neighbours in the packed word must survive it.
        assert_eq!(key.weight(), FontWeight::REGULAR);
        assert_eq!(key.slant(), FontSlant::Normal);
        assert_eq!(key.line_align(), LineAlign::Auto);
        assert_eq!(key.fit(), LineFit::Wrap);
    }
}

/// Every axis in the packed face word survives being written beside the others,
/// including the two a committed width rewrites.
#[test]
fn the_packed_face_word_keeps_every_axis_apart() {
    let face = GlyphFont {
        family: FontFamily::MONO,
        weight: FontWeight::new(950),
        slant: FontSlant::Italic,
        ..GlyphFont::new(16.0)
    };
    let unbounded = TextShapeKey::for_text("hi", face).expect("a fixture face is usable");
    let bound = unbounded.with_bound(WrapBound::new(120.0, HAlign::Right, LineFit::Wrap));

    for (label, key) in [("unbounded", unbounded), ("bound", bound)] {
        assert_eq!(key.family(), FontFamily::MONO, "{label}");
        assert_eq!(key.weight(), FontWeight::new(950), "{label}");
        assert_eq!(key.slant(), FontSlant::Italic, "{label}");
    }
    assert_eq!(unbounded.line_align(), LineAlign::Auto);
    assert_eq!(unbounded.fit(), LineFit::Wrap);
    assert_eq!(bound.line_align(), LineAlign::Right);
    assert_eq!(bound.fit(), LineFit::Wrap);
    assert_eq!(
        bound.unbounded_version(),
        unbounded,
        "dropping the bound must restore the key the face alone mints",
    );
}

/// A load reports its failure by what the file named: no names is no face; names
/// none of which fit the family table is a full table. The first that fits is
/// the load's family.
#[test]
fn a_load_names_its_family_or_why_it_has_none() {
    use crate::text::cosmic::first_family;
    use crate::text::font_family::FontFamily;

    let none = |_: &str| None;
    assert!(matches!(
        first_family(&[], none),
        Err(FontLoadError::NoFaces)
    ));
    let names = ["Full".to_owned(), "Also Full".to_owned()];
    assert!(matches!(
        first_family(&names, none),
        Err(FontLoadError::FamilyTableFull)
    ));
    let second = |name: &str| (name == "Also Full").then_some(FontFamily::MONO);
    assert_eq!(first_family(&names, second).ok(), Some(FontFamily::MONO));
}
