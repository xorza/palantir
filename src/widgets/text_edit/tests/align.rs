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
/// Default `TextEditTheme::caret_width` — the widget reserves this much
/// room at every line's trailing edge so a caret on right/center-aligned
/// text stays inside the clip.
const CARET_W: f32 = 1.5;
const INNER_W: f32 = EDIT_W - 2.0 * PAD_L; // 270
const INNER_H: f32 = EDIT_H - 2.0 * PAD_T; // 34
const ALIGN_W: f32 = INNER_W - CARET_W; // 268.5
const LINE_H: f32 = 19.2; // 16 px font × 1.2 LINE_HEIGHT_MULT
const TEXT_W_4CH: f32 = 32.0; // mono "abcd" width

/// Drive one frame of a single-line editor at `text_align` + buffer +
/// optional placeholder, returning the leaf `NodeId` so the caller
/// can read shapes back. `response.rect` is one frame stale (cascade
/// runs in `post_record`), so callers must warm up at least one
/// frame before reading shapes for align assertions — `warmup_then`
/// below packages that.
fn frame(
    ui: &mut UiCore,
    buf: &mut String,
    text_align: Option<Align>,
    placeholder: Option<&'static str>,
) -> NodeId {
    let mut node: Option<NodeId> = None;
    let mut record = |ui: &mut UiCore| {
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
            node = Some(e.show(ui).node(ui));
        });
    };
    ui.run_at_acked(NARROW, &mut record);
    node.unwrap()
}

