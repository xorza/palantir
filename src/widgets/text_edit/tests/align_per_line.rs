//! Per-line halign tests use real cosmic shaping (`ui_with_text`).
//!
//! These assert the *block-local* half of alignment. A shaped run is
//! measured and reported as a block whose own left edge is x = 0; the
//! owner then places that block inside its rect with the same halign
//! (`TextGeometry::block_offset`, and `Align::place_in` for encoder-placed
//! text). So halign shows up here only as the offset of a *narrow* line
//! relative to the widest one — never as an offset of the block itself,
//! which would be the same alignment applied twice.

use crate::text::font_family::FontFamily;
use crate::text::font_slant::FontSlant;
use crate::text::font_weight::FontWeight;
use crate::text::glyph_font::GlyphFont;
use crate::text::key::LineAlign;
use crate::text::request::internals::TestShape;
use crate::text::shaper::TextShaper;
use crate::widgets::text_edit::tests::*;
use crate::{Align, HAlign};

const FS: f32 = 16.0;
const LH: f32 = 19.2;

fn cosmic_ui() -> UiHarness {
    UiHarness::with_text(UVec2::new(800, 200))
}

fn shape(wrap: f32, halign: HAlign) -> TestShape {
    TestShape::new(GlyphFont {
        size: FS,
        line_height: LH,
        family: FontFamily::SANS,
        weight: FontWeight::REGULAR,
        slant: FontSlant::Normal,
    })
    .width(wrap)
    .halign(halign)
}

const ALL: [HAlign; 3] = [HAlign::Left, HAlign::Center, HAlign::Right];

/// A single-line run fills its own block, so its caret is at the same
/// block-local x under every halign — the alignment is entirely the
/// owner's placement of that block.
///
/// Was the reverse before the block/placement split: cosmic baked
/// `(wrap - line_w) * factor` into the glyphs, so the caret moved with
/// halign *and* the owner moved the block again, aligning twice.
#[test]
fn a_single_line_caret_is_halign_independent() {
    let ui = cosmic_ui();
    let text = "hi";
    let xs: Vec<f32> = ALL
        .iter()
        .map(|&halign| {
            ui.ui
                .shaper()
                .cursor_xy(text, text.len(), shape(300.0, halign))
                .x
        })
        .collect();
    for (halign, x) in ALL.iter().zip(&xs) {
        assert_eq!(
            *x, xs[0],
            "{halign:?} caret {x} must match Left {} on a single line",
            xs[0]
        );
    }
    // …and it is the run's own width, not the wrap target.
    let measured = ui.ui.shaper().measure(text, shape(300.0, HAlign::Right));
    // The caret sits at the glyphs' own extent, h 9.4609375 + i 3.875,
    // nowhere near the 300 px wrap target. The block is that extent
    // ceiled to whole pixels.
    assert_eq!(xs[0], 9.4609375 + 3.875);
    assert_eq!(measured.extent.size.w, xs[0].ceil());
}

/// Halign *does* move a line that is narrower than the widest one:
/// that offset is internal to the block and cannot be recovered by
/// placing the block. Right pushes the short line to the block's right
/// edge, Center to half the slack, Left not at all.
#[test]
fn a_narrow_line_shifts_within_the_block() {
    let ui = cosmic_ui();
    // Two hard-broken lines: "i" is far narrower than "wwwwww".
    let text = "wwwwww\ni";
    let wrap = 300.0;
    let block = ui
        .ui
        .shaper()
        .measure(text, shape(wrap, HAlign::Right))
        .extent
        .size
        .w;
    let caret = |halign| {
        ui.ui
            .shaper()
            .cursor_xy(text, text.len(), shape(wrap, halign))
            .x
    };
    let (left, center, right) = (
        caret(HAlign::Left),
        caret(HAlign::Center),
        caret(HAlign::Right),
    );
    // Left leaves the short line at the block's left edge, so its
    // end-caret is the line's own width, the 3.875 px `i`. Right takes it
    // to the block's right edge — the block, not the 300 px wrap target:
    // six 13.09375 px `w`s, which the measured block ceils to 79. Center
    // splits the slack.
    assert_eq!(left, 3.875);
    assert_eq!(right, 6.0 * 13.09375);
    assert_eq!(block, right.ceil());
    assert_eq!(center, f32::midpoint(left, right));
}

