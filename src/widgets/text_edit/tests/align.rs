//! Tests for `TextEdit::text_align` and the default alignment per
//! mode. Mono fallback (`ui_at_no_cosmic`): 8 px / char @ 16 px font,
//! `LINE_HEIGHT_MULT = 1.2` → line height 19.2 px. Editor is 280×40
//! with theme padding (5, 3), so the inner rect is 270×34, leaving
//! 270 − measured.w of horizontal slack and 34 − line_height = 14.8
//! of vertical slack to align inside.

use super::*;
use crate::Align;
use crate::forest::shapes::record::ShapeRecord;
use crate::forest::tree::{Layer, NodeId};

const EDIT_W: f32 = 280.0;
const EDIT_H: f32 = 40.0;
const PAD_L: f32 = 5.0;
const PAD_T: f32 = 3.0;
const INNER_W: f32 = EDIT_W - 2.0 * PAD_L; // 270
const INNER_H: f32 = EDIT_H - 2.0 * PAD_T; // 34
const LINE_H: f32 = 19.2; // 16 px font × 1.2 LINE_HEIGHT_MULT
const TEXT_W_4CH: f32 = 32.0; // mono "abcd" width

/// Drive one frame of a single-line editor at `text_align` + buffer +
/// optional placeholder, returning the leaf `NodeId` so the caller
/// can read shapes back. `response.rect` is one frame stale (cascade
/// runs in `post_record`), so callers must warm up at least one
/// frame before reading shapes for align assertions — `warmup_then`
/// below packages that.
fn frame(
    ui: &mut Ui,
    buf: &mut String,
    text_align: Option<Align>,
    placeholder: Option<&'static str>,
) -> NodeId {
    let mut node: Option<NodeId> = None;
    let mut record = |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            let mut e = TextEdit::new(buf)
                .id_salt("align-ed")
                .size((Sizing::Fixed(EDIT_W), Sizing::Fixed(EDIT_H)));
            if let Some(a) = text_align {
                e = e.text_align(a);
            }
            if let Some(p) = placeholder {
                e = e.placeholder(p);
            }
            node = Some(e.show(ui).node);
        });
    };
    run_at_acked(ui, NARROW, &mut record);
    node.unwrap()
}

/// Two-frame helper: first frame warms up the cascade so the editor
/// has a real `response.rect`; the second frame's shape stream is
/// what every align assertion reads.
fn warmup_then(
    ui: &mut Ui,
    buf: &mut String,
    text_align: Option<Align>,
    placeholder: Option<&'static str>,
) -> NodeId {
    frame(ui, buf, text_align, placeholder);
    frame(ui, buf, text_align, placeholder)
}

/// `(text_origin, caret_origin)` from the leaf's shape stream. The
/// paint order is selection-wash → text → caret, so the text shape
/// is the only `Shape::Text` and the caret is the *last* `RoundedRect`
/// with a `local_rect` (selection rects come before the text; the
/// caret comes after — for empty focused editors it's the only
/// `RoundedRect` in the stream).
fn shape_origins(ui: &Ui, node: NodeId) -> (Option<glam::Vec2>, Option<glam::Vec2>) {
    let mut text_origin = None;
    let mut caret_origin = None;
    for s in shapes_of(ui.forest.tree(Layer::Main), node) {
        match s {
            ShapeRecord::Text {
                local_rect: Some(r),
                ..
            } => text_origin = Some(glam::Vec2::new(r.min.x, r.min.y)),
            ShapeRecord::RoundedRect {
                local_rect: Some(r),
                ..
            } => caret_origin = Some(glam::Vec2::new(r.min.x, r.min.y)),
            _ => {}
        }
    }
    (text_origin, caret_origin)
}

/// Emit a shift+ArrowRight as the focused widget would see it.
/// `InputEvent::KeyDown` is a unit event; modifier state attaches to
/// the queued `KeyPress` after the fact (mirror of how
/// `multiline.rs:96-98` builds shift+arrow).
fn shift_arrow_right(ui: &mut Ui) {
    ui.on_input(InputEvent::KeyDown {
        key: Key::ArrowRight,
        repeat: false,
    });
    ui.input.frame_keys.last_mut().unwrap().mods = Modifiers {
        shift: true,
        ..Modifiers::NONE
    };
}