/// Two-frame helper: first frame warms up the cascade so the editor
/// has a real `response.rect`; the second frame's shape stream is
/// what every align assertion reads.
fn warmup_then(
    ui: &mut UiCore,
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
fn shape_origins(ui: &UiCore, node: NodeId) -> (Option<glam::Vec2>, Option<glam::Vec2>) {
    let mut text_origin = None;
    let mut caret_origin = None;
    for s in ui.forest.tree(Layer::Main).shapes_of(node) {
        match s {
            ShapeRecord::Text {
                local_origin: Some(o),
                ..
            } => text_origin = Some(*o),
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
fn shift_arrow_right(ui: &mut UiCore) {
    ui.on_input(InputEvent::KeyDown {
        key: Key::ArrowRight,
        repeat: false,
    });
    if let Some(crate::input::keyboard::KeyboardEvent::Down(kp)) =
        ui.input.frame_keyboard_events.last_mut()
    {
        kp.mods = Modifiers {
            shift: true,
            ..Modifiers::NONE
        };
    }
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
    // because 32×19.2 fits inside 268.5×34 (inner − caret reservation).
    let cx = (ALIGN_W - TEXT_W_4CH) * 0.5; // 118.25
    let rx = ALIGN_W - TEXT_W_4CH; // 236.5
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
    // origin shifts right by `ALIGN_W − TEXT_W_4CH`; the caret must
    // shift by the same dx so it sits at the rightmost glyph trailing
    // edge, leaving `CARET_W` of reserved room before the clip edge.
    let mut ui = ui_at_no_cosmic(NARROW);
    let mut buf = String::from("abcd");
    // Warmup so response.rect lands; click; then a final frame so
    // the post-click focus state drives a caret render with the
    // resolved align offset.
    frame(&mut ui, &mut buf, Some(Align::RIGHT), None);
    ui.click_at(glam::Vec2::new(260.0, 20.0));
    ui.on_input(InputEvent::KeyDown {
        key: Key::End,
        repeat: false,
    });
    frame(&mut ui, &mut buf, Some(Align::RIGHT), None);
    let node = frame(&mut ui, &mut buf, Some(Align::RIGHT), None);
    let (text_origin, caret_origin) = shape_origins(&ui, node);
    let t = text_origin.expect("text shape");
    let c = caret_origin.expect("caret rect emitted while focused");
    let dx = ALIGN_W - TEXT_W_4CH; // 236.5
    let dy = (INNER_H - LINE_H) * 0.5; // 7.4
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
    let mut ui = ui_at_no_cosmic(NARROW);
    let mut buf = String::new();
    frame(&mut ui, &mut buf, None, None);
    ui.click_at(glam::Vec2::new(50.0, 20.0));
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
    // editor-local x = PAD_L + (ALIGN_W − TEXT_W_4CH).
    let mut ui = ui_at_no_cosmic(NARROW);
    let mut buf = String::from("abcd");
    frame(&mut ui, &mut buf, Some(Align::RIGHT), None);
    frame(&mut ui, &mut buf, Some(Align::RIGHT), None);
    ui.click_at(glam::Vec2::new(260.0, 20.0));
    ui.on_input(InputEvent::KeyDown {
        key: Key::Home,
        repeat: false,
    });
    shift_arrow_right(&mut ui);
    shift_arrow_right(&mut ui);
    let node = frame(&mut ui, &mut buf, Some(Align::RIGHT), None);

    // Selection wash is emitted *before* the text shape; pick the
    // first RoundedRect with a `local_rect` in the leaf's stream.
    let first_rounded = ui
        .forest
        .tree(Layer::Main)
        .shapes_of(node)
        .find_map(|s| match s {
            ShapeRecord::RoundedRect { local_rect, .. } => *local_rect,
            _ => None,
        });
    let r = first_rounded.expect("selection wash rect present");
    let dx = ALIGN_W - TEXT_W_4CH;
    assert!(
        (r.min.x - (PAD_L + dx)).abs() < 1e-3,
        "selection wash must align with right-aligned text: x = {}",
        r.min.x,
    );
}

/// Per-line halign tests use real cosmic shaping (`ui_with_text`).
/// Asks the shaper directly for caret + selection coords on a
/// wrapped multi-line buffer at different halign values; verifies
/// that line-internal x offsets reflect the encoder convention
/// `dx_per_line = (line_width - line_w) * factor` where factor is
/// 0 (Left), 0.5 (Center), 1.0 (Right).
mod per_line {
    use super::super::*;
    use crate::text::FontFamily;
    use crate::{Align, HAlign};
    use glam::UVec2;

    fn cosmic_ui() -> UiCore {
        UiCore::for_test_at_text(UVec2::new(800, 200))
    }

    #[test]
    fn caret_at_eol_shifts_with_halign_under_wrap() {
        // Wrapped paragraph: line "hi" (2 chars) inside a 300 px
        // wrap target. Caret at the end of the line under
        // `HAlign::Right` must sit at x ≈ wrap_target; under
        // `HAlign::Left` at x ≈ line_w; under `HAlign::Center` at
        // x ≈ (wrap + line_w) / 2.
        let ui = cosmic_ui();
        let fs = 16.0_f32;
        let lh = fs * 1.2;
        let wrap = 300.0_f32;
        let text = "hi";

        let left = ui
            .text
            .cursor_xy(text, 2, fs, lh, Some(wrap), FontFamily::Sans, HAlign::Left)
            .x;
        let center = ui
            .text
            .cursor_xy(
                text,
                2,
                fs,
                lh,
                Some(wrap),
                FontFamily::Sans,
                HAlign::Center,
            )
            .x;
        let right = ui
            .text
            .cursor_xy(text, 2, fs, lh, Some(wrap), FontFamily::Sans, HAlign::Right)
            .x;

        // Right > Center > Left (caret follows the per-line offset).
        assert!(
            right > center,
            "right ({right}) must exceed center ({center})"
        );
        assert!(center > left, "center ({center}) must exceed left ({left})");
        // Right caret sits inside the wrap target (one cap of slack
        // for inter-line trailing whitespace handling).
        assert!(
            right <= wrap + 1.0,
            "right caret ({right}) must be within wrap target ({wrap})",
        );
        // Center caret is roughly midway between left and right.
        let mid_expected = (left + right) * 0.5;
        assert!(
            (center - mid_expected).abs() < 2.0,
            "center caret {center} must be ~mid of left {left} and right {right}",
        );
    }

    #[test]
    fn cache_key_distinguishes_halign() {
        // Cosmic shapes a different buffer for each per-line align.
        // The cache key must reflect that so two simultaneous lookups
        // (e.g. caret then selection) can't pick up the wrong buffer.
        use crate::text::cosmic::CosmicMeasure;
        let mut c = CosmicMeasure::with_bundled_fonts();
        let l = c
            .measure(
                "hi",
                16.0,
                19.2,
                Some(100.0),
                FontFamily::Sans,
                HAlign::Left,
            )
            .key;
        let r = c
            .measure(
                "hi",
                16.0,
                19.2,
                Some(100.0),
                FontFamily::Sans,
                HAlign::Right,
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
        // same buffer. `key_for` collapses `halign_q` to `Auto`'s
        // discriminant on that path so single-line callers don't
        // pay an N-way cache split for identical glyph positions.
        use crate::text::cosmic::CosmicMeasure;
        let mut c = CosmicMeasure::with_bundled_fonts();
        let left = c
            .measure("hi", 16.0, 19.2, None, FontFamily::Sans, HAlign::Left)
            .key;
        let right = c
            .measure("hi", 16.0, 19.2, None, FontFamily::Sans, HAlign::Right)
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
        use crate::forest::tree::Layer;
        let mut ui = cosmic_ui();
        let mut buf = String::from("hi\nyo");
        let mut node = None;
        let mut record = |ui: &mut UiCore| {
            Panel::hstack().auto_id().show(ui, |ui| {
                node = Some(
                    TextEdit::new(&mut buf)
                        .id_salt("fits-ml")
                        .multiline(true)
                        .text_align(Align::TOP_RIGHT)
                        .size((Sizing::Fixed(300.0), Sizing::Fixed(120.0)))
                        .show(ui)
                        .node(ui),
                );
            });
        };
        // Two frames — first warms up `response.rect`, second is the
        // one we inspect.
        ui.run_at_acked(UVec2::new(800, 200), &mut record);
        ui.run_at_acked(UVec2::new(800, 200), &mut record);
        // Read the layout's `ShapedText.key` for the rendered text.
        // `text_spans[node]` indexes one entry per `ShapeRecord::Text`
        // on the leaf; multi-line TextEdit emits a single text shape.
        let node = node.unwrap();
        let main = &ui.layout[Layer::Main];
        let span = main.text_spans[node.index()];
        assert_eq!(span.len, 1, "one Shape::Text expected on the leaf");
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

    /// Regression: `LayoutEngine::shape_text` always re-shapes
    /// through `shape_wrap` for `TextWrap::Wrap` (item 4 in the
    /// per-line-align review). With the slot cache keyed on
    /// `(target_q, halign)`, the layout pipeline must hit that
    /// cache on every steady-state frame — otherwise we'd reshape
    /// on every frame and the per-frame text path becomes O(n) in
    /// glyph count.
    ///
    /// `TextShaper::measure` increments `measure_calls`
    /// unconditionally (even on cosmic-cache hits), so the widget's
    /// own probes (offset measure + cursor_xy) inflate the raw
    /// counter every frame. We instead check the *delta* across
    /// consecutive stable frames is constant — a reshape regression
    /// would bump that delta by one or more.
    #[test]
    fn stable_multiline_holds_constant_per_frame_cost() {
        let mut ui = cosmic_ui();
        let mut buf = String::from("hi\nyo");
        let mut record = |ui: &mut UiCore| {
            Panel::hstack().auto_id().show(ui, |ui| {
                TextEdit::new(&mut buf)
                    .id_salt("stable-ml")
                    .multiline(true)
                    .text_align(Align::TOP_RIGHT)
                    .size((Sizing::Fixed(300.0), Sizing::Fixed(120.0)))
                    .show(ui);
            });
        };
        // Warmup: two frames so `response.rect` lands and every cache
        // is primed.
        ui.run_at_acked(UVec2::new(800, 200), &mut record);
        ui.run_at_acked(UVec2::new(800, 200), &mut record);
        let a = ui.text.measure_calls();
        ui.run_at_acked(UVec2::new(800, 200), &mut record);
        let b = ui.text.measure_calls();
        let per_frame = b - a;
        // Drive several more frames with identical inputs and verify
        // each one costs exactly the same number of `measure_calls`.
        for i in 0..5 {
            let before = ui.text.measure_calls();
            ui.run_at_acked(UVec2::new(800, 200), &mut record);
            let after = ui.text.measure_calls();
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
        use crate::forest::shapes::record::ShapeRecord;
        use crate::forest::tree::Layer;
        let mut ui = cosmic_ui();
        let mut buf = String::new();
        let mut node = None;
        let mut record = |ui: &mut UiCore| {
            Panel::hstack().auto_id().show(ui, |ui| {
                node = Some(
                    TextEdit::new(&mut buf)
                        .id_salt("ph-ml")
                        .multiline(true)
                        .text_align(Align::TOP_RIGHT)
                        .placeholder("type a paragraph here — long enough to actually wrap")
                        .size((Sizing::Fixed(300.0), Sizing::Fixed(120.0)))
                        .show(ui)
                        .node(ui),
                );
            });
        };
        ui.run_at_acked(UVec2::new(800, 200), &mut record);
        ui.run_at_acked(UVec2::new(800, 200), &mut record);
        let node = node.unwrap();
        // (a) `Shape::Text.align` reflects the user's text_align.
        let arena = ui.frame_arena.inner();
        let bytes = arena.fmt_scratch.as_str();
        let tree = ui.forest.tree(Layer::Main);
        let shape_align = tree.shapes_of(node).find_map(|s| match s {
            ShapeRecord::Text { align, text, .. } => Some((*align, text.as_str(bytes).to_owned())),
            _ => None,
        });
        let (shape_align, shape_text) = shape_align.expect("placeholder paints as Shape::Text");
        assert_eq!(shape_align, Align::TOP_RIGHT);
        assert!(
            shape_text.contains("type a paragraph"),
            "rendered text must be the placeholder, got {shape_text:?}",
        );
        // (b) + (c) cached buffer key.
        let main = &ui.layout[Layer::Main];
        let span = main.text_spans[node.index()];
        assert_eq!(span.len, 1, "one Shape::Text expected on the leaf");
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

    /// Regression: an empty multi-line buffer with right-align must
    /// place the caret at the right edge of the wrap target, not at
    /// x = 0. Empty text returns `TextCacheKey::INVALID` and the
    /// shaper's `with_buffer` falls through to the mono path, which
    /// historically ignored halign — caret pinned to the left while
    /// the user expects it to anchor where typed text will appear.
    #[test]
    fn empty_buffer_caret_lands_at_aligned_edge() {
        let ui = cosmic_ui();
        let fs = 16.0_f32;
        let lh = fs * 1.2;
        let wrap = 290.0_f32;
        let right = ui
            .text
            .cursor_xy("", 0, fs, lh, Some(wrap), FontFamily::Sans, HAlign::Right)
            .x;
        let center = ui
            .text
            .cursor_xy("", 0, fs, lh, Some(wrap), FontFamily::Sans, HAlign::Center)
            .x;
        let left = ui
            .text
            .cursor_xy("", 0, fs, lh, Some(wrap), FontFamily::Sans, HAlign::Left)
            .x;
        assert!(
            (right - wrap).abs() < 1e-3,
            "right-aligned empty caret must sit at the wrap target: got {right}",
        );
        assert!(
            (center - wrap * 0.5).abs() < 1e-3,
            "center-aligned empty caret must sit at wrap/2: got {center}",
        );
        assert!(
            left.abs() < 1e-3,
            "left-aligned empty caret at 0: got {left}"
        );
    }

    /// Regression: `MeasureResult.size.w` must extend to the right-
    /// most rendered pixel under per-line align, not to the content
    /// width of the widest visual line. cosmic-text positions
    /// right-aligned glyphs at `(wrap_target - line_w)`, so the
    /// effective bbox reaches `wrap_target`. If `measured.w` stays
    /// at `max(line_w)` (the unaligned content width), the encoder
    /// hands glyphon a `TextBounds` too narrow on the right and
    /// every right-aligned glyph is clipped — the user sees nothing.
    #[test]
    fn measured_width_encloses_aligned_glyphs() {
        use crate::text::cosmic::CosmicMeasure;
        let mut c = CosmicMeasure::with_bundled_fonts();
        let wrap = 290.0_f32;
        let aligned = c.measure(
            "hi\nyo",
            16.0,
            19.2,
            Some(wrap),
            FontFamily::Sans,
            HAlign::Right,
        );
        // The widest visual line content is ~13 px for "hi"; with
        // right-align it sits at x ≈ 277 inside a 290 wrap. Bbox
        // must reach the wrap target (within rounding slop).
        assert!(
            aligned.size.w >= wrap - 1.0,
            "right-aligned bbox width must reach the wrap target: got {} (wrap {})",
            aligned.size.w,
            wrap,
        );
    }

    #[test]
    fn multiline_widget_right_aligns_each_line() {
        // End-to-end: a multi-line TextEdit with `.text_align(RIGHT)`
        // must produce caret coords at end-of-line that approach the
        // wrap target. Pre-existing `block alignment` would have
        // collapsed this to (widest_line - line_w) ≈ 0 for the
        // widest line; per-line alignment offsets each shorter line
        // by `wrap_target - line_w`.
        let mut ui = cosmic_ui();
        let buf_init = String::from("short\nlonger line here");
        let id = WidgetId::from_hash("ml-right");
        let mut buf = buf_init.clone();
        let mut record = |ui: &mut UiCore| {
            Panel::hstack().auto_id().show(ui, |ui| {
                TextEdit::new(&mut buf)
                    .id_salt("ml-right")
                    .multiline(true)
                    .text_align(Align::TOP_RIGHT)
                    .size((Sizing::Fixed(300.0), Sizing::Fixed(120.0)))
                    .show(ui);
            });
        };
        ui.run_at_acked(UVec2::new(800, 200), &mut record);
        // Caret at end of "short" (byte 5): under right-align the
        // caret should sit far from the left edge.
        ui.state_mut::<TextEditState>(id).caret = 5;
        ui.run_at_acked(UVec2::new(800, 200), &mut record);
        // Ask the shaper directly for the caret position the widget
        // would have seen this frame. wrap target = inner width =
        // 300 - 2*5 = 290.
        let fs = 16.0_f32;
        let lh = fs * 1.2;
        let caret_short = ui
            .text
            .cursor_xy(
                &buf,
                5,
                fs,
                lh,
                Some(290.0),
                FontFamily::Sans,
                HAlign::Right,
            )
            .x;
        // Without per-line alignment, the short line would land at
        // x ≈ line_w ≈ 35-40 px. With per-line alignment under
        // right-align, the caret at end-of-short sits near the wrap
        // target (~290).
        assert!(
            caret_short > 200.0,
            "right-aligned 'short' caret at end must be far from 0 (got {caret_short})",
        );
    }
}

#[test]
fn multiline_default_is_top_left() {
    // Default for `multiline(true)` is `Align::TOP_LEFT`. With "abcd"
    // the text origin sits flush at the inner top-left = padding.
    let mut ui = ui_at_no_cosmic(NARROW);
    let mut buf = String::from("abcd");
    let mut node: Option<NodeId> = None;
    let mut record = |ui: &mut UiCore| {
        Panel::hstack().auto_id().show(ui, |ui| {
            node = Some(
                TextEdit::new(&mut buf)
                    .id_salt("align-ed")
                    .multiline(true)
                    .size((Sizing::Fixed(EDIT_W), Sizing::Fixed(80.0)))
                    .show(ui)
                    .node(ui),
            );
        });
    };
    // Two frames: first to warm up the cascade.
    ui.run_at_acked(NARROW, &mut record);
    ui.run_at_acked(NARROW, &mut record);
    let (origin, _) = shape_origins(&ui, node.unwrap());
    let o = origin.expect("text shape");
    assert!((o.x - PAD_L).abs() < 1e-3, "x = {}", o.x);
    assert!((o.y - PAD_T).abs() < 1e-3, "y = {}", o.y);
}
