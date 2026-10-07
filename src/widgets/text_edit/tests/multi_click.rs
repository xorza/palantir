use crate::widgets::text_edit::tests::*;

/// Double-click selects the word, triple-click the buffer; dispatch follows the input layer's `press_count` run (within `DOUBLE_CLICK_WINDOW`/`DOUBLE_CLICK_RADIUS` on the event-time clock, hence the idle frame before the pause press).
#[test]
fn double_and_triple_click_select_word_and_all() {
    fn body(ui: &mut Ui, buf: &mut String) {
        Panel::hstack().auto_id().show(ui, |ui| {
            TextEdit::new(buf)
                .id(WidgetId::from_hash("multi-ed"))
                .size((Sizing::fixed(280.0), Sizing::fixed(40.0)))
                .show(ui);
        });
    }

    use std::time::Duration;

    let ed_id = WidgetId::from_hash("multi-ed");

    let mut h = UiHarness::new(NARROW);
    let mut buf = String::from("hello world");

    h.at(Duration::from_secs_f32(0.0))
        .frame(|ui| body(ui, &mut buf));

    h.press_at(Vec2::new(32.0, 20.0));
    h.at(Duration::from_secs_f32(0.0))
        .frame(|ui| body(ui, &mut buf));
    h.release();
    h.at(Duration::from_secs_f32(0.0))
        .frame(|ui| body(ui, &mut buf));
    let st = h.state::<TextEditState>(ed_id).clone();
    assert_eq!(st.edit.caret, 3, "single click places the caret");
    assert_eq!(st.edit.selection, None);

    h.press();
    h.at(Duration::from_secs_f32(0.1))
        .frame(|ui| body(ui, &mut buf));
    let st = h.state::<TextEditState>(ed_id).clone();
    assert_eq!(st.edit.sel_range(), Some(0..5), "double click selects word");
    h.release();
    h.at(Duration::from_secs_f32(0.1))
        .frame(|ui| body(ui, &mut buf));

    h.press();
    h.at(Duration::from_secs_f32(0.2))
        .frame(|ui| body(ui, &mut buf));
    let st = h.state::<TextEditState>(ed_id).clone();
    assert_eq!(
        st.edit.sel_range(),
        Some(0..buf.len()),
        "triple click selects all"
    );
    h.release();
    h.at(Duration::from_secs_f32(0.2))
        .frame(|ui| body(ui, &mut buf));

    // An idle frame advances the event clock; the next click restarts the run.
    h.at(Duration::from_secs_f32(5.0))
        .frame(|ui| body(ui, &mut buf));
    h.press();
    h.at(Duration::from_secs_f32(5.0))
        .frame(|ui| body(ui, &mut buf));
    let st = h.state::<TextEditState>(ed_id).clone();
    assert_eq!(st.edit.caret, 3, "pause resets the run to a single click");
    assert_eq!(
        st.edit.selection, None,
        "no selection after the reset press"
    );
}