#[test]
fn single_line_default_is_left_vcenter() {
    // No `.text_align(...)` → mode default `Align::LEFT` (left +
    // vcenter). With "abcd" (32×19.2) inside 270×34: dx = 0,
    // dy = (34 − 19.2) / 2 = 7.4. Origin = padding + offset.
    let mut ui = ui_at_no_cosmic(NARROW);
    let mut buf = String::from("abcd");
    let node = warmup_then(&mut ui, &mut buf, None, None);
    let (origin, _) = shape_origins(&ui, node);
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
    // because 32×19.2 fits inside 270×34.
    let cx = (INNER_W - TEXT_W_4CH) * 0.5; // 119
    let rx = INNER_W - TEXT_W_4CH; // 238
    let cy = (INNER_H - LINE_H) * 0.5; // 7.4
    let by = INNER_H - LINE_H; // 14.8
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
        let mut ui = ui_at_no_cosmic(NARROW);
        let mut buf = String::from("abcd");
        let node = warmup_then(&mut ui, &mut buf, Some(align), None);
        let (origin, _) = shape_origins(&ui, node);
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
    // origin shifts right by 238; the caret must shift by the same
    // dx so it sits at the rightmost glyph trailing edge.
    let mut ui = ui_at_no_cosmic(NARROW);
    let mut buf = String::from("abcd");
    // Warmup so response.rect lands; click; then a final frame so
    // the post-click focus state drives a caret render with the
    // resolved align offset.
    frame(&mut ui, &mut buf, Some(Align::RIGHT), None);
    click_at(&mut ui, glam::Vec2::new(260.0, 20.0));
    ui.on_input(InputEvent::KeyDown {
        key: Key::End,
        repeat: false,
    });
    frame(&mut ui, &mut buf, Some(Align::RIGHT), None);
    let node = frame(&mut ui, &mut buf, Some(Align::RIGHT), None);
    let (text_origin, caret_origin) = shape_origins(&ui, node);
    let t = text_origin.expect("text shape");
    let c = caret_origin.expect("caret rect emitted while focused");
    let dx = INNER_W - TEXT_W_4CH; // 238
    let dy = (INNER_H - LINE_H) * 0.5; // 7.4
    assert!((t.x - (PAD_L + dx)).abs() < 1e-3, "text.x = {}", t.x);
    assert!(
        (c.x - (PAD_L + dx + TEXT_W_4CH)).abs() < 1e-3,
        "caret.x = {} (expected {})",
        c.x,
        PAD_L + dx + TEXT_W_4CH,
    );
    assert!((c.y - (PAD_T + dy)).abs() < 1e-3, "caret.y = {}", c.y);
}

#[test]
fn empty_focused_caret_vcenters_against_one_line() {
    // Bug fix pin: empty buffer's measured height is 0; if the widget
    // used it directly the caret would sit below center. The widget
    // floors measured.h at `line_height_px`, so VAlign::Center
    // centers the caret against a full virtual line.
    let mut ui = ui_at_no_cosmic(NARROW);
    let mut buf = String::new();
    frame(&mut ui, &mut buf, None, None);
    click_at(&mut ui, glam::Vec2::new(50.0, 20.0));
    frame(&mut ui, &mut buf, None, None);
    let node = frame(&mut ui, &mut buf, None, None);
    let (_, caret_origin) = shape_origins(&ui, node);
    let c = caret_origin.expect("focused empty editor still paints caret");
    let dy = (INNER_H - LINE_H) * 0.5;
    assert!((c.x - PAD_L).abs() < 1e-3, "caret.x = {}", c.x);
    assert!((c.y - (PAD_T + dy)).abs() < 1e-3, "caret.y = {}", c.y);
}

