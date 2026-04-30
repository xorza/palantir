use crate::Ui;
use crate::shape::Shape;
use crate::widgets::{Button, HStack};

#[test]
fn shapes_attached_to_button_node() {
    let mut ui = Ui::new();
    ui.begin_frame();
    let mut button_node = None;
    HStack::new().show(&mut ui, |ui| {
        button_node = Some(Button::new().label("X").show(ui).node);
    });

    let shapes = ui.tree.shapes_of(button_node.unwrap());
    assert_eq!(shapes.len(), 2);
    assert!(matches!(shapes[0], Shape::RoundedRect { .. }));
    assert!(matches!(shapes[1], Shape::Text { .. }));
}