/// Measured width is the glyphs' own extent under every halign. It
/// used to be the distance from x = 0 to the rightmost glyph, which
/// for a right-aligned run is the whole wrap target — so a hugging
/// owner inflated to the full offered width and its damage rect with
/// it.
#[test]
fn measured_width_is_the_content_extent_not_the_wrap_target() {
    let c = TextShaper::new();
    let wrap = 290.0_f32;
    let widths: Vec<f32> = ALL
        .iter()
        .map(|&halign| c.measure("hi\nyo", shape(wrap, halign)).extent.size.w)
        .collect();
    for (halign, w) in ALL.iter().zip(&widths) {
        assert_eq!(
            *w, widths[0],
            "{halign:?} measured {w} must match Left {}",
            widths[0]
        );
    }
    assert_eq!(
        widths[0], 19.0,
        "the wider line, \"yo\", ceiled — not the {wrap} px wrap target",
    );
}

/// An empty buffer has a zero-width block, so its caret is at 0 for
/// every halign — the owner's placement of that empty block is what
/// puts it on the correct edge.
#[test]
fn an_empty_buffer_caret_is_at_the_block_origin() {
    let ui = cosmic_ui();
    for halign in ALL {
        let x = ui.ui.shaper().cursor_xy("", 0, shape(300.0, halign)).x;
        assert_eq!(x, 0.0, "{halign:?} empty caret");
    }
}

#[test]
fn cache_key_distinguishes_halign() {
    // Cosmic shapes a different buffer for each per-line align.
    // The cache key must reflect that so two simultaneous lookups
    // (e.g. caret then selection) can't pick up the wrong buffer.
    let c = TextShaper::new();
    let l = c
        .measure(
            "hi",
            TestShape {
                font: GlyphFont {
                    size: 16.0,
                    line_height: 19.2,
                    family: FontFamily::SANS,
                    weight: FontWeight::REGULAR,
                    slant: FontSlant::Normal,
                },
                max_width: Some(100.0),
                halign: HAlign::Left,
            },
        )
        .buffer_key();
    let r = c
        .measure(
            "hi",
            TestShape {
                font: GlyphFont {
                    size: 16.0,
                    line_height: 19.2,
                    family: FontFamily::SANS,
                    weight: FontWeight::REGULAR,
                    slant: FontSlant::Normal,
                },
                max_width: Some(100.0),
                halign: HAlign::Right,
            },
        )
        .buffer_key();
    assert_ne!(l, r, "halign must enter the cache key");
    assert_ne!(
        l.line_align(),
        r.line_align(),
        "the align is the discriminating field",
    );
}

#[test]
fn unbounded_halign_collapses_to_auto_in_key() {
    // Without a wrap target cosmic can't apply per-line align,
    // so every halign value at `max_width = None` shapes the
    // same buffer. Key construction collapses `halign_q` to `Auto`'s
    // discriminant on that path so single-line callers don't
    // pay an N-way cache split for identical glyph positions.
    let c = TextShaper::new();
    let left = c
        .measure(
            "hi",
            TestShape {
                font: GlyphFont {
                    size: 16.0,
                    line_height: 19.2,
                    family: FontFamily::SANS,
                    weight: FontWeight::REGULAR,
                    slant: FontSlant::Normal,
                },
                max_width: None,
                halign: HAlign::Left,
            },
        )
        .buffer_key();
    let right = c
        .measure(
            "hi",
            TestShape {
                font: GlyphFont {
                    size: 16.0,
                    line_height: 19.2,
                    family: FontFamily::SANS,
                    weight: FontWeight::REGULAR,
                    slant: FontSlant::Normal,
                },
                max_width: None,
                halign: HAlign::Right,
            },
        )
        .buffer_key();
    assert_eq!(left, right, "halign must not split the unbounded cache");
    assert_eq!(
        left.line_align(),
        LineAlign::Auto,
        "unbounded entries always carry the Auto discriminant",
    );
}

