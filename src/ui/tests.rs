use crate::Ui;
use crate::widgets::{Button, Panel};

#[test]
fn duplicate_widget_id_traces_but_does_not_panic() {
    let mut ui = Ui::new();
    ui.begin_frame();
    Panel::hstack().show(&mut ui, |ui| {
        Button::with_id("dup").show(ui);
        Button::with_id("dup").show(ui);
    });
    assert_eq!(ui.tree.nodes.len(), 3);
}
