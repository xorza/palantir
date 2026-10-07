//! Modal flows: a confirm Modal and close-request interception (`Ui::close_requested` / `Ui::keep_open`) behind an unsaved-changes toggle.

use crate::support::{api, checklist, note, readout, row, section};
use palantir::{
    Button, Checkbox, CloseHandle, Configure, Modal, OverlayResponse, Panel, Text, Ui, WidgetId,
    WindowToken,
};

#[derive(Clone, Copy, Default, Debug)]
struct State {
    modal_open: bool,
    last: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, Default)]
struct ExitState {
    pretend_dirty: bool,
    show_dialog: bool,
}

fn exit_state_id() -> WidgetId {
    WidgetId::from_hash("showcase::dialogs::exit-state")
}

#[track_caller]
fn dialog(
    ui: &mut Ui,
    title: &'static str,
    buttons: impl FnOnce(&mut Ui, &CloseHandle),
) -> OverlayResponse<()> {
    Modal::new().auto_id().show(ui, |ui, close| {
        Panel::vstack().gap(16.0).show(ui, |ui| {
            Text::new(title).show(ui);
            Panel::hstack().gap(8.0).show(ui, |ui| buttons(ui, close));
        });
    })
}

pub(crate) fn build(ui: &mut Ui) {
    let state_id = WidgetId::from_hash("showcase::dialogs::state");
    ui.with_state::<State, _>(state_id, |ui, state| {
        ui.with_state::<ExitState, _>(exit_state_id(), |ui, exit| page(ui, state, exit));
    });
}

fn page(ui: &mut Ui, state: &mut State, exit: &mut ExitState) {
    section(
        ui,
        "Modal",
        &[api!(Modal::new), api!(CloseHandle::close)],
        |ui| {
            note(
                ui,
                "A modal dims the window and takes every pointer. Escape or a click on the \
             backdrop closes it, and Tab stays inside it — see the focus & keyboard page \
             for the focus side.",
            );
            row(ui, |ui| {
                if Button::new().label("Delete all…").show(ui).clicked() {
                    state.modal_open = true;
                }
                readout(ui, "last answer", state.last.unwrap_or("—"));
            });
            checklist(
                ui,
                &[
                    "The backdrop dims the page, and nothing under it reacts to the pointer",
                    "Escape and a backdrop click close the dialog, and leave the last answer as it was",
                ],
            );
        },
    );

    section(
        ui,
        "Close interception",
        &[api!(Ui::close_requested), api!(Ui::keep_open)],
        |ui| {
            note(
                ui,
                "Turn on the unsaved changes, then close the window: the app vetoes the OS \
                 request and asks first.",
            );
            Checkbox::new(&mut exit.pretend_dirty)
                .label("simulate unsaved changes")
                .show(ui);
            checklist(
                ui,
                &[
                    "With no unsaved changes, closing the window closes it",
                    "With unsaved changes, closing the window shows the dialog instead",
                    "Cancel keeps the window open, and Discard closes it",
                ],
            );
        },
    );

    if state.modal_open {
        let resp = dialog(ui, "Delete all the things?", |ui, close| {
            for label in ["Cancel", "Delete"] {
                if Button::new().id_salt(label).label(label).show(ui).clicked() {
                    state.last = Some(label);
                    close.close();
                }
            }
        });
        if resp.closed() {
            state.modal_open = false;
        }
    }
}

/// Wire into the frame after the page content: with changes pending the OS close is vetoed and prompts; `win` is closed for real on confirm.
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

    let resp = dialog(
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
    if resp.closed() {
        exit.show_dialog = false;
    }
}