#[test]
fn placeholder_uses_own_measured_size_for_alignment() {
    // Bug fix pin: empty + unfocused → render placeholder. Offset is
    // computed from the placeholder string ("wxyz", mono 32 px), not
    // the empty buffer (which would collapse any halign to zero).
    let mut ui = ui_at_no_cosmic(NARROW);
    let mut buf = String::new();
    let node = warmup_then(&mut ui, &mut buf, Some(Align::RIGHT), Some("wxyz"));
    let (origin, _) = shape_origins(&ui, node);
    let o = origin.expect("placeholder paints when unfocused + empty");
    let dx = INNER_W - TEXT_W_4CH;
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
    // land on byte 1, proving `handle_input` subtracts the same
    // `align_offset.x` from the local pointer coords.
    let mut ui = ui_at_no_cosmic(NARROW);
    let mut buf = String::from("abcd");
    // Two warmup frames so the second one carries response.rect and
    // the click hit-test runs against the right-aligned layout.
    frame(&mut ui, &mut buf, Some(Align::RIGHT), None);
    frame(&mut ui, &mut buf, Some(Align::RIGHT), None);
    ui.on_input(InputEvent::PointerMoved(glam::Vec2::new(254.0, 20.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    frame(&mut ui, &mut buf, Some(Align::RIGHT), None);
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));
    let id = WidgetId::from_hash("align-ed");
    let caret = ui.state_mut::<TextEditState>(id).caret;
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
        let mut ui = ui_at_no_cosmic(NARROW);
        let mut buf = "a".repeat(100);
        let node = warmup_then(&mut ui, &mut buf, Some(align), None);
        let (origin, _) = shape_origins(&ui, node);
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
    // editor-local x = 5 + 238.
    let mut ui = ui_at_no_cosmic(NARROW);
    let mut buf = String::from("abcd");
    frame(&mut ui, &mut buf, Some(Align::RIGHT), None);
    frame(&mut ui, &mut buf, Some(Align::RIGHT), None);
    click_at(&mut ui, glam::Vec2::new(260.0, 20.0));
    ui.on_input(InputEvent::KeyDown {
        key: Key::Home,
        repeat: false,
    });
    shift_arrow_right(&mut ui);
    shift_arrow_right(&mut ui);
    let node = frame(&mut ui, &mut buf, Some(Align::RIGHT), None);

    // Selection wash is emitted *before* the text shape; pick the
    // first RoundedRect with a `local_rect` in the leaf's stream.
    let first_rounded = shapes_of(ui.forest.tree(Layer::Main), node).find_map(|s| match s {
        ShapeRecord::RoundedRect { local_rect, .. } => *local_rect,
        _ => None,
    });
    let r = first_rounded.expect("selection wash rect present");
    let dx = INNER_W - TEXT_W_4CH;
    assert!(
        (r.min.x - (PAD_L + dx)).abs() < 1e-3,
        "selection wash must align with right-aligned text: x = {}",
        r.min.x,
    );
}

#[test]
fn multiline_default_is_top_left() {
    // Default for `multiline(true)` is `Align::TOP_LEFT`. With "abcd"
    // the text origin sits flush at the inner top-left = padding.
    let mut ui = ui_at_no_cosmic(NARROW);
    let mut buf = String::from("abcd");
    let mut node: Option<NodeId> = None;
    let mut record = |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            node = Some(
                TextEdit::new(&mut buf)
                    .id_salt("align-ed")
                    .multiline(true)
                    .size((Sizing::Fixed(EDIT_W), Sizing::Fixed(80.0)))
                    .show(ui)
                    .node,
            );
        });
    };
    // Two frames: first to warm up the cascade.
    run_at_acked(&mut ui, NARROW, &mut record);
    run_at_acked(&mut ui, NARROW, &mut record);
    let (origin, _) = shape_origins(&ui, node.unwrap());
    let o = origin.expect("text shape");
    assert!((o.x - PAD_L).abs() < 1e-3, "x = {}", o.x);
    assert!((o.y - PAD_T).abs() < 1e-3, "y = {}", o.y);
}
