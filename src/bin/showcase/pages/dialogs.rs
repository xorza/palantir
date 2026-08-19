//! Modal flows: a ComboBox dropdown, a confirm Modal, and close-request
//! interception (`Ui::close_requested` / `Ui::keep_open`). The page
//! exposes a toggle standing in for "the document has unsaved changes";
//! [`intercept`], wired into the window's frame at the top level in the
//! shell, catches the OS close request, vetoes it while changes are
//! pending, and shows a Save / Discard / Cancel dialog instead of
//! letting the window vanish.

use crate::support;
use crate::support::{note_style, row, section};
use palantir::{
    Button, Checkbox, ComboBox, Configure, Modal, Panel, Sizing, Text, Ui, WidgetId, WindowToken,
    fmt,
};

#[derive(Clone, Copy, Default, Debug)]
struct State {
    fruit: usize,
    modal_open: bool,
}

/// Shared between the page (writes `pretend_dirty`) and [`intercept`]
/// (reads it, drives `show_dialog`). Keyed on one stable id so both
/// reach the same row regardless of which page is open.
#[derive(Clone, Copy, Debug, Default)]
struct ExitState {
    /// Stand-in for "unsaved changes exist".
    pretend_dirty: bool,
    /// Whether the confirm-on-exit dialog is currently up.
    show_dialog: bool,
}

fn state_id() -> WidgetId {
    WidgetId::from_hash("showcase::dialogs::state")
}

fn exit_state_id() -> WidgetId {
    WidgetId::from_hash("showcase::dialogs::exit-state")
}

pub(crate) fn build(ui: &mut Ui) {
    // Both rows are read and written from several nested closures below, so
    // each is lent to the whole page body once. Probing the map at each use
    // was sixteen lookups a frame to move four bytes around.
    ui.with_state::<State, _>(state_id(), |ui, state| {
        ui.with_state::<ExitState, _>(exit_state_id(), |ui, exit| page(ui, state, exit))
    });
}

fn page(ui: &mut Ui, state: &mut State, exit: &mut ExitState) {
    let options = ["Apple", "Banana", "Cherry", "Durian", "Elderberry"];

    section(ui, "combo box — click to open the dropdown", |ui| {
        row(ui, |ui| {
            ComboBox::new(&mut state.fruit, &options)
                .size((Sizing::fixed(180.0), Sizing::HUG))
                .id_salt("combo")
                .show(ui);
            let chosen = fmt!(ui, "selected: {}", options[state.fruit]);
            Text::new(chosen)
                .id_salt("chosen")
                .style(&note_style())
                .show(ui);
        });
    });

    section(
        ui,
        "modal — dims the background and takes every pointer; Esc or a backdrop \
         click closes",
        |ui| {
            if Button::new()
                .id_salt("open")
                .label("Open dialog")
                .show(ui)
                .left
                .clicked()
            {
                state.modal_open = true;
            }
        },
    );

    section(
        ui,
        "close interception — the app decides whether the window may go away",
        |ui| {
            support::note(
                ui,
                "Turn on 'unsaved changes', then close the window: the app vetoes \
                 the OS request via ui.keep_open() and prompts instead of vanishing.",
            );
            Checkbox::new(&mut exit.pretend_dirty)
                .id_salt("dirty")
                .label("simulate unsaved changes")
                .show(ui);
        },
    );

    if state.modal_open {
        let resp = Modal::new().id_salt("confirm-modal").show(ui, |ui| {
            Panel::vstack().id_salt("mbody").gap(16.0).show(ui, |ui| {
                Text::new("Delete all the things?")
                    .id_salt("mtitle")
                    .show(ui);
                Panel::hstack().id_salt("mrow").gap(8.0).show(ui, |ui| {
                    if Button::new()
                        .id_salt("cancel")
                        .label("Cancel")
                        .show(ui)
                        .left
                        .clicked()
                    {
                        state.modal_open = false;
                    }
                    if Button::new()
                        .id_salt("ok")
                        .label("Delete")
                        .show(ui)
                        .left
                        .clicked()
                    {
                        state.modal_open = false;
                    }
                });
            });
        });
        if resp.dismissed {
            state.modal_open = false;
        }
    }
}

/// Wire into the window's frame after the page content. With no pending
/// changes the OS close proceeds untouched; with changes it vetoes and
/// prompts. `win` is the window closed for real once the user confirms.
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

    let resp = Modal::new().id_salt(("exit", "modal")).show(ui, |ui| {
        Panel::vstack()
            .id_salt(("exit", "body"))
            .gap(16.0)
            .show(ui, |ui| {
                Text::new("You have unsaved changes. Close anyway?")
                    .id_salt(("exit", "q"))
                    .show(ui);
                Panel::hstack()
                    .id_salt(("exit", "row"))
                    .gap(8.0)
                    .show(ui, |ui| {
                        if Button::new()
                            .id_salt(("exit", "save"))
                            .label("Save & Close")
                            .show(ui)
                            .left
                            .clicked()
                        {
                            exit.pretend_dirty = false;
                            exit.show_dialog = false;
                            ui.close_window(win);
                        }
                        if Button::new()
                            .id_salt(("exit", "discard"))
                            .label("Discard")
                            .show(ui)
                            .left
                            .clicked()
                        {
                            exit.show_dialog = false;
                            ui.close_window(win);
                        }
                        if Button::new()
                            .id_salt(("exit", "cancel"))
                            .label("Cancel")
                            .show(ui)
                            .left
                            .clicked()
                        {
                            exit.show_dialog = false;
                        }
                    });
            });
    });
    if resp.dismissed {
        exit.show_dialog = false;
    }
}
