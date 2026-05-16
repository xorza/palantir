//! Demonstrates the carrier-only state model: `WinitHost` owns
//! `AppState` and threads `&mut AppState` into the builder closure
//! alongside `&mut Ui`. Widgets that need to read or mutate caller
//! state take it as an explicit parameter — no ambient slot, no
//! borrow conflicts with collection iteration.
//!
//! Buttons mutate the counter; a deeply-nested helper also reads it
//! to prove the parameter threads through arbitrary nesting.

use palantir::{Button, Configure, Panel, Sizing, Text, Ui};

/// State threaded through the entire showcase frame. Lives on `State`
/// in `main.rs` and is handed to `build` by the central dispatcher.
pub struct AppState {
    pub counter: i32,
}

pub fn build(ui: &mut Ui, app: &mut AppState) {
    Panel::vstack()
        .auto_id()
        .gap(12.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Text::new(format!("counter: {}", app.counter))
                .auto_id()
                .show(ui);

            Panel::hstack().auto_id().gap(8.0).show(ui, |ui| {
                if Button::new().id_salt("dec").label("-").show(ui).clicked() {
                    app.counter -= 1;
                }
                if Button::new().id_salt("inc").label("+").show(ui).clicked() {
                    app.counter += 1;
                }
                if Button::new()
                    .id_salt("reset")
                    .label("reset")
                    .show(ui)
                    .clicked()
                {
                    app.counter = 0;
                }
            });

            deeply_nested_reader(ui, app.counter);
        });
}

fn deeply_nested_reader(ui: &mut Ui, counter: i32) {
    Panel::vstack().auto_id().gap(4.0).show(ui, |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::vstack().auto_id().show(ui, |ui| {
                Text::new(format!("(deep) still sees: {counter}"))
                    .auto_id()
                    .show(ui);
            });
        });
    });
}
