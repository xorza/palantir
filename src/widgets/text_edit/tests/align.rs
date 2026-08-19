//! Tests for `TextEdit::text_align` and the default alignment per
//! mode. Mono fallback (`ui_at_no_cosmic`): 8 px / char @ 16 px font,
//! `LINE_HEIGHT_MULT = 1.2` → canonical line height
//! `round(19.2 × 64) / 64 = 19.203125` px. Editor is 280×40
//! with theme padding (5, 3) plus the 1.5 px chrome stroke that
//! `Tree::open_node` folds into padding (mirrored by TextEdit so
//! glyph/caret coords land on the encoder's clip rect). Stroke width
//! is constant across normal/focused — the only thing focus changes
//! is the color — so the inner rect doesn't shift when the user
//! clicks in. Effective padding is (6.5, 4.5), inner rect 267×31.

use crate::Align;
use crate::primitives::size::Size;
use crate::primitives::translate_scale::TranslateScale;
use crate::scene::layer::Layer;
use crate::scene::shapes::paint::QuadShape;
use crate::scene::shapes::record::ShapeRecord;
use crate::scene::tree::node_id::NodeId;
use crate::shape::rect::RectKind;
use crate::ui::harness::UiHarness;
use crate::widgets::text_edit::tests::*;

const EDIT_W: f32 = 280.0;
const EDIT_H: f32 = 40.0;
/// Theme padding (5, 3) + chrome stroke width (1.5), folded together
/// because the encoder's clip mask is `rect.deflated_by(post-fold
/// padding)` and TextEdit mirrors the fold so its glyph + caret
/// coords match the clip. Constant across normal/focused — stroke
/// color changes on focus, width does not.
const PAD_L: f32 = 6.5;
const PAD_T: f32 = 4.5;
/// Default `TextEditTheme::caret_width` — the widget reserves this much
/// room at every line's trailing edge so a caret on right/center-aligned
/// text stays inside the clip.
const CARET_W: f32 = 1.5;
const INNER_W: f32 = EDIT_W - 2.0 * PAD_L; // 267
const INNER_H: f32 = EDIT_H - 2.0 * PAD_T; // 31
const ALIGN_W: f32 = INNER_W - CARET_W; // 265.5
const LINE_H: f32 = 19.203_125;
const TEXT_W_4CH: f32 = 32.0; // mono "abcd" width

/// Drive one frame of a single-line editor at `text_align` + buffer +
/// optional placeholder, returning the field's `NodeId` so the caller
/// can read shapes back.
///
/// One frame is enough for alignment — the engine places the block against the
/// rect it has just arranged, so there is nothing stale to warm up; see
/// [`the_first_frame_aligns_like_the_ones_after_it`]. What still lags a frame
/// is `response.rect` itself, which the hit-test and the scroll read, so tests
/// about *those* go through [`warmup_then`].
fn frame(
    h: &mut UiHarness,
    buf: &mut String,
    text_align: Option<Align>,
    placeholder: Option<&'static str>,
) -> NodeId {
    let mut node: Option<NodeId> = None;
    let mut record = |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            let mut e = TextEdit::new(buf)
                .id(ed_id())
                .size((Sizing::fixed(EDIT_W), Sizing::fixed(EDIT_H)));
            if let Some(a) = text_align {
                e = e.text_align(a);
            }
            if let Some(p) = placeholder {
                e = e.placeholder(p);
            }
            node = Some(e.show(ui).response.node());
        });
    };
    h.frame(&mut record);
    node.unwrap()
}

/// Two-frame helper: the first warms up the cascade so the editor has a real
/// `response.rect`, the second is the one that gets read.
///
/// Alignment no longer needs it — see [`frame`] — and these keep it so that
/// what they assert is the *settled* answer, which is what the first-frame test
/// compares against.
fn warmup_then(
    h: &mut UiHarness,
    buf: &mut String,
    text_align: Option<Align>,
    placeholder: Option<&'static str>,
) -> NodeId {
    frame(h, buf, text_align, placeholder);
    frame(h, buf, text_align, placeholder)
}

/// The id every field below is recorded under.
fn ed_id() -> WidgetId {
    WidgetId::from_hash("align-ed")
}

