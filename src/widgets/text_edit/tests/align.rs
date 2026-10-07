//! Tests for `TextEdit::text_align` and per-mode default alignment. Mono fallback: 8 px/char at 16 px font, line height 19.203125 (`round(19.2 × 64) / 64`). Editor 280×40, padding (5, 3) plus 1.5 px stroke folded in: effective (6.5, 4.5), inner rect 267×31.

use crate::Align;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::shape::paint::quad_shape::QuadShape;
use crate::shape::rect::RectKind;
use crate::widgets::text_edit::tests::*;
use crate::widgets::theme::text_style::LINE_HEIGHT_MULT;

const EDIT_W: f32 = 280.0;
const EDIT_H: f32 = 40.0;
/// Theme padding (5, 3) plus chrome stroke (1.5), folded as the encoder's clip and TextEdit do.
const PAD_L: f32 = 6.5;
const PAD_T: f32 = 4.5;
/// Default `TextEditTheme::caret_width`, reserved at each line's trailing edge.
const CARET_W: f32 = 1.5;
const INNER_W: f32 = EDIT_W - 2.0 * PAD_L; // 267
const INNER_H: f32 = EDIT_H - 2.0 * PAD_T; // 31
const ALIGN_W: f32 = INNER_W - CARET_W; // 265.5
const LINE_H: f32 = 19.203_125;
const TEXT_W_4CH: f32 = 32.0; // mono "abcd" width

/// Drive one frame of a single-line editor, returning the field's `NodeId`. One frame suffices for alignment; tests of hit-test or scroll use [`warmup_then`].
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

/// Two frames: the first warms the cascade so `response.rect` is real, the second is read.
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

/// Where the block child (run, wash, caret; see [`PaintInput::record`](crate::widgets::text_edit::paint_input::PaintInput::record)) was arranged, relative to its field's corner.
fn block_at(ui: &Ui, field: NodeId) -> Vec2 {
    let tree = ui.tree(Layer::Main);
    let of = |node: NodeId| {
        ui.response_for(tree.records.widget_id()[node.idx()])
            .layout_rect
            .expect("arranged")
    };
    of(block_of(ui, field)).min - of(field).min
}

/// Where the text and the caret start, in the field's own coordinates.
#[derive(Debug)]
struct Origins {
    text: Option<Vec2>,
    caret: Option<Vec2>,
}

/// [`Origins`] of the field at `node`. Paint order is wash, text, caret: the caret is the last rounded rect with a `local_rect`.
fn shape_origins(ui: &Ui, node: NodeId) -> Origins {
    let at = block_at(ui, node);
    let block = block_of(ui, node);
    let tree = ui.tree(Layer::Main);
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
            }) => caret_origin = Some(Vec2::new(r.min.x, r.min.y) + at),
            _ => {}
        }
    }
    Origins {
        text: text_origin,
        caret: caret_origin,
    }
}

/// **The first frame an editor exists on aligns like the ones after it.**
///
/// Alignment needs the arranged rect, which record lacks; resolved there, a new field painted hard left for one frame. The block is now a child placed against the fresh rect. Compared against the settled answer, not restated numbers.
#[test]
fn the_first_frame_aligns_like_the_ones_after_it() {
    let settled = {
        let mut h = UiHarness::new(NARROW);
        let mut buf = String::from("abcd");
        let node = warmup_then(&mut h, &mut buf, Some(Align::CENTER), None);
        shape_origins(&h.ui, node).text.expect("text shape emitted")
    };
    assert!(settled.x > PAD_L + 1.0, "x = {} is not centred", settled.x);

    let mut h = UiHarness::new(NARROW);
    let mut buf = String::from("abcd");
    let node = frame(&mut h, &mut buf, Some(Align::CENTER), None);
    let first = shape_origins(&h.ui, node).text.expect("text shape emitted");
    assert_eq!(
        first.x, settled.x,
        "first frame painted x = {} where the settled frame paints {}",
        first.x, settled.x
    );
    assert_eq!(
        first.y, settled.y,
        "first frame painted y = {} where the settled frame paints {}",
        first.y, settled.y
    );
}

#[test]
fn single_line_default_is_left_vcenter() {
    // No `.text_align(...)`: mode default `Align::LEFT`. "abcd" (32×19.203125): dx = 0, dy = (inner height − line height) / 2.
    let mut h = UiHarness::new(NARROW);
    let mut buf = String::from("abcd");
    let node = warmup_then(&mut h, &mut buf, None, None);
    let origin = shape_origins(&h.ui, node).text;
    let o = origin.expect("text shape emitted for non-empty buffer");
    assert_eq!(o.x, PAD_L, "x = {}", o.x);
    let dy = (INNER_H - LINE_H) * 0.5;
    assert_eq!(o.y, PAD_T + dy, "y = {}", o.y);
}

