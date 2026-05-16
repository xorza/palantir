use palantir::{Button, Configure, Panel, Sizing, Text, UiCore};

pub fn build(ui: &mut UiCore) {
    Panel::hstack()
        .auto_id()
        .gap(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            // Left: a normal panel — buttons inside are interactive.
            section(ui, "alive", "alive", false);
            // Right: a disabled panel — disabled cascades, descendants suppress
            // input even though they don't set `disabled` themselves.
            section(ui, "frozen", "frozen", true);
        });
}

fn section(ui: &mut UiCore, id: &'static str, label: &'static str, disabled: bool) {
    Panel::vstack()
        .id_salt(id)
        .size((Sizing::FILL, Sizing::FILL))
        .padding(12.0)
        .gap(8.0)
        .disabled(disabled)
        .show(ui, |ui| {
            Text::new(label).id_salt((id, "label")).show(ui);
            Button::new()
                .id_salt((id, "btn1"))
                .label("click me")
                .show(ui);
            Button::new().id_salt((id, "btn2")).label("or me").show(ui);
        });
}