/// Where the block child was arranged, relative to its field's own corner.
///
/// The block is where the run, the wash and the caret are recorded — see
/// [`PaintInput::record`](crate::widgets::text_edit::paint_input::PaintInput::record) — because *where* it sits inside
/// the inner rect is an alignment, and an alignment is the layout engine's to
/// resolve. Every origin below is asked for in the field's own coordinates,
/// which is this composed with the shape's origin inside the block.
///
/// Taken off the tree rather than off a name the caller passes, so a test that
/// records its field under some other id needs to say nothing about it.
fn block_at(ui: &Ui, field: NodeId) -> glam::Vec2 {
    let tree = &ui.forest.trees[Layer::Main];
    let of = |node: NodeId| {
        ui.response_for(tree.records.widget_id()[node.idx()])
            .layout_rect
            .expect("arranged")
    };
    of(block_of(ui, field)).min - of(field).min
}

/// `(text_origin, caret_origin)` in the field's own coordinates. The paint
/// order is selection-wash → text → caret, so the text shape is the only
/// `Shape::Text` and the caret is the *last* rounded rect with a `local_rect`
/// (selection rects come before the text; the caret comes after — for empty
/// focused editors it's the only rounded rect in the stream).
fn shape_origins(ui: &Ui, node: NodeId) -> (Option<glam::Vec2>, Option<glam::Vec2>) {
    let at = block_at(ui, node);
    let block = block_of(ui, node);
    let tree = &ui.forest.trees[Layer::Main];
    let mut text_origin = None;
    let mut caret_origin = None;
    for s in tree.shapes_of(block) {
        match s {
            ShapeRecord::Text {
                local_origin: Some(o),
                ..
            } => text_origin = Some(*o + at),
            ShapeRecord::Quad(QuadShape::Rect {
                kind: RectKind::Rounded,
                local_rect: Some(r),
                ..
            }) => caret_origin = Some(glam::Vec2::new(r.min.x, r.min.y) + at),
            _ => {}
        }
    }
    (text_origin, caret_origin)
}

/// Emit Shift+ArrowRight as the focused widget would see it.
fn shift_arrow_right(ui: &mut Ui) {
    ui.on_input(InputEvent::ModifiersChanged(Modifiers {
        shift: true,
        ..Modifiers::NONE
    }));
    ui.on_input(InputEvent::KeyDown {
        key: Key::ArrowRight,
        repeat: false,
        physical: Key::Other,
    });
}

/// **The first frame an editor exists on aligns like the ones after it.**
///
/// Where the text block sits inside the inner rect is an alignment, and an
/// alignment wants the rect — which the *record* pass does not have, because
/// arrange has not run. Resolved at record time it read last pass's rect, absent
/// on the frame a field appears: a centred field painted hard left for one
/// frame and snapped across on the next, and the vertical half of it misplaced
/// even a field that asked for nothing.
///
/// The fix is not to guess better but to stop guessing — the block is a child,
/// and the engine places it against the rect it has just arranged. So one
/// `frame` here is enough where every other test in this file warms up first.
///
/// Both axes, and both against the settled answer rather than against numbers
/// written out again: the claim is that the two *agree*, so a test restating
/// the arithmetic could pass with both of them wrong.
#[test]
fn the_first_frame_aligns_like_the_ones_after_it() {
    let settled = {
        let mut h = ui_at_no_cosmic(NARROW);
        let mut buf = String::from("abcd");
        let node = warmup_then(&mut h, &mut buf, Some(Align::CENTER), None);
        shape_origins(&h.ui, node).0.expect("text shape emitted")
    };
    // Centred rather than left, so a zero-sized box is a wrong answer rather
    // than accidentally the right one.
    assert!(settled.x > PAD_L + 1.0, "x = {} is not centred", settled.x);

    let mut h = ui_at_no_cosmic(NARROW);
    let mut buf = String::from("abcd");
    let node = frame(&mut h, &mut buf, Some(Align::CENTER), None);
    let first = shape_origins(&h.ui, node).0.expect("text shape emitted");
    assert!(
        (first.x - settled.x).abs() < 1e-3,
        "first frame painted x = {} where the settled frame paints {}",
        first.x,
        settled.x
    );
    assert!(
        (first.y - settled.y).abs() < 1e-3,
        "first frame painted y = {} where the settled frame paints {}",
        first.y,
        settled.y
    );
}

#[test]
fn single_line_default_is_left_vcenter() {
    // No `.text_align(...)` → mode default `Align::LEFT` (left +
    // vcenter). With "abcd" (32×19.203125) inside the inner rect,
    // dx = 0 and dy = (inner height − line height) / 2.
    let mut h = ui_at_no_cosmic(NARROW);
    let mut buf = String::from("abcd");
    let node = warmup_then(&mut h, &mut buf, None, None);
    let (origin, _) = shape_origins(&h.ui, node);
    let o = origin.expect("text shape emitted for non-empty buffer");
    assert!((o.x - PAD_L).abs() < 1e-3, "x = {}", o.x);
    let dy = (INNER_H - LINE_H) * 0.5;
    assert!((o.y - (PAD_T + dy)).abs() < 1e-3, "y = {}", o.y);
}