#[test]
fn single_line_text_align_table() {
    // Sweep every (HAlign × VAlign) on single-line "abcd"; overflow clamping does not fire.
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
        let mut h = UiHarness::new(NARROW);
        let mut buf = String::from("abcd");
        let node = warmup_then(&mut h, &mut buf, Some(align), None);
        let origin = shape_origins(&h.ui, node).text;
        let o = origin.expect("text shape emitted");
        assert_eq!(
            o.x,
            PAD_L + dx,
            "{label}: text.x = {} (expected {})",
            o.x,
            PAD_L + dx
        );
        assert_eq!(
            o.y,
            PAD_T + dy,
            "{label}: text.y = {} (expected {})",
            o.y,
            PAD_T + dy
        );
    }
}

#[test]
fn caret_tracks_aligned_text() {
    // Caret at end of "abcd", HAlign::Right: it shifts by the text's `ALIGN_W − TEXT_W_4CH` too.
    let mut h = UiHarness::new(NARROW);
    let mut buf = String::from("abcd");
    frame(&mut h, &mut buf, Some(Align::RIGHT), None);
    h.click_at(Vec2::new(260.0, 20.0));
    h.key(Key::End);
    frame(&mut h, &mut buf, Some(Align::RIGHT), None);
    let node = frame(&mut h, &mut buf, Some(Align::RIGHT), None);
    let Origins {
        text: text_origin,
        caret: caret_origin,
    } = shape_origins(&h.ui, node);
    let t = text_origin.expect("text shape");
    let c = caret_origin.expect("caret rect emitted while focused");
    let dx = ALIGN_W - TEXT_W_4CH; // 233.5
    let dy = (INNER_H - LINE_H) * 0.5;
    assert_eq!(t.x, PAD_L + dx, "text.x = {}", t.x);
    assert_eq!(
        c.x,
        PAD_L + dx + TEXT_W_4CH,
        "caret.x = {} (expected {})",
        c.x,
        PAD_L + dx + TEXT_W_4CH
    );
    assert_eq!(
        c.x + CARET_W,
        PAD_L + INNER_W,
        "caret should reserve CARET_W before clip edge: caret.x + CARET_W = {}",
        c.x + CARET_W
    );
    assert_eq!(c.y, PAD_T + dy, "caret.y = {}", c.y);
}

#[test]
fn empty_focused_caret_vcenters_against_one_line() {
    // Pin: an empty buffer measures height 0; the widget floors it at `line_height`.
    let mut h = UiHarness::new(NARROW);
    let mut buf = String::new();
    frame(&mut h, &mut buf, None, None);
    h.click_at(Vec2::new(50.0, 20.0));
    frame(&mut h, &mut buf, None, None);
    let node = frame(&mut h, &mut buf, None, None);
    let caret_origin = shape_origins(&h.ui, node).caret;
    let c = caret_origin.expect("focused empty editor still paints caret");
    let line_height = 1229.0 / 64.0;
    assert_eq!(line_height, (16.0 * LINE_HEIGHT_MULT * 64.0).round() / 64.0);
    let dy = (INNER_H - line_height) * 0.5;
    assert_eq!(c.x, PAD_L, "caret.x = {}", c.x);
    assert_eq!(c.y, PAD_T + dy, "caret.y = {}", c.y);
}

#[test]
fn placeholder_uses_own_measured_size_for_alignment() {
    // Pin: empty and unfocused renders the placeholder, offset from its text ("wxyz"), not the empty buffer.
    let mut h = UiHarness::new(NARROW);
    let mut buf = String::new();
    let node = warmup_then(&mut h, &mut buf, Some(Align::RIGHT), Some("wxyz"));
    let origin = shape_origins(&h.ui, node).text;
    let o = origin.expect("placeholder paints when unfocused + empty");
    let dx = ALIGN_W - TEXT_W_4CH;
    assert_eq!(
        o.x,
        PAD_L + dx,
        "placeholder must align right: x = {} (expected {})",
        o.x,
        PAD_L + dx
    );
    // The offset stored for hit-testing is that same placement.
    let stored = h.state::<TextEditState>(ed_id()).view.block_offset;
    let placed = block_at(&h.ui, node) - Vec2::new(PAD_L, PAD_T);
    assert_eq!(
        stored, placed,
        "stored {stored:?} is not where the block was placed, {placed:?}"
    );
}

#[test]
fn click_compensates_for_right_align() {
    // Right-aligned "abcd": dx = 238. Glyph 'b' spans x = 5+238+8..5+238+16 = 251..259. Clicking at 254 must land on byte 1, so the input pass subtracts the same `align_offset.x`.
    let mut h = UiHarness::new(NARROW);
    let mut buf = String::from("abcd");
    frame(&mut h, &mut buf, Some(Align::RIGHT), None);
    frame(&mut h, &mut buf, Some(Align::RIGHT), None);
    h.press_at(Vec2::new(254.0, 20.0));
    frame(&mut h, &mut buf, Some(Align::RIGHT), None);
    h.release();
    let id = WidgetId::from_hash("align-ed");
    let caret = h.state::<TextEditState>(id).edit.caret;
    assert!(
        (1..=2).contains(&caret),
        "click on right-aligned glyph 'b' must land near byte 1 (got {caret})",
    );
}