/// Regression: a multi-line editor whose content fits within the
/// wrap target (every `\n`-separated line shorter than inner width)
/// must still shape its rendered buffer through the wrap path so
/// cosmic bakes per-line `set_align` offsets. Without this the
/// widget's `cursor_xy` reads from an aligned cache entry while
/// the encoder paints from an unaligned one — caret looks right-
/// aligned but glyphs sit at x = 0.
#[test]
fn rendered_buffer_uses_per_line_align_even_when_content_fits() {
    use crate::scene::layer::Layer;
    let mut h = cosmic_ui();
    let mut buf = String::from("hi\nyo");
    let mut record = |ui: &mut Ui| {
        Panel::hstack()
            .auto_id()
            .show(ui, |ui| {
                TextEdit::new(&mut buf)
                    .id(WidgetId::from_hash("fits-ml"))
                    .multiline(true)
                    .text_align(Align::TOP_RIGHT)
                    .size((Sizing::fixed(300.0), Sizing::fixed(120.0)))
                    .show(ui)
                    .response
                    .node()
            })
            .inner
    };
    // Two frames — first warms up `response.rect`, second is the
    // one we inspect.
    h.frame_value(&mut record);
    let node = h.frame_value(&mut record);
    // `text_spans[node]` indexes one entry per `ShapeRecord::Text`
    // on the node; a multi-line field emits a single text shape, and
    // records it on the block child that carries its alignment.
    let node = block_of(&h.ui, node);
    let main = h.ui.layout(Layer::Main);
    let span = main.text_spans[node.idx()];
    assert_eq!(span.len, 1, "one Shape::Text expected on the block");
    let key = main.text_shapes[span.start as usize].buffer_key();
    // Pinned on the decoded value, so a shifted packing or a
    // reordered variant trips here instead of silently falling
    // through.
    assert_eq!(
        key.line_align(),
        LineAlign::Right,
        "rendered buffer must carry the user's halign in its cache key (got {:?})",
        key.line_align(),
    );
    // Also check the wrap-target axis is set — without a committed
    // width cosmic applies no per-line align at all.
    assert!(
        key.max_width().is_some(),
        "rendered buffer must have a finite wrap target so cosmic per-line align fires",
    );
}

/// Regression: `LayoutEngine::shape_text` always re-shapes through the
/// bounded path for `TextWrap::Wrap` (item 4 in the
/// per-line-align review). With the slot cache keyed on width and
/// halign, the layout pipeline must hit that
/// cache on every steady-state frame — otherwise we'd reshape
/// on every frame and the per-frame text path becomes O(n) in
/// glyph count.
///
/// Direct layout inspection increments `measure_calls` even on a
/// cosmic-cache hit. Check the delta across consecutive stable frames
/// so an extra layout-engine reshape cannot hide in the aggregate.
#[test]
fn stable_multiline_holds_constant_per_frame_cost() {
    let mut h = cosmic_ui();
    let mut buf = String::from("hi\nyo");
    let mut record = |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(&mut buf)
                .id(WidgetId::from_hash("stable-ml"))
                .multiline(true)
                .text_align(Align::TOP_RIGHT)
                .size((Sizing::fixed(300.0), Sizing::fixed(120.0)))
                .show(ui);
        });
    };
    // Warmup: two frames so `response.rect` lands and every cache
    // is primed.
    h.prime(2, &mut record);
    let a = h.ui.shaper().measure_calls();
    h.frame(&mut record);
    let b = h.ui.shaper().measure_calls();
    let per_frame = b - a;
    // Drive several more frames with identical inputs and verify
    // each one costs exactly the same number of `measure_calls`.
    for i in 0..5 {
        let before = h.ui.shaper().measure_calls();
        h.frame(&mut record);
        let after = h.ui.shaper().measure_calls();
        assert_eq!(
            after - before,
            per_frame,
            "frame {i}: per-frame measure cost changed (baseline {per_frame}, this frame {})",
            after - before,
        );
    }
}

