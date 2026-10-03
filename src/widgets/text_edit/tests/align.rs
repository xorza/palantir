//! Tests for `TextEdit::text_align` and the default alignment per
//! mode. Mono fallback (`UiHarness::new`): 8 px / char @ 16 px font,
//! `LINE_HEIGHT_MULT = 1.2` → canonical line height
//! `round(19.2 × 64) / 64 = 19.203125` px. Editor is 280×40
//! with theme padding (5, 3) plus the 1.5 px chrome stroke that
//! `Tree::open_node` folds into padding (mirrored by TextEdit so
//! glyph/caret coords land on the encoder's clip rect). Stroke width
//! is constant across normal/focused — the only thing focus changes
//! is the color — so the inner rect doesn't shift when the user
//! clicks in. Effective padding is (6.5, 4.5), inner rect 267×31.

use crate::Align;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::shape::paint::quad_shape::QuadShape;
use crate::shape::rect::RectKind;
use crate::widgets::text_edit::tests::*;
use crate::widgets::theme::text_style::LINE_HEIGHT_MULT;

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

/// [`Origins`] of the field at `node`. The paint
/// order is selection-wash → text → caret, so the text shape is the only
/// `Shape::Text` and the caret is the *last* rounded rect with a `local_rect`
/// (selection rects come before the text; the caret comes after — for empty
/// focused editors it's the only rounded rect in the stream).
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
        let mut h = UiHarness::new(NARROW);
        let mut buf = String::from("abcd");
        let node = warmup_then(&mut h, &mut buf, Some(Align::CENTER), None);
        shape_origins(&h.ui, node).text.expect("text shape emitted")
    };
    // Centred rather than left, so a zero-sized box is a wrong answer rather
    // than accidentally the right one.
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
    // No `.text_align(...)` → mode default `Align::LEFT` (left +
    // vcenter). With "abcd" (32×19.203125) inside the inner rect,
    // dx = 0 and dy = (inner height − line height) / 2.
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
    // Focus + caret at end of "abcd". With HAlign::Right the text
    // origin shifts right by `ALIGN_W − TEXT_W_4CH`; the caret must
    // shift by the same dx so it sits at the rightmost glyph trailing
    // edge, leaving `CARET_W` of reserved room before the clip edge.
    let mut h = UiHarness::new(NARROW);
    let mut buf = String::from("abcd");
    // Warmup so response.rect lands; click; then a final frame so
    // the post-click focus state drives a caret render with the
    // resolved align offset.
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
    // Caret right edge sits exactly at the clip's right edge.
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
    // Bug fix pin: empty buffer's measured height is 0; if the widget
    // used it directly the caret would sit below center. The widget
    // floors measured.h at `line_height_px`, so VAlign::Center
    // centers the caret against a full virtual line.
    let mut h = UiHarness::new(NARROW);
    let mut buf = String::new();
    frame(&mut h, &mut buf, None, None);
    h.click_at(Vec2::new(50.0, 20.0));
    frame(&mut h, &mut buf, None, None);
    let node = frame(&mut h, &mut buf, None, None);
    let caret_origin = shape_origins(&h.ui, node).caret;
    let c = caret_origin.expect("focused empty editor still paints caret");
    // The shaper's 1/64-px leading: 16 × 1.2 = 19.2 rounds to 1229/64.
    let line_height = 1229.0 / 64.0;
    assert_eq!(line_height, (16.0 * LINE_HEIGHT_MULT * 64.0).round() / 64.0);
    let dy = (INNER_H - line_height) * 0.5;
    assert_eq!(c.x, PAD_L, "caret.x = {}", c.x);
    assert_eq!(c.y, PAD_T + dy, "caret.y = {}", c.y);
}

#[test]
fn placeholder_uses_own_measured_size_for_alignment() {
    // Bug fix pin: empty + unfocused → render placeholder. Offset is
    // computed from the placeholder string ("wxyz", mono 32 px), not
    // the empty buffer (which would collapse any halign to zero).
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
    // The offset stored for next frame's hit-test is the same placement:
    // the block the engine arranged, not a second alignment of the empty
    // buffer's own measure, which would have parked it a whole
    // placeholder further right.
    let stored = h.state::<TextEditState>(ed_id()).view.block_offset;
    let placed = block_at(&h.ui, node) - Vec2::new(PAD_L, PAD_T);
    assert_eq!(
        stored, placed,
        "stored {stored:?} is not where the block was placed, {placed:?}"
    );
}

#[test]
fn click_compensates_for_right_align() {
    // Right-aligned "abcd": dx = 238. Glyph 'b' spans editor x =
    // 5+238+8..5+238+16 = 251..259. Clicking at 254 (mid-glyph) must
    // land on byte 1, proving the input pass subtracts the same
    // `align_offset.x` from the local pointer coords.
    let mut h = UiHarness::new(NARROW);
    let mut buf = String::from("abcd");
    // Two warmup frames so the second one carries response.rect and
    // the click hit-test runs against the right-aligned layout.
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
    // Text wider than the inner rect: alignment offset clamps to
    // zero on the overflowing axis (encoder convention), leaving
    // scroll-to-caret to keep the active end visible. "a" × 100 →
    // 800 px > 270 inner_w. LEFT, RIGHT, CENTER must all render text
    // at x = padding.left.
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
    // Selection wash uses the same `align_offset` as the text shape.
    // Mono fallback emits one rect for [0..2] on "abcd" → x = 0,
    // w = 16 in text-local coords. Under HAlign::Right that becomes
    // editor-local x = PAD_L + (ALIGN_W − TEXT_W_4CH).
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

    // Selection wash is emitted *before* the text shape; pick the
    // first rounded rect with a `local_rect` in the block's stream.
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
    // Default for `multiline(true)` is `Align::TOP_LEFT`. With "abcd"
    // the text origin sits flush at the inner top-left = padding.
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
    // Two frames: first to warm up the cascade.
    h.prime(2, &mut record);
    let origin = shape_origins(&h.ui, node.unwrap()).text;
    let o = origin.expect("text shape");
    assert_eq!(o.x, PAD_L, "x = {}", o.x);
    assert_eq!(o.y, PAD_T, "y = {}", o.y);
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
        // Two frames: cascade lags one frame, so the second frame is
        // the one whose `response.layout_rect` drives the offset math.
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

/// **A field placed by
/// [`TextEditTheme::corner_centering`](crate::TextEditTheme::corner_centering)
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
    let mut h = UiHarness::new(WIDE);
    // What the mono fallback measures "abcd" as. Its height rather than the
    // glyphs' own, because that is the box a line is laid in — see
    // `resolve_geometry`, which floors the run at the leading.
    let text = Size::new(TEXT_W_4CH, LINE_H);
    // Clear of every edge, so a field that fell back to the surface's own
    // corner is a wrong answer rather than a near miss.
    let at = Vec2::new(200.0, 40.0);
    // The theme the field below will be shown with, since it asks for none of
    // its own — so the two cannot be answering about different fields.
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
    // Two frames: the block is placed against the rect arrange has just
    // resolved, and `response.layout_rect` is a frame behind on the first.
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