#[test]
fn align_overflow_clamps_to_zero() {
    // Text wider than the inner rect ("a" × 100 = 800 px > 270): offset clamps to zero, so x = padding.left.
    for align in [Align::LEFT, Align::RIGHT, Align::CENTER] {
        let mut h = UiHarness::new(NARROW);
        let mut buf = "a".repeat(100);
        let node = warmup_then(&mut h, &mut buf, Some(align), None);
        let origin = shape_origins(&h.ui, node).text;
        let o = origin.expect("text shape");
        assert_eq!(
            o.x, PAD_L,
            "overflow under {align:?}: text.x = {} (expected {PAD_L})",
            o.x
        );
    }
}

#[test]
fn selection_rects_offset_matches_text() {
    // The wash uses the text's `align_offset`: under Right, x = PAD_L + (ALIGN_W − TEXT_W_4CH).
    let mut h = UiHarness::new(NARROW);
    let mut buf = String::from("abcd");
    frame(&mut h, &mut buf, Some(Align::RIGHT), None);
    frame(&mut h, &mut buf, Some(Align::RIGHT), None);
    h.click_at(Vec2::new(260.0, 20.0));
    h.key(Key::Home);
    h.set_modifiers(Modifiers::SHIFT);
    h.key(Key::ArrowRight);
    h.key(Key::ArrowRight);
    h.set_modifiers(Modifiers::NONE);
    let node = frame(&mut h, &mut buf, Some(Align::RIGHT), None);

    let at = block_at(&h.ui, node);
    let block = block_of(&h.ui, node);
    let first_rounded =
        h.ui.tree(Layer::Main)
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
    assert_eq!(
        r.min.x + at.x,
        PAD_L + dx,
        "selection wash must align with right-aligned text: x = {}",
        r.min.x + at.x
    );
}

#[test]
fn multiline_default_is_top_left() {
    // `multiline(true)` defaults to `Align::TOP_LEFT`: the origin sits at padding.
    let mut h = UiHarness::new(NARROW);
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
    h.prime(2, &mut record);
    let origin = shape_origins(&h.ui, node.unwrap()).text;
    let o = origin.expect("text shape");
    assert_eq!(o.x, PAD_L, "x = {}", o.x);
    assert_eq!(o.y, PAD_T, "y = {}", o.y);
}

/// Regression: an ancestor `Panel::transform` zoom must not drift the text origin; `inner_h` must come from `response.layout_rect` (pre-transform), not `response.rect`.
#[test]
fn text_origin_invariant_under_ancestor_transform_zoom() {
    fn run(scale: f32) -> Vec2 {
        let mut h = UiHarness::new(NARROW);
        let mut buf = String::from("abcd");
        let mut node: Option<NodeId> = None;
        let mut record = |ui: &mut Ui| {
            Panel::canvas()
                .auto_id()
                .transform(TranslateScale::new(Vec2::ZERO, scale))
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
        h.prime(2, &mut record);
        let origin = shape_origins(&h.ui, node.unwrap()).text;
        origin.expect("text shape emitted for non-empty buffer")
    }
    let unscaled = run(1.0);
    for &scale in &[2.0_f32, 0.5, 1.7] {
        let zoomed = run(scale);
        assert_eq!(
            zoomed.x,
            unscaled.x,
            "scale {scale}: text.x = {} drifted from {} (Δ = {})",
            zoomed.x,
            unscaled.x,
            zoomed.x - unscaled.x
        );
        assert_eq!(
            zoomed.y,
            unscaled.y,
            "scale {scale}: text.y = {} drifted from {} (Δ = {})",
            zoomed.y,
            unscaled.y,
            zoomed.y - unscaled.y
        );
    }
}

/// **A field placed by [`TextEditTheme::corner_centering`](crate::TextEditTheme::corner_centering) lands its glyphs on the requested point.**
///
/// The offset combines theme padding, the folded chrome stroke, the hug reservation and the caret room; checked against a laid-out field.
#[test]
fn a_field_placed_by_its_own_text_centres_that_text_where_it_was_asked() {
    let mut h = UiHarness::new(WIDE);
    // The line box height, floored at the leading by `resolve_geometry`.
    let text = Size::new(TEXT_W_4CH, LINE_H);
    let at = Vec2::new(200.0, 40.0);
    let corner = h.ui.theme().text_edit.corner_centering(text, at);

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
    h.prime(2, &mut record);

    let field = h.arranged(ed_id());
    assert_eq!(
        field.min, corner,
        "the field was put at {:?} having been placed at {corner:?}",
        field.min
    );
    let origin = shape_origins(&h.ui, node.unwrap())
        .text
        .expect("text shape emitted");
    let centre = field.min + origin + Vec2::new(text.w, text.h) * 0.5;
    assert_eq!(
        centre, at,
        "the glyphs centred on {centre:?} for a field asked to centre them on {at:?}"
    );
}