#[test]
fn single_line_text_align_table() {
    // Sweep every (HAlign × VAlign) combination on a single-line
    // editor with "abcd". Expected `(dx, dy)` per the encoder
    // convention — overflow clamps to zero, which doesn't fire here
    // because the text fits inside the inner rect minus caret reservation.
    let cx = (ALIGN_W - TEXT_W_4CH) * 0.5; // 118.25
    let rx = ALIGN_W - TEXT_W_4CH; // 236.5
    let cy = (INNER_H - LINE_H) * 0.5;
    let by = INNER_H - LINE_H;
    let cases: &[(Align, f32, f32, &str)] = &[
        (Align::TOP_LEFT, 0.0, 0.0, "TOP_LEFT"),
        (Align::TOP, cx, 0.0, "TOP"),
        (Align::TOP_RIGHT, rx, 0.0, "TOP_RIGHT"),
        (Align::LEFT, 0.0, cy, "LEFT (= default single-line)"),
        (Align::CENTER, cx, cy, "CENTER"),
        (Align::RIGHT, rx, cy, "RIGHT"),
        (Align::BOTTOM_LEFT, 0.0, by, "BOTTOM_LEFT"),
        (Align::BOTTOM, cx, by, "BOTTOM"),
        (Align::BOTTOM_RIGHT, rx, by, "BOTTOM_RIGHT"),
    ];
    for &(align, dx, dy, label) in cases {
        let mut h = ui_at_no_cosmic(NARROW);
        let mut buf = String::from("abcd");
        let node = warmup_then(&mut h, &mut buf, Some(align), None);
        let (origin, _) = shape_origins(&h.ui, node);
        let o = origin.expect("text shape emitted");
        assert!(
            (o.x - (PAD_L + dx)).abs() < 1e-3,
            "{label}: text.x = {} (expected {})",
            o.x,
            PAD_L + dx,
        );
        assert!(
            (o.y - (PAD_T + dy)).abs() < 1e-3,
            "{label}: text.y = {} (expected {})",
            o.y,
            PAD_T + dy,
        );
    }
}

#[test]
fn caret_tracks_aligned_text() {
    // Focus + caret at end of "abcd". With HAlign::Right the text
    // origin shifts right by `ALIGN_W − TEXT_W_4CH`; the caret must
    // shift by the same dx so it sits at the rightmost glyph trailing
    // edge, leaving `CARET_W` of reserved room before the clip edge.
    let mut h = ui_at_no_cosmic(NARROW);
    let mut buf = String::from("abcd");
    // Warmup so response.rect lands; click; then a final frame so
    // the post-click focus state drives a caret render with the
    // resolved align offset.
    frame(&mut h, &mut buf, Some(Align::RIGHT), None);
    h.click_at(glam::Vec2::new(260.0, 20.0));
    h.key(Key::End);
    frame(&mut h, &mut buf, Some(Align::RIGHT), None);
    let node = frame(&mut h, &mut buf, Some(Align::RIGHT), None);
    let (text_origin, caret_origin) = shape_origins(&h.ui, node);
    let t = text_origin.expect("text shape");
    let c = caret_origin.expect("caret rect emitted while focused");
    let dx = ALIGN_W - TEXT_W_4CH; // 233.5
    let dy = (INNER_H - LINE_H) * 0.5;
    assert!((t.x - (PAD_L + dx)).abs() < 1e-3, "text.x = {}", t.x);
    assert!(
        (c.x - (PAD_L + dx + TEXT_W_4CH)).abs() < 1e-3,
        "caret.x = {} (expected {})",
        c.x,
        PAD_L + dx + TEXT_W_4CH,
    );
    // Caret right edge sits exactly at the clip's right edge.
    assert!(
        (c.x + CARET_W - (PAD_L + INNER_W)).abs() < 1e-3,
        "caret should reserve CARET_W before clip edge: caret.x + CARET_W = {}",
        c.x + CARET_W,
    );
    assert!((c.y - (PAD_T + dy)).abs() < 1e-3, "caret.y = {}", c.y);
}

