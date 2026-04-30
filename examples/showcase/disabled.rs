use palantir::{Button, Color, Element, HStack, Sizing, Stroke, Ui, VStack, ZStack};

pub fn build(ui: &mut Ui) {
    HStack::new()
        .gap(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            // Left: a normal panel — buttons inside are interactive.
            panel(ui, "alive", false);
            // Right: a disabled panel — disabled cascades, descendants suppress
            // input even though they don't set `disabled` themselves.
            panel(ui, "frozen", true);
        });
}

fn panel(ui: &mut Ui, id: &'static str, disabled: bool) {
    ZStack::with_id(id)
        .size((Sizing::FILL, Sizing::FILL))
        .padding(12.0)
        .fill(Color::rgb(0.16, 0.18, 0.24))
        .stroke(Stroke {
            width: 1.0,
            color: Color::rgb(0.30, 0.36, 0.46),
        })
        .radius(8.0)
        .disabled(disabled)
        .show(ui, |ui| {
            VStack::with_id((id, "stack"))
                .size((Sizing::FILL, Sizing::Hug))
                .gap(8.0)
                .show(ui, |ui| {
                    Button::with_id((id, "btn1")).label("click me").show(ui);
                    Button::with_id((id, "btn2")).label("or me").show(ui);
                });
        });
}
