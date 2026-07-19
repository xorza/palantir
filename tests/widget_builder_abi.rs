use aperture::{Button, Checkbox, ComboBox, DragValue, RadioButton, Switch, TextEdit, Theme};

fn consume<T>(value: T) {
    std::hint::black_box(value);
}

#[test]
fn borrowed_style_builders_move_by_value_across_the_crate_boundary() {
    let theme = Theme::default();
    let mut checked = false;
    let mut switched = false;
    let mut selected = 0;
    let options = ["one", "two"];
    let mut number = 42_i64;
    let mut radio = 0_u8;
    let mut text = String::from("text");

    consume(Button::new().style(&theme.button));
    consume(Checkbox::new(&mut checked).style(&theme.checkbox));
    consume(Switch::new(&mut switched).style(&theme.switch));
    consume(ComboBox::new(&mut selected, &options).style(&theme.button));
    consume(DragValue::new(&mut number).style(&theme.drag_value));
    consume(RadioButton::new(&mut radio, 1).style(&theme.radio));
    consume(TextEdit::new(&mut text).style(&theme.text_edit));
}