#[test]
fn empty_focused_caret_vcenters_against_one_line() {
    // Bug fix pin: empty buffer's measured height is 0; if the widget
    // used it directly the caret would sit below center. The widget
    // floors measured.h at `line_height_px`, so VAlign::Center
    // centers the caret against a full virtual line.
    let mut h = ui_at_no_cosmic(NARROW);
    let mut buf = String::new();
    frame(&mut h, &mut buf, None, None);
    h.click_at(glam::Vec2::new(50.0, 20.0));
    frame(&mut h, &mut buf, None, None);
    let node = frame(&mut h, &mut buf, None, None);
    let (_, caret_origin) = shape_origins(&h.ui, node);
    let c = caret_origin.expect("focused empty editor still paints caret");
    let authored_line_height = 16.0 * crate::widgets::theme::text_style::LINE_HEIGHT_MULT;
    let dy = (INNER_H - authored_line_height) * 0.5;
    assert!((c.x - PAD_L).abs() < 1e-3, "caret.x = {}", c.x);
    assert!((c.y - (PAD_T + dy)).abs() < 1e-3, "caret.y = {}", c.y);
}

#[test]
fn placeholder_uses_own_measured_size_for_alignment() {
    // Bug fix pin: empty + unfocused → render placeholder. Offset is
    // computed from the placeholder string ("wxyz", mono 32 px), not
    // the empty buffer (which would collapse any halign to zero).
    let mut h = ui_at_no_cosmic(NARROW);
    let mut buf = String::new();
    let node = warmup_then(&mut h, &mut buf, Some(Align::RIGHT), Some("wxyz"));
    let (origin, _) = shape_origins(&h.ui, node);
    let o = origin.expect("placeholder paints when unfocused + empty");
    let dx = ALIGN_W - TEXT_W_4CH;
    assert!(
        (o.x - (PAD_L + dx)).abs() < 1e-3,
        "placeholder must align right: x = {} (expected {})",
        o.x,
        PAD_L + dx,
    );
}

#[test]
fn click_compensates_for_right_align() {
    // Right-aligned "abcd": dx = 238. Glyph 'b' spans editor x =
    // 5+238+8..5+238+16 = 251..259. Clicking at 254 (mid-glyph) must
    // land on byte 1, proving `run_input` subtracts the same
    // `align_offset.x` from the local pointer coords.
    let mut h = ui_at_no_cosmic(NARROW);
    let mut buf = String::from("abcd");
    // Two warmup frames so the second one carries response.rect and
    // the click hit-test runs against the right-aligned layout.
    frame(&mut h, &mut buf, Some(Align::RIGHT), None);
    frame(&mut h, &mut buf, Some(Align::RIGHT), None);
    h.press_at(glam::Vec2::new(254.0, 20.0));
    frame(&mut h, &mut buf, Some(Align::RIGHT), None);
    h.release();
    let id = WidgetId::from_hash("align-ed");
    let caret = h.ui.state_mut::<TextEditState>(id).edit.caret;
    assert!(
        (1..=2).contains(&caret),
        "click on right-aligned glyph 'b' must land near byte 1 (got {caret})",
    );
}

#[test]
fn align_overflow_clamps_to_zero() {
    // Text wider than the inner rect: alignment offset clamps to
    // zero on the overflowing axis (encoder convention), leaving
    // scroll-to-caret to keep the active end visible. "a" × 100 →
    // 800 px > 270 inner_w. LEFT, RIGHT, CENTER must all render text
    // at x = padding.left.
    for align in [Align::LEFT, Align::RIGHT, Align::CENTER] {
        let mut h = ui_at_no_cosmic(NARROW);
        let mut buf = "a".repeat(100);
        let node = warmup_then(&mut h, &mut buf, Some(align), None);
        let (origin, _) = shape_origins(&h.ui, node);
        let o = origin.expect("text shape");
        assert!(
            (o.x - PAD_L).abs() < 1e-3,
            "overflow under {align:?}: text.x = {} (expected {PAD_L})",
            o.x,
        );
    }
}

