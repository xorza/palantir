//! The carrier-only state model: the host threads `&mut AppState` into the builder closure beside `&mut Ui`. The second window records a separate tree from the same `AppState`, so both counters are one value.

use crate::shell;
use crate::support;
use crate::support::{api, note, section};
use palantir::{Align, App, Button, Configure, Text, Ui, fmt};

/// State threaded through the whole showcase frame.
#[derive(Debug)]
pub(crate) struct AppState {
    pub(crate) counter: i32,
}

pub(crate) fn build(ui: &mut Ui, app: &mut AppState) {
    section(ui, "Counter", &[api!(type App)], |ui| {
        note(
            ui,
            "The app owns this value and passes &mut AppState into the record call beside \
             &mut Ui, so any depth of widgets can read and write it without a global.",
        );
        counter(ui, app);
    });

    section(
        ui,
        "Second window",
        &[api!(Ui::open_window), api!(Ui::is_window_open)],
        |ui| {
            note(
                ui,
                "The inspector records its own tree from this same &mut AppState: change the \
                 counter in either window and both move. The live window set is the source of \
                 truth for whether it is open, so closing it from its title bar needs no bool \
                 kept in step. F8 toggles it too.",
            );
            let open = ui.is_window_open(shell::INSPECTOR_WINDOW);
            let label = if open {
                "Close inspector window"
            } else {
                "Open inspector window"
            };
            if Button::new().label(label).show(ui).clicked() {
                shell::toggle_inspector(ui);
            }
        },
    );
}

/// The counter, recorded by this page and the inspector window.
pub(crate) fn counter(ui: &mut Ui, app: &mut AppState) {
    support::row(ui, |ui| {
        if Button::new()
            .label("−")
            .min_size((44.0, 0.0))
            .show(ui)
            .clicked()
        {
            app.counter -= 1;
        }
        let value = fmt!(ui, "{}", app.counter);
        Text::new(value)
            .style(&support::mono_style(20.0, support::INK))
            .min_size((56.0, 0.0))
            .text_align(Align::CENTER)
            .show(ui);
        if Button::new()
            .label("+")
            .min_size((44.0, 0.0))
            .show(ui)
            .clicked()
        {
            app.counter += 1;
        }
        if Button::new().label("Reset").show(ui).clicked() {
            app.counter = 0;
        }
    });
}
