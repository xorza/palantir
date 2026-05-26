use super::*;

/// Fixed-size editor: scroll offset stays at zero while text fits, grows
/// to keep the caret visible once content overflows the inner width, and
/// snaps back when the caret returns home. Mono fallback (8 px / char @
/// 16 px font, 1.5 px caret) gives predictable math; the editor's inner
/// width is `280 − 2·(5 + 1.5)` = 267 px (theme default padding 5 px each
/// side, plus the 1.5 px chrome stroke that `Tree::open_node` folds into
/// padding — TextEdit mirrors that fold so glyph coords land on the
/// encoder's clip rect).
#[test]
fn scroll_keeps_caret_inside_visible_inner_rect() {
    let ed_id = WidgetId::from_hash("scroll-ed");
    fn body(ui: &mut Ui, buf: &mut String) {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(buf)
                .id(WidgetId::from_hash("scroll-ed"))
                .size((Sizing::Fixed(280.0), Sizing::Fixed(40.0)))
                .show(ui);
        });
    }

    let mut ui = ui_at_no_cosmic(NARROW);

    // Short text: caret at end (5) → x = 40 px ≤ inner_w. No scroll.
    let mut buf = String::from("hello");
    ui.run_at_acked(NARROW, |ui| body(ui, &mut buf));
    ui.state_mut::<TextEditState>(ed_id).caret = 5;
    ui.run_at_acked(NARROW, |ui| body(ui, &mut buf));
    let scroll = ui.state_mut::<TextEditState>(ed_id).scroll;
    assert_eq!(scroll, Vec2::ZERO, "text fits — no scroll");

    // Long text past inner_w: caret at end (100) → x = 800 px.
    // Trailing clamp leaves a caret-width sliver inside the scissor:
    // scroll.x = caret_right (800 + 1.5) − (inner_w − caret_width) =
    // 801.5 − (267 − 1.5) = 536.
    let mut long = "a".repeat(100);
    ui.run_at_acked(NARROW, |ui| body(ui, &mut long));
    ui.state_mut::<TextEditState>(ed_id).caret = 100;
    ui.run_at_acked(NARROW, |ui| body(ui, &mut long));
    let scroll = ui.state_mut::<TextEditState>(ed_id).scroll;
    assert!((scroll.x - 536.0).abs() < 0.5, "scroll.x = {}", scroll.x);
    assert_eq!(scroll.y, 0.0, "single-line never scrolls y");

    // Caret home: scroll.x snaps back so the start of the text is
    // visible again.
    ui.state_mut::<TextEditState>(ed_id).caret = 0;
    ui.run_at_acked(NARROW, |ui| body(ui, &mut long));
    let scroll = ui.state_mut::<TextEditState>(ed_id).scroll;
    assert_eq!(scroll.x, 0.0, "scroll snaps to 0 when caret moves home");
}

/// A `Hug`-width single-line editor must show its *whole* text as it
/// grows under the caret — no permanent left clip. Pins two coupled
/// fixes: (1) the editor reserves the caret sliver in its desired width
/// so a content-hugging field is wide enough for an end-of-text caret,
/// and (2) `update_scroll` upper-clamps `scroll.x` to the content end so
/// the transient scroll — caused by the one-frame-stale rect lagging the
/// field's own growth — settles back to zero instead of sticking.
/// Mono fallback: 8 px/char @ 16 px font, 1.5 px caret, theme padding
/// 5 px + 1.5 px stroke fold each side.
#[test]
fn hug_width_editor_shows_full_text_after_growth() {
    let ed_id = WidgetId::from_hash("hug-ed");
    fn body(ui: &mut Ui, buf: &mut String) {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(buf)
                .id(WidgetId::from_hash("hug-ed"))
                .size((Sizing::Hug, Sizing::Fixed(40.0)))
                .show(ui);
        });
    }

    let mut ui = ui_at_no_cosmic(WIDE);

    // Start narrow so the Hug width settles small (rect ≈ one glyph).
    let mut buf = String::from("1");
    ui.run_at_acked(WIDE, |ui| body(ui, &mut buf));
    ui.state_mut::<TextEditState>(ed_id).caret = 1;
    ui.run_at_acked(WIDE, |ui| body(ui, &mut buf));

    // Grow the buffer with the caret pinned at the end. The first frame
    // still sees last frame's narrower rect and scrolls left to chase the
    // caret — exactly the transient that used to stick.
    buf.push_str("2345");
    ui.state_mut::<TextEditState>(ed_id).caret = buf.len();
    ui.run_at_acked(WIDE, |ui| body(ui, &mut buf));
    // Settle: the widened rect is now visible to update_scroll.
    ui.run_at_acked(WIDE, |ui| body(ui, &mut buf));

    let scroll = ui.state_mut::<TextEditState>(ed_id).scroll;
    assert_eq!(
        scroll.x, 0.0,
        "hug editor must show its whole text (no left clip); scroll.x = {}",
        scroll.x,
    );
}

/// After horizontal scroll kicks in, clicking the left edge of the
/// widget must hit the byte that's *visibly* at the left edge — not
/// byte 0. Pins that `handle_input` adds `state.scroll` back into the
/// hit-test coords.
#[test]
fn click_hit_test_compensates_for_scroll() {
    let ed_id = WidgetId::from_hash("hit-ed");
    fn body(ui: &mut Ui, buf: &mut String) {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(buf)
                .id(WidgetId::from_hash("hit-ed"))
                .size((Sizing::Fixed(280.0), Sizing::Fixed(40.0)))
                .show(ui);
        });
    }

    let mut ui = ui_at_no_cosmic(NARROW);
    let mut buf = "a".repeat(100);

    // Drive caret to end so the editor scrolls all the way right.
    ui.run_at_acked(NARROW, |ui| body(ui, &mut buf));
    ui.state_mut::<TextEditState>(ed_id).caret = 100;
    ui.run_at_acked(NARROW, |ui| body(ui, &mut buf));
    let scroll_x = ui.state_mut::<TextEditState>(ed_id).scroll.x;
    assert!(scroll_x > 100.0, "precondition: editor is scrolled");

    // Click 8 px into the widget (right at the left edge of the
    // inner rect). With scroll compensation, mono hit-test sees x =
    // scroll_x (≈ 537.5), which lands on byte ≈ 67 (scroll_x / 8).
    // Without compensation it'd land on byte 0.
    ui.on_input(InputEvent::PointerMoved(Vec2::new(8.0, 20.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.run_at_acked(NARROW, |ui| body(ui, &mut buf));
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));
    ui.run_at_acked(NARROW, |ui| body(ui, &mut buf));

    let caret = ui.state_mut::<TextEditState>(ed_id).caret;
    let expected = (scroll_x / 8.0).round() as usize;
    assert!(
        caret.abs_diff(expected) <= 1,
        "click should land near byte {expected} (visible left edge), got {caret}",
    );
}
