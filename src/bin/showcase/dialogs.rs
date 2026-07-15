//! Modal flows: ComboBox dropdown, a confirm Modal, and close-request
//! interception (`Ui::close_requested` / `Ui::keep_open`). The page
//! exposes a toggle standing in for "the document has unsaved
//! changes"; [`intercept`], wired into the window's frame at the top
//! level in `main.rs`, catches the OS close request, vetoes it while
//! changes are pending, and shows a Save / Discard / Cancel dialog
//! instead of letting the window vanish.

use crate::support;
use crate::support::{row, section};
use aperture::{
    Button, Checkbox, ComboBox, Configure, Modal, Panel, Sizing, Text, Ui, WidgetId, WindowToken,
};

#[derive(Default)]
struct State {
    fruit: usize,
    modal_open: bool,
}

/// Shared between the page (writes `pretend_dirty`) and [`intercept`]
/// (reads it, drives `show_dialog`). Keyed on one stable id so both
/// reach the same row regardless of which tab is active.
#[derive(Debug, Default)]
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
    let state_id = state_id();
    let options = ["Apple", "Banana", "Cherry", "Durian", "Elderberry"];

    support::page(ui, |ui| {
        support::header(
            ui,
            "Modal flows — a dropdown, a confirm dialog, and OS close-request \
             interception.",
        );

        section(
            ui,
            "combo",
            "ComboBox — click to open the dropdown",
            |ui| {
                row(ui, "combo-row", |ui| {
                    let mut fruit = ui.state_mut::<State>(state_id).fruit;
                    ComboBox::new(&mut fruit, &options)
                        .size((Sizing::Fixed(180.0), Sizing::Hug))
                        .id_salt("combo")
                        .show(ui);
                    ui.state_mut::<State>(state_id).fruit = fruit;
                    let chosen = ui.fmt(format_args!("selected: {}", options[fruit]));
                    Text::new(chosen).id_salt("chosen").show(ui);
                });
            },
        );

        section(
            ui,
            "modal",
            "Modal — dims the background; Esc or backdrop click closes",
            |ui| {
                if Button::new()
                    .id_salt("open")
                    .label("Open dialog")
                    .show(ui)
                    .left
                    .clicked()
                {
                    ui.state_mut::<State>(state_id).modal_open = true;
                }
            },
        );

        section(
            ui,
            "exit",
            "close interception — toggle 'unsaved changes', then close the window: \
             the app vetoes via ui.keep_open() and prompts instead",
            |ui| {
                let id = exit_state_id();
                let mut dirty = ui.state_mut::<ExitState>(id).pretend_dirty;
                Checkbox::new(&mut dirty)
                    .id_salt("dirty")
                    .label("simulate unsaved changes")
                    .show(ui);
                ui.state_mut::<ExitState>(id).pretend_dirty = dirty;
            },
        );
    });

    if ui.state_mut::<State>(state_id).modal_open {
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
                        ui.state_mut::<State>(state_id).modal_open = false;
                    }
                    if Button::new()
                        .id_salt("ok")
                        .label("Delete")
                        .show(ui)
                        .left
                        .clicked()
                    {
                        ui.state_mut::<State>(state_id).modal_open = false;
                    }
                });
            });
        });
        if resp.dismissed {
            ui.state_mut::<State>(state_id).modal_open = false;
        }
    }
}

/// Wire into the window's frame after the page content. With no pending
/// changes the OS close proceeds untouched; with changes it vetoes and
/// prompts. `win` is the window closed for real once the user confirms.
pub(crate) fn intercept(ui: &mut Ui, win: WindowToken) {
    let id = exit_state_id();
    if ui.close_requested() && ui.state_mut::<ExitState>(id).pretend_dirty {
        ui.keep_open();
        ui.state_mut::<ExitState>(id).show_dialog = true;
    }
    if !ui.state_mut::<ExitState>(id).show_dialog {
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
                            let s = ui.state_mut::<ExitState>(id);
                            s.pretend_dirty = false;
                            s.show_dialog = false;
                            ui.close_window(win);
                        }
                        if Button::new()
                            .id_salt(("exit", "discard"))
                            .label("Discard")
                            .show(ui)
                            .left
                            .clicked()
                        {
                            ui.state_mut::<ExitState>(id).show_dialog = false;
                            ui.close_window(win);
                        }
                        if Button::new()
                            .id_salt(("exit", "cancel"))
                            .label("Cancel")
                            .show(ui)
                            .left
                            .clicked()
                        {
                            ui.state_mut::<ExitState>(id).show_dialog = false;
                        }
                    });
            });
    });
    if resp.dismissed {
        ui.state_mut::<ExitState>(id).show_dialog = false;
    }
}
