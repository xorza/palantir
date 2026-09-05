//! Demonstrates the carrier-only state model: the host owns `AppState`
//! and threads `&mut AppState` into the builder closure alongside
//! `&mut Ui`. Widgets that need to read or mutate caller state take it
//! as an explicit parameter — no ambient slot, no borrow conflicts with
//! collection iteration.
//!
//! The second window is the point: it records an entirely separate UI
//! tree from the same `&mut AppState`, so the two counters are one
//! value rather than copies that have to be kept in sync.

use crate::shell;
use crate::support;
use palantir::{Button, Configure, Panel, Sizing, Text, Ui, fmt};

/// State threaded through the entire showcase frame. Lives on the
/// shell's `State` and is handed to [`build`] by the page dispatcher.
#[derive(Debug)]
pub(crate) struct AppState {
    pub(crate) counter: i32,
}

pub(crate) fn build(ui: &mut Ui, app: &mut AppState) {
    support::section(ui, "counter — the value every reader shares", |ui| {
        counter(ui, app);
    });

    support::section(
        ui,
        "nesting — the parameter threads through arbitrary depth",
        |ui| {
            Panel::vstack().id_salt("deep-0").gap(4.0).show(ui, |ui| {
                Panel::hstack().id_salt("deep-1").show(ui, |ui| {
                    Panel::vstack().id_salt("deep-2").show(ui, |ui| {
                        let deep = fmt!(ui, "four levels down, still {}", app.counter);
                        Text::new(deep)
                            .id_salt("deep-readout")
                            .style(&support::note_style())
                            .show(ui);
                    });
                });
            });
        },
    );

    support::section(
        ui,
        "windows — a second OS window recording the same state",
        |ui| {
            support::note(
                ui,
                "The inspector records its own tree from this same &mut AppState \
                 — change the counter in either window and both move. Closing it \
                 from its titlebar stays in sync: the live window set is the \
                 source of truth, so there is no stale bool to track. F8 toggles \
                 it too.",
            );
            let open = ui.window_open(shell::INSPECTOR_WINDOW);
            let label = if open {
                "close inspector window"
            } else {
                "open inspector window"
            };
            if Button::new()
                .id_salt("toggle-inspector")
                .label(label)
                .show(ui)
                .left
                .clicked()
            {
                shell::toggle_inspector(ui);
            }
        },
    );
}

/// The counter itself — recorded by this page and, unchanged, by the
/// inspector window.
pub(crate) fn counter(ui: &mut Ui, app: &mut AppState) {
    Panel::vstack()
        .id_salt("counter-block")
        .size((Sizing::FILL, Sizing::HUG))
        .gap(10.0)
        .show(ui, |ui| {
            let value = fmt!(ui, "counter: {}", app.counter);
            Text::new(value).id_salt("counter-value").show(ui);
            support::row(ui, |ui| {
                if Button::new()
                    .id_salt("dec")
                    .label("−")
                    .min_size((44.0, 0.0))
                    .show(ui)
                    .left
                    .clicked()
                {
                    app.counter -= 1;
                }
                if Button::new()
                    .id_salt("inc")
                    .label("+")
                    .min_size((44.0, 0.0))
                    .show(ui)
                    .left
                    .clicked()
                {
                    app.counter += 1;
                }
                if Button::new()
                    .id_salt("reset")
                    .label("reset")
                    .show(ui)
                    .left
                    .clicked()
                {
                    app.counter = 0;
                }
            });
        });
}
