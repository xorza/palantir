use crate::widgets::text_edit::tests::*;

/// Double-click selects the word under the caret; triple-click
/// selects the whole buffer. Pins `handle_input`'s dispatch on the
/// input layer's `press_count` run (chained within
/// `DOUBLE_CLICK_WINDOW`/`DOUBLE_CLICK_RADIUS`, classified with the
/// event-time frame clock — hence the idle frame before the "pause"
/// press below, standing in for the frames a real host runs between
/// gestures).
#[test]
fn double_and_triple_click_select_word_and_all() {
    use std::time::Duration;

    let ed_id = WidgetId::from_hash("multi-ed");
    fn body(ui: &mut Ui, buf: &mut String) {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(buf)
                .id(WidgetId::from_hash("multi-ed"))
                .size((Sizing::fixed(280.0), Sizing::fixed(40.0)))
                .show(ui);
        });
    }
    fn frame_at(h: &mut UiHarness, now_secs: f32, mut f: impl FnMut(&mut Ui)) {
        h.at(Duration::from_secs_f32(now_secs)).frame(|ui| f(ui));
    }

    let mut h = ui_at_no_cosmic(NARROW);
    let mut buf = String::from("hello world");

    // Setup: record once so the editor's rect is known to the next frame.
    frame_at(&mut h, 0.0, |ui| body(ui, &mut buf));

    // Click 1 at x=32 (mono byte 3, inside "hello").
    h.press_at(Vec2::new(32.0, 20.0));
    frame_at(&mut h, 0.0, |ui| body(ui, &mut buf));
    h.on_input(InputEvent::PointerReleased(PointerButton::Left));
    frame_at(&mut h, 0.0, |ui| body(ui, &mut buf));
    let st = h.ui.state_mut::<TextEditState>(ed_id).clone();
    assert_eq!(st.edit.caret, 3, "single click places the caret");
    assert_eq!(st.edit.selection, None);

    // Click 2 at same pos, well inside the window → double press,
    // selects word at byte 3 → "hello".
    h.on_input(InputEvent::PointerPressed(PointerButton::Left));
    frame_at(&mut h, 0.1, |ui| body(ui, &mut buf));
    let st = h.ui.state_mut::<TextEditState>(ed_id).clone();
    assert_eq!(st.edit.sel_range(), Some(0..5), "double click selects word");
    h.on_input(InputEvent::PointerReleased(PointerButton::Left));
    frame_at(&mut h, 0.1, |ui| body(ui, &mut buf));

    // Click 3 still inside the window → triple press → select all.
    h.on_input(InputEvent::PointerPressed(PointerButton::Left));
    frame_at(&mut h, 0.2, |ui| body(ui, &mut buf));
    let st = h.ui.state_mut::<TextEditState>(ed_id).clone();
    assert_eq!(
        st.edit.sel_range(),
        Some(0..buf.len()),
        "triple click selects all"
    );
    h.on_input(InputEvent::PointerReleased(PointerButton::Left));
    frame_at(&mut h, 0.2, |ui| body(ui, &mut buf));

    // Long pause (an idle frame advances the event clock, as a real
    // host's frames would), then another click restarts the run:
    // plain caret placement, no selection.
    frame_at(&mut h, 5.0, |ui| body(ui, &mut buf));
    h.on_input(InputEvent::PointerPressed(PointerButton::Left));
    frame_at(&mut h, 5.0, |ui| body(ui, &mut buf));
    let st = h.ui.state_mut::<TextEditState>(ed_id).clone();
    assert_eq!(st.edit.caret, 3, "pause resets the run to a single click");
    assert_eq!(
        st.edit.selection, None,
        "no selection after the reset press"
    );
}