#[test]
fn selection_rects_offset_matches_text() {
    // Selection wash uses the same `align_offset` as the text shape.
    // Mono fallback emits one rect for [0..2] on "abcd" → x = 0,
    // w = 16 in text-local coords. Under HAlign::Right that becomes
    // editor-local x = PAD_L + (ALIGN_W − TEXT_W_4CH).
    let mut h = ui_at_no_cosmic(NARROW);
    let mut buf = String::from("abcd");
    frame(&mut h, &mut buf, Some(Align::RIGHT), None);
    frame(&mut h, &mut buf, Some(Align::RIGHT), None);
    h.click_at(glam::Vec2::new(260.0, 20.0));
    h.key(Key::Home);
    shift_arrow_right(&mut h.ui);
    shift_arrow_right(&mut h.ui);
    let node = frame(&mut h, &mut buf, Some(Align::RIGHT), None);

    // Selection wash is emitted *before* the text shape; pick the
    // first rounded rect with a `local_rect` in the block's stream.
    let at = block_at(&h.ui, node);
    let block = block_of(&h.ui, node);
    let first_rounded = h.ui.forest.trees[Layer::Main]
        .shapes_of(block)
        .find_map(|s| match s {
            ShapeRecord::Quad(QuadShape::Rect {
                kind: RectKind::Rounded,
                local_rect,
                ..
            }) => *local_rect,
            _ => None,
        });
    let r = first_rounded.expect("selection wash rect present");
    let dx = ALIGN_W - TEXT_W_4CH;
    assert!(
        (r.min.x + at.x - (PAD_L + dx)).abs() < 1e-3,
        "selection wash must align with right-aligned text: x = {}",
        r.min.x + at.x,
    );
}
/// Per-line halign tests use real cosmic shaping (`ui_with_text`).
///
/// These assert the *block-local* half of alignment. A shaped run is
/// measured and reported as a block whose own left edge is x = 0; the
/// owner then places that block inside its rect with the same halign
/// (`TextGeometry::block_offset`, and `align_in_rect` for encoder-placed
/// text). So halign shows up here only as the offset of a *narrow* line
/// relative to the widest one — never as an offset of the block itself,
/// which would be the same alignment applied twice.
mod per_line {
    use crate::text::glyph_font::GlyphFont;
    use crate::text::request::internals::TestShape;
    use crate::text::shaper::TextShaper;
    use crate::text::{FontFamily, FontWeight};
    use crate::widgets::text_edit::tests::*;
    use crate::{Align, HAlign};
    use glam::UVec2;

    const FS: f32 = 16.0;
    const LH: f32 = 19.2;

    fn cosmic_ui() -> UiHarness {
        UiHarness::with_text(UVec2::new(800, 200))
    }

