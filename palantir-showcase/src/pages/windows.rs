//! The app and its windows. The carrier-only state model: the host threads `&mut AppState` into
//! the builder closure beside `&mut Ui`, and the second window records a separate tree from the
//! same `AppState`, so both counters are one value. A close request the app can veto
//! (`Ui::close_requested` / `Ui::keep_open`) behind an unsaved-changes toggle.

use crate::app_state::AppState;
use crate::appearance::Appearance;
use crate::job::Job;
use crate::scaffold::api;
use crate::shell;
use crate::support;
use crate::support::{Dialog, Kit, Section};
use palantir::{
    Align, App, Button, Checkbox, ColorButton, ColorPicker, Configure, ProgressBar, SizeSpec, Text,
    TextEdit, Ui, Waker, WidgetId, fmt,
};

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
    Section::new("Shared state")
        .apis(&[api!(type App)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "The app owns these values and passes &mut AppState into the record call beside \
                 &mut Ui, so any depth of widgets can read and write them without a global.",
            );
            support::well(ui, k, |ui| shared(ui, k, app));
        });

    Section::new("Second window")
        .apis(&[api!(Ui::open_window), api!(Ui::is_window_open)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "The inspector records its own tree from this same &mut AppState: change a \
                 value in either window and both show it. The live window set is the source of \
                 truth for whether it is open, so closing it from its title bar needs no bool \
                 kept in step. F8 toggles it too.",
            );
            support::well(ui, k, |ui| {
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
        });

    Section::new("Background job")
        .apis(&[api!(Ui::waker), api!(Waker::wake)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "A worker thread counts through its steps and wakes the window after each: the \
                 bar moves with no frame spent polling, and an idle window draws nothing.",
            );
            support::well(ui, k, |ui| {
                if let Some(job) = &mut app.job {
                    job.reap();
                }
                let running = app.job.as_ref().is_some_and(Job::is_running);
                let done = app.job.as_ref().map_or(0, Job::done);
                support::row(ui, |ui| {
                    if Button::new()
                        .label("Start job")
                        .disabled(running)
                        .show(ui)
                        .clicked()
                    {
                        app.job = Some(Job::start(ui.waker()));
                    }
                    let line = match &app.job {
                        None => ui.intern("not started"),
                        Some(_) if running => fmt!(ui, "step {done} of {}", Job::STEPS),
                        Some(_) => ui.intern("done"),
                    };
                    support::readout(ui, k, "worker", line);
                });
                ProgressBar::new(f32::from(done) / f32::from(Job::STEPS)).show(ui);
            });
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
            support::well(ui, k, |ui| {
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
        });
}

/// Wire into the main window's frame after the page content: with changes pending the OS close
/// is vetoed and a dialog asks; the window closes for real on Discard or Save & Close.
pub(crate) fn intercept(ui: &mut Ui, k: Kit, look: &Appearance) {
    ui.with_state::<ExitState, _>(exit_state_id(), |ui, exit| {
        if ui.close_requested() && exit.pretend_dirty {
            ui.keep_open();
            exit.show_dialog = true;
        }
        if !exit.show_dialog {
            return;
        }
        let accent = look.accent_button(ui.color_scheme());
        let response = Dialog {
            title: "Close with unsaved changes?",
            body: "Save them first, or discard them and close.",
        }
        .show(ui, k, |ui| {
            if Button::new().label("Cancel").show(ui).clicked() {
                ui.close_overlay();
            }
            if Button::new().label("Discard").show(ui).clicked() {
                ui.close_overlay();
                ui.close_window(shell::MAIN_WINDOW);
            }
            if Button::new()
                .label("Save & Close")
                .style(&accent)
                .show(ui)
                .clicked()
            {
                exit.pretend_dirty = false;
                ui.close_overlay();
                ui.close_window(shell::MAIN_WINDOW);
            }
        });
        if response.closed() {
            exit.show_dialog = false;
        }
    });
}

/// The shared values, recorded by this page and the inspector window.
pub(crate) fn shared(ui: &mut Ui, k: Kit, app: &mut AppState) {
    counter(ui, k, app);
    support::row(ui, |ui| {
        TextEdit::new(&mut app.note)
            .size(SizeSpec::FILL_W)
            .max_size((320.0, f32::INFINITY))
            .show(ui);
        ColorButton::new(ColorPicker::new(&mut app.color)).show(ui);
    });
}

fn counter(ui: &mut Ui, k: Kit, app: &mut AppState) {
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
