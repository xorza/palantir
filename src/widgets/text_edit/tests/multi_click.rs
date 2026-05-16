use super::*;

/// Double-click selects the word under the caret; triple-click
/// selects the whole buffer. Pin the multi-click state machine in
/// `handle_input` against the configured time/distance thresholds.
#[test]
fn double_and_triple_click_select_word_and_all() {
    use crate::layout::types::display::Display;
    use std::time::Duration;

    let ed_id = WidgetId::from_hash("multi-ed");
    fn body(ui: &mut Ui<()>, buf: &mut String) {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(buf)
                .id_salt("multi-ed")
                .size((Sizing::Fixed(280.0), Sizing::Fixed(40.0)))
                .show(ui);
        });
    }
    fn frame_at(ui: &mut Ui<()>, now_secs: f32, mut f: impl FnMut(&mut Ui<()>)) {
        let display = Display::from_physical(NARROW, 1.0);
        ui.frame(
            FrameStamp::new(display, Duration::from_secs_f32(now_secs)),
            &mut (),
            |ui| f(ui),
        );
        ui.frame_state.mark_submitted();
    }

    let mut ui = ui_at_no_cosmic(NARROW);
    let mut buf = String::from("hello world");

    // Setup: record once so the editor's rect is known to the next frame.
    frame_at(&mut ui, 0.0, |ui| body(ui, &mut buf));

    // Click 1 at x=32 (mono byte 3, inside "hello").
    ui.on_input(InputEvent::PointerMoved(Vec2::new(32.0, 20.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    frame_at(&mut ui, 0.0, |ui| body(ui, &mut buf));
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));
    frame_at(&mut ui, 0.0, |ui| body(ui, &mut buf));
    let st = ui.state_mut::<TextEditState>(ed_id).clone();
    assert_eq!(st.click_count, 1, "single click");
    assert_eq!(st.caret, 3);
    assert_eq!(st.selection, None);

    // Click 2 at same pos, well inside the window → click_count = 2,
    // selects word at byte 3 → "hello".
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    frame_at(&mut ui, 0.1, |ui| body(ui, &mut buf));
    let st = ui.state_mut::<TextEditState>(ed_id).clone();
    assert_eq!(st.click_count, 2, "double click");
    assert_eq!(st.sel_range(), Some(0..5));
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));
    frame_at(&mut ui, 0.1, |ui| body(ui, &mut buf));

    // Click 3 still inside the window → click_count = 3 → select all.
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    frame_at(&mut ui, 0.2, |ui| body(ui, &mut buf));
    let st = ui.state_mut::<TextEditState>(ed_id).clone();
    assert_eq!(st.click_count, 3, "triple click");
    assert_eq!(st.sel_range(), Some(0..buf.len()));
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));
    frame_at(&mut ui, 0.2, |ui| body(ui, &mut buf));

    // Long pause then another click resets click_count to 1.
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    frame_at(&mut ui, 5.0, |ui| body(ui, &mut buf));
    let st = ui.state_mut::<TextEditState>(ed_id).clone();
    assert_eq!(st.click_count, 1, "pause resets multi-click");
}
