//! The app and its windows. The carrier-only state model: the host threads `&mut AppState` into
//! the builder closure beside `&mut Ui`, and the second window records a separate tree from the
//! same `AppState`, so both counters are one value. A close request the app can veto
//! (`Ui::close_requested` / `Ui::keep_open`) behind an unsaved-changes toggle.

use crate::scaffold::api;
use crate::shell;
use crate::support;
use crate::support::{Kit, Section};
use palantir::{Align, App, Button, Checkbox, Configure, Text, Ui, WidgetId, WindowToken, fmt};

/// State threaded through the whole showcase frame.
#[derive(Debug)]
pub(crate) struct AppState {
    pub(crate) counter: i32,
}

/// The close interception's state, in a row the page and [`intercept`] both reach.
#[derive(Clone, Copy, Debug, Default)]
struct ExitState {
    pretend_dirty: bool,
    show_dialog: bool,
}

fn exit_state_id() -> WidgetId {
    WidgetId::from_hash("showcase::windows::exit-state")
}

pub(crate) fn build(ui: &mut Ui, k: Kit, app: &mut AppState) {
    Section::new("Counter")
        .apis(&[api!(type App)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "The app owns this value and passes &mut AppState into the record call beside \
             &mut Ui, so any depth of widgets can read and write it without a global.",
            );
            counter(ui, k, app);
        });

    Section::new("Second window")
        .apis(&[api!(Ui::open_window), api!(Ui::is_window_open)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
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
        });

    Section::new("Close interception")
        .apis(&[api!(Ui::close_requested), api!(Ui::keep_open)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "Turn on the unsaved changes, then close the window: the app vetoes the OS \
                 request and asks first.",
            );
            ui.with_state::<ExitState, _>(exit_state_id(), |ui, exit| {
                Checkbox::new(&mut exit.pretend_dirty)
                    .label("simulate unsaved changes")
                    .show(ui);
            });
            // The OS close button needs a real window, which no harness drives.
            support::checklist(
                ui,
                &[
                    "With no unsaved changes, closing the window closes it",
                    "With unsaved changes, closing the window shows the dialog instead",
                    "Cancel keeps the window open, and Discard closes it",
                ],
            );
        });
}

/// Wire into the frame after the page content: with changes pending the OS close is vetoed and
/// prompts; `win` is closed for real on confirm.
pub(crate) fn intercept(ui: &mut Ui, win: WindowToken) {
    ui.with_state::<ExitState, _>(exit_state_id(), |ui, exit| exit_dialog(ui, win, exit));
}

fn exit_dialog(ui: &mut Ui, win: WindowToken, exit: &mut ExitState) {
    if ui.close_requested() && exit.pretend_dirty {
        ui.keep_open();
        exit.show_dialog = true;
    }
    if !exit.show_dialog {
        return;
    }
    let response = support::dialog(
        ui,
        "You have unsaved changes. Close anyway?",
        |ui, close| {
            if Button::new().label("Save & Close").show(ui).clicked() {
                exit.pretend_dirty = false;
                close.close();
                ui.close_window(win);
            }
            if Button::new().label("Discard").show(ui).clicked() {
                close.close();
                ui.close_window(win);
            }
            if Button::new().label("Cancel").show(ui).clicked() {
                close.close();
            }
        },
    );
    if response.closed() {
        exit.show_dialog = false;
    }
}

/// The counter, recorded by this page and the inspector window.
pub(crate) fn counter(ui: &mut Ui, k: Kit, app: &mut AppState) {
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
            .style(&k.t.mono(k.t.ink).with_font_size(20.0))
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
