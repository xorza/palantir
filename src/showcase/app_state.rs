//! Demonstrates `Ui::app::<T>()` — ambient caller-owned state
//! installed once at the frame boundary via `Host::frame`
//! and reached by deep widgets without threading `&mut T` through
//! every closure. Buttons mutate the counter; a deeply-nested helper
//! also reads it to prove the install crosses arbitrary nesting and
//! closure boundaries.

use palantir::{Button, Configure, Panel, Sizing, Text, Ui};

/// State threaded through the entire showcase frame. Lives on `State`
/// in `main.rs` and is installed via `Host::frame`.
pub struct AppState {
    pub counter: i32,
}

pub fn build(ui: &mut Ui<AppState>) {
    Panel::vstack()
        .auto_id()
        .gap(12.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            let value = ui.app().counter;
            Text::new(format!("counter: {value}")).auto_id().show(ui);

            Panel::hstack().auto_id().gap(8.0).show(ui, |ui| {
                if Button::new().id_salt("dec").label("-").show(ui).clicked() {
                    ui.app().counter -= 1;
                }
                if Button::new().id_salt("inc").label("+").show(ui).clicked() {
                    ui.app().counter += 1;
                }
                if Button::new()
                    .id_salt("reset")
                    .label("reset")
                    .show(ui)
                    .clicked()
                {
                    ui.app().counter = 0;
                }
            });

            deeply_nested_reader(ui);
        });
}

fn deeply_nested_reader(ui: &mut Ui<AppState>) {
    Panel::vstack().auto_id().gap(4.0).show(ui, |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::vstack().auto_id().show(ui, |ui| {
                let v = ui.app().counter;
                Text::new(format!("(deep) still sees: {v}"))
                    .auto_id()
                    .show(ui);
            });
        });
    });
}