    fn shape(wrap: f32, halign: HAlign) -> TestShape {
        TestShape {
            font: GlyphFont {
                size_px: FS,
                line_height_px: LH,
                family: FontFamily::Sans,
                weight: FontWeight::Regular,
            },
            max_width_px: Some(wrap),
            halign,
        }
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
                    .resources
                    .text
                    .cursor_xy(text, text.len(), shape(300.0, halign))
                    .x
            })
            .collect();
        for (halign, x) in ALL.iter().zip(&xs) {
            assert!(
                (x - xs[0]).abs() < 1e-3,
                "{halign:?} caret {x} must match Left {} on a single line",
                xs[0],
            );
        }
        // …and it is the run's own width, not the wrap target.
        let measured = ui
            .ui
            .resources
            .text
            .measure(text, shape(300.0, HAlign::Right));
        assert!(
            (xs[0] - measured.measured.w).abs() <= 1.0,
            "end-of-line caret {} must sit at the block's right edge {}",
            xs[0],
            measured.measured.w,
        );
        assert!(
            xs[0] < 100.0,
            "\"hi\" is nowhere near the 300 px wrap target"
        );
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
            .resources
            .text
            .measure(text, shape(wrap, HAlign::Right))
            .measured
            .w;
        let caret = |halign| {
            ui.ui
                .resources
                .text
                .cursor_xy(text, text.len(), shape(wrap, halign))
                .x
        };
        let (left, center, right) = (
            caret(HAlign::Left),
            caret(HAlign::Center),
            caret(HAlign::Right),
        );
        // Left leaves the short line at the block's left edge, so its
        // end-caret is just the line's own width.
        assert!(
            left < block * 0.5,
            "left-aligned short line stays narrow: {left}"
        );
        // Right takes it to the block's right edge — the block, not the
        // wrap target.
        assert!(
            (right - block).abs() <= 1.0,
            "right-aligned short line must end at the block edge {block}, got {right}",
        );
        assert!(
            block < wrap - 100.0,
            "the block ({block}) must be much narrower than the wrap target, or this proves nothing",
        );
        // Center splits the slack.
        assert!(
            (center - (left + right) / 2.0).abs() <= 1.0,
            "center ({center}) must sit midway between left ({left}) and right ({right})",
        );
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
            .map(|&halign| c.measure("hi\nyo", shape(wrap, halign)).measured.w)
            .collect();
        for (halign, w) in ALL.iter().zip(&widths) {
            assert!(
                (w - widths[0]).abs() < 1e-3,
                "{halign:?} measured {w} must match Left {}",
                widths[0],
            );
        }
        assert!(
            widths[0] < 60.0,
            "\"hi\"/\"yo\" is ~13 px of glyphs, not {} (wrap {wrap})",
            widths[0],
        );
    }

    /// An empty buffer has a zero-width block, so its caret is at 0 for
    /// every halign — the owner's placement of that empty block is what
    /// puts it on the correct edge.
    #[test]
    fn an_empty_buffer_caret_is_at_the_block_origin() {
        let ui = cosmic_ui();
        for halign in ALL {
            let x = ui
                .ui
                .resources
                .text
                .cursor_xy("", 0, shape(300.0, halign))
                .x;
            assert!(x.abs() < 1e-3, "{halign:?} empty caret must be 0, got {x}");
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
                        size_px: 16.0,
                        line_height_px: 19.2,
                        family: FontFamily::Sans,
                        weight: FontWeight::Regular,
                    },
                    max_width_px: Some(100.0),
                    halign: HAlign::Left,
                },
            )
            .key;
        let r = c
            .measure(
                "hi",
                TestShape {
                    font: GlyphFont {
                        size_px: 16.0,
                        line_height_px: 19.2,
                        family: FontFamily::Sans,
                        weight: FontWeight::Regular,
                    },
                    max_width_px: Some(100.0),
                    halign: HAlign::Right,
                },
            )
            .key;
        assert_ne!(l, r, "halign must enter the cache key");
        assert_ne!(
            l.halign_q, r.halign_q,
            "halign_q is the discriminating field"
        );
    }

    #[test]
    fn unbounded_halign_collapses_to_auto_in_key() {
        // Without a wrap target cosmic can't apply per-line align,
        // so every halign value at `max_width_px = None` shapes the
        // same buffer. Key construction collapses `halign_q` to `Auto`'s
        // discriminant on that path so single-line callers don't
        // pay an N-way cache split for identical glyph positions.
        let c = TextShaper::new();
        let left = c
            .measure(
                "hi",
                TestShape {
                    font: GlyphFont {
                        size_px: 16.0,
                        line_height_px: 19.2,
                        family: FontFamily::Sans,
                        weight: FontWeight::Regular,
                    },
                    max_width_px: None,
                    halign: HAlign::Left,
                },
            )
            .key;
        let right = c
            .measure(
                "hi",
                TestShape {
                    font: GlyphFont {
                        size_px: 16.0,
                        line_height_px: 19.2,
                        family: FontFamily::Sans,
                        weight: FontWeight::Regular,
                    },
                    max_width_px: None,
                    halign: HAlign::Right,
                },
            )
            .key;
        assert_eq!(left, right, "halign must not split the unbounded cache");
        assert_eq!(
            left.halign_q,
            HAlign::Auto as u8,
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
        let mut node = None;
        let mut record = |ui: &mut Ui| {
            Panel::hstack().auto_id().show(ui, |ui| {
                node = Some(
                    TextEdit::new(&mut buf)
                        .id(WidgetId::from_hash("fits-ml"))
                        .multiline(true)
                        .text_align(Align::TOP_RIGHT)
                        .size((Sizing::fixed(300.0), Sizing::fixed(120.0)))
                        .show(ui)
                        .response
                        .node(),
                );
            });
        };
        // Two frames — first warms up `response.rect`, second is the
        // one we inspect.
        h.frame(&mut record);
        h.frame(&mut record);
        // Read the layout's `ShapedText.key` for the rendered text.
        // `text_spans[node]` indexes one entry per `ShapeRecord::Text`
        // on the node; a multi-line field emits a single text shape, and
        // records it on the block child that carries its alignment.
        let node = block_of(&h.ui, node.unwrap());
        let main = &h.ui.layout[Layer::Main];
        let span = main.text_spans[node.idx()];
        assert_eq!(span.len, 1, "one Shape::Text expected on the block");
        let shaped = main.text_shapes[span.start as usize];
        // `HAlign::Right as u8 = 3` — pin the discriminant directly
        // so a variant reordering trips here instead of silently
        // falling through.
        assert_eq!(
            shaped.key.halign_q,
            HAlign::Right as u8,
            "rendered buffer must carry the user's halign in its cache key (got {})",
            shaped.key.halign_q,
        );
        // Also check the wrap-target axis is set — if it's
        // `u32::MAX` the buffer was shaped without `max_width_px`
        // and cosmic wouldn't have applied per-line align.
        assert_ne!(
            shaped.key.max_w_q,
            u32::MAX,
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
        h.frame(&mut record);
        h.frame(&mut record);
        let a = h.ui.resources.text.measure_calls();
        h.frame(&mut record);
        let b = h.ui.resources.text.measure_calls();
        let per_frame = b - a;
        // Drive several more frames with identical inputs and verify
        // each one costs exactly the same number of `measure_calls`.
        for i in 0..5 {
            let before = h.ui.resources.text.measure_calls();
            h.frame(&mut record);
            let after = h.ui.resources.text.measure_calls();
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
        use crate::scene::shapes::record::ShapeRecord;
        let mut h = cosmic_ui();
        let mut buf = String::new();
        let mut node = None;
        let mut record = |ui: &mut Ui| {
            Panel::hstack().auto_id().show(ui, |ui| {
                node = Some(
                    TextEdit::new(&mut buf)
                        .id(WidgetId::from_hash("ph-ml"))
                        .multiline(true)
                        .text_align(Align::TOP_RIGHT)
                        .placeholder("type a paragraph here — long enough to actually wrap")
                        .size((Sizing::fixed(300.0), Sizing::fixed(120.0)))
                        .show(ui)
                        .response
                        .node(),
                );
            });
        };
        h.frame(&mut record);
        h.frame(&mut record);
        let node = node.unwrap();
        // (a) `Shape::Text.align` reflects the user's text_align.
        let payloads = h.ui.forest.record_store.payloads.borrow();
        let interned_text = payloads.interned_text();
        let node = block_of(&h.ui, node);
        let tree = &h.ui.forest.trees[Layer::Main];
        let shape_align = tree.shapes_of(node).find_map(|s| match s {
            ShapeRecord::Text { align, text, .. } => {
                Some((*align, text.source.resolve(&interned_text).to_owned()))
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
        let main = &h.ui.layout[Layer::Main];
        let span = main.text_spans[node.idx()];
        assert_eq!(span.len, 1, "one Shape::Text expected on the block");
        let shaped = main.text_shapes[span.start as usize];
        assert_eq!(
            shaped.key.halign_q,
            HAlign::Right as u8,
            "placeholder buffer must carry the user's halign in its cache key",
        );
        assert_ne!(
            shaped.key.max_w_q,
            u32::MAX,
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
        h.ui.state_mut::<TextEditState>(id).edit.caret = 5;
        h.frame(&mut record);
        // wrap target = inner width = 300 - 2*5 = 290.
        let wrap = 290.0;
        let block =
            h.ui.resources
                .text
                .measure(&buf, shape(wrap, HAlign::Right))
                .measured
                .w;
        let caret_short =
            h.ui.resources
                .text
                .cursor_xy(&buf, 5, shape(wrap, HAlign::Right))
                .x;
        // "short" is the narrow line, so right-align carries its caret to
        // the block's right edge rather than leaving it at ~35 px.
        assert!(
            (caret_short - block).abs() <= 1.0,
            "right-aligned 'short' caret must reach the block edge {block}, got {caret_short}",
        );
        assert!(
            caret_short > 100.0,
            "…and that is far from the line's own ~35 px width (got {caret_short})",
        );
    }
}

#[test]
fn multiline_default_is_top_left() {
    // Default for `multiline(true)` is `Align::TOP_LEFT`. With "abcd"
    // the text origin sits flush at the inner top-left = padding.
    let mut h = ui_at_no_cosmic(NARROW);
    let mut buf = String::from("abcd");
    let mut node: Option<NodeId> = None;
    let mut record = |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            node = Some(
                TextEdit::new(&mut buf)
                    .id(ed_id())
                    .multiline(true)
                    .size((Sizing::fixed(EDIT_W), Sizing::fixed(80.0)))
                    .show(ui)
                    .response
                    .node(),
            );
        });
    };
    // Two frames: first to warm up the cascade.
    h.frame(&mut record);
    h.frame(&mut record);
    let (origin, _) = shape_origins(&h.ui, node.unwrap());
    let o = origin.expect("text shape");
    assert!((o.x - PAD_L).abs() < 1e-3, "x = {}", o.x);
    assert!((o.y - PAD_T).abs() < 1e-3, "y = {}", o.y);
}

/// Regression: an ancestor `Panel::transform` zoom must not drift the
/// text origin. The widget computes vcenter as
/// `(inner_h − measured_h) / 2`; `measured_h` comes from the shaper in
/// logical units, so `inner_h` must come from `response.layout_rect`
/// (pre-transform) — not `response.rect` (post-transform). Reading
/// `rect` instead inflates `inner_h` by the zoom factor, pushing the
/// text down by `(scale − 1) · line_height / 2` and clipping it at the
/// editor's bottom edge. Repro is the darkroom graph view: a static
/// `TextEdit` inside `Panel::canvas().transform(TranslateScale::new(pan,
/// zoom))` drifts text down as the user zooms in.
#[test]
fn text_origin_invariant_under_ancestor_transform_zoom() {
    fn run(scale: f32) -> glam::Vec2 {
        let mut h = ui_at_no_cosmic(NARROW);
        let mut buf = String::from("abcd");
        let mut node: Option<NodeId> = None;
        let mut record = |ui: &mut Ui| {
            Panel::canvas()
                .auto_id()
                .transform(TranslateScale::new(glam::Vec2::ZERO, scale))
                .show(ui, |ui| {
                    node = Some(
                        TextEdit::new(&mut buf)
                            .id(WidgetId::from_hash("zoom-ed"))
                            .size((Sizing::fixed(EDIT_W), Sizing::fixed(EDIT_H)))
                            .show(ui)
                            .response
                            .node(),
                    );
                });
        };
        // Two frames: cascade lags one frame, so the second frame is
        // the one whose `response.layout_rect` drives the offset math.
        h.frame(&mut record);
        h.frame(&mut record);
        let (origin, _) = shape_origins(&h.ui, node.unwrap());
        origin.expect("text shape emitted for non-empty buffer")
    }
    let unscaled = run(1.0);
    for &scale in &[2.0_f32, 0.5, 1.7] {
        let zoomed = run(scale);
        assert!(
            (zoomed.x - unscaled.x).abs() < 1e-3,
            "scale {scale}: text.x = {} drifted from {} (Δ = {})",
            zoomed.x,
            unscaled.x,
            zoomed.x - unscaled.x,
        );
        assert!(
            (zoomed.y - unscaled.y).abs() < 1e-3,
            "scale {scale}: text.y = {} drifted from {} (Δ = {})",
            zoomed.y,
            unscaled.y,
            zoomed.y - unscaled.y,
        );
    }
}

/// **A field placed by
/// [`TextEditTheme::corner_centring`](crate::TextEditTheme::corner_centring)
/// lands its glyphs on the point it was asked for.**
///
/// The claim an in-place edit rests on: something is drawn, and a field stands
/// where it was drawn without the value moving under the reader. What makes it
/// worth pinning is that the offset it names is not one number but four facts in
/// three passes — the theme's padding, the chrome stroke `Tree::open_node` folds
/// into it, the hug reservation in [`PaintInput::record`](crate::widgets::text_edit::paint_input::PaintInput::record),
/// and the single caret's room
/// [`TextGeometry::resolve`](crate::widgets::text_edit::text_geometry::TextGeometry::resolve) takes off the box the
/// run is centred in. An application working that out for itself would be
/// copying all four and could not be told when one moved.
///
/// Against a field actually laid out rather than against the same arithmetic
/// spelled twice, so the two are checked to *agree* — the theme's own padding
/// and stroke, so restyling a field moves both together.
#[test]
fn a_field_placed_by_its_own_text_centres_that_text_where_it_was_asked() {
    let mut h = ui_at_no_cosmic(WIDE);
    // What the mono fallback measures "abcd" as. Its height rather than the
    // glyphs' own, because that is the box a line is laid in — see
    // `resolve_geometry`, which floors the run at the leading.
    let text = Size::new(TEXT_W_4CH, LINE_H);
    // Clear of every edge, so a field that fell back to the surface's own
    // corner is a wrong answer rather than a near miss.
    let at = glam::Vec2::new(200.0, 40.0);
    // The theme the field below will be shown with, since it asks for none of
    // its own — so the two cannot be answering about different fields.
    let corner = h.ui.theme().text_edit.corner_centring(text, at);

    let mut buf = String::from("abcd");
    let mut node: Option<NodeId> = None;
    let mut record = |ui: &mut Ui| {
        Panel::canvas().auto_id().show(ui, |ui| {
            node = Some(
                TextEdit::new(&mut buf)
                    .id(ed_id())
                    .text_align(Align::CENTER)
                    .size((Sizing::HUG, Sizing::HUG))
                    .position(corner)
                    .show(ui)
                    .response
                    .node(),
            );
        });
    };
    // Two frames: the block is placed against the rect arrange has just
    // resolved, and `response.layout_rect` is a frame behind on the first.
    h.frame(&mut record);
    h.frame(&mut record);

    let field =
        h.ui.response_for(ed_id())
            .layout_rect
            .expect("the field was arranged");
    assert!(
        (field.min - corner).abs().max_element() < 1e-3,
        "the field was put at {:?} having been placed at {corner:?}",
        field.min,
    );
    let origin = shape_origins(&h.ui, node.unwrap())
        .0
        .expect("text shape emitted");
    let centre = field.min + origin + glam::Vec2::new(text.w, text.h) * 0.5;
    assert!(
        (centre - at).abs().max_element() < 1e-3,
        "the glyphs centred on {centre:?} for a field asked to centre them on {at:?}",
    );
}