/// End-to-end: empty + unfocused multi-line editor with a long
/// placeholder + `text_align(RIGHT)`. The widget renders the
/// *placeholder string* through the layout pipeline so cosmic
/// per-line-aligns each visual line of the placeholder. Pins:
/// (a) the rendered `Shape::Text` carries `align = TOP_RIGHT`,
/// (b) the cached buffer key carries `halign_q = Right`,
/// (c) `max_w_q` is finite (cosmic actually got a wrap target).
/// Without these, the placeholder would shape with `HAlign::Auto`
/// and render left-aligned regardless of `text_align`.
#[test]
fn placeholder_per_line_aligns_under_wrap() {
    use crate::scene::layer::Layer;
    use crate::shape::record::ShapeRecord;
    let mut h = cosmic_ui();
    let mut buf = String::new();
    let mut record = |ui: &mut Ui| {
        Panel::hstack()
            .auto_id()
            .show(ui, |ui| {
                TextEdit::new(&mut buf)
                    .id(WidgetId::from_hash("ph-ml"))
                    .multiline(true)
                    .text_align(Align::TOP_RIGHT)
                    .placeholder("type a paragraph here — long enough to actually wrap")
                    .size((Sizing::fixed(300.0), Sizing::fixed(120.0)))
                    .show(ui)
                    .response
                    .node()
            })
            .inner
    };
    h.frame_value(&mut record);
    let node = h.frame_value(&mut record);
    // (a) `Shape::Text.align` reflects the user's text_align.
    let store = h.ui.record_store();
    let interned_text = store.interned_text();
    let node = block_of(&h.ui, node);
    let tree = h.ui.tree(Layer::Main);
    let shape_align = tree.shapes_of(node).find_map(|s| match s {
        ShapeRecord::Text { align, text, .. } => {
            Some((*align, interned_text.resolve(text.span).to_owned()))
        }
        _ => None,
    });
    let (shape_align, shape_text) = shape_align.expect("placeholder paints as Shape::Text");
    assert_eq!(shape_align, Align::TOP_RIGHT);
    assert!(
        shape_text.contains("type a paragraph"),
        "rendered text must be the placeholder, got {shape_text:?}",
    );
    // (b) + (c) cached buffer key.
    let main = h.ui.layout(Layer::Main);
    let span = main.text_spans[node.idx()];
    assert_eq!(span.len, 1, "one Shape::Text expected on the block");
    let key = main.text_shapes[span.start as usize].buffer_key();
    assert_eq!(
        key.line_align(),
        LineAlign::Right,
        "placeholder buffer must carry the user's halign in its cache key",
    );
    assert!(
        key.max_width().is_some(),
        "placeholder buffer must have a finite wrap target so cosmic per-line align fires",
    );
}

/// End-to-end: the widget still right-aligns each line on screen.
/// Drives a real frame so the block placement and the block-local
/// offset compose the way they do in a running app.
#[test]
fn multiline_widget_right_aligns_each_line() {
    let mut h = cosmic_ui();
    let id = WidgetId::from_hash("ml-right");
    let mut buf = String::from("short\na much longer line here");
    let mut record = |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(&mut buf)
                .id(WidgetId::from_hash("ml-right"))
                .multiline(true)
                .text_align(Align::TOP_RIGHT)
                .size((Sizing::fixed(300.0), Sizing::fixed(120.0)))
                .show(ui);
        });
    };
    h.frame(&mut record);
    h.ui.with_state::<TextEditState, _>(id, |_, s| s.edit.caret = 5);
    h.frame(&mut record);
    // wrap target = inner width = 300 - 2*5 = 290.
    let wrap = 290.0;
    let block =
        h.ui.shaper()
            .measure(&buf, shape(wrap, HAlign::Right))
            .extent
            .size
            .w;
    let caret_short =
        h.ui.shaper()
            .cursor_xy(&buf, 5, shape(wrap, HAlign::Right))
            .x;
    // "short" is the narrow line, so right-align carries its caret to
    // the block's right edge.
    // The block edge is the long line's extent, 22727/128 px, which the
    // measured block ceils to 178 — far from "short"'s own width.
    assert_eq!(caret_short, 22727.0 / 128.0);
    assert_eq!(block, caret_short.ceil());
}
