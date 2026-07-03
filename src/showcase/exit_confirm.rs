//! Demonstrates close-request interception (`Ui::close_requested` /
//! `Ui::keep_open`). The tab exposes a toggle standing in for "the
//! document has unsaved changes"; [`intercept`], wired into the window's
//! frame at the top level, catches the OS close request, vetoes it while
//! changes are pending, and shows a Save / Discard / Cancel dialog instead
//! of letting the window vanish.

use palantir::{
    Button, Checkbox, Configure, Modal, Panel, Sizing, Text, TextWrap, Ui, WidgetId, WindowToken,
};

/// Shared between the tab (writes `pretend_dirty`) and [`intercept`]
/// (reads it, drives `show_dialog`). Keyed on one stable id so both reach
/// the same row regardless of which tab is active.
#[derive(Debug, Default)]
struct ExitState {
    /// Stand-in for "unsaved changes exist".
    pretend_dirty: bool,
    /// Whether the confirm-on-exit dialog is currently up.
    show_dialog: bool,
}

fn state_id() -> WidgetId {
    WidgetId::from_hash("showcase::exit_confirm::state")
}

pub fn build(ui: &mut Ui) {
    let id = state_id();
    Panel::vstack()
        .auto_id()
        .gap(12.0)
        .padding(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Text::new("Close-request interception")
                .id_salt(("exit", "title"))
                .show(ui);
            Text::new(
                "Toggle 'unsaved changes', then click the window's close button. \
                 The host surfaces the OS close request as ui.close_requested(); \
                 the app calls ui.keep_open() to veto it and shows a dialog instead \
                 of the window disappearing. With the toggle off, close works \
                 normally.",
            )
            .id_salt(("exit", "desc"))
            .text_wrap(TextWrap::Wrap)
            .show(ui);

            let mut dirty = ui.state_mut::<ExitState>(id).pretend_dirty;
            Checkbox::new(&mut dirty)
                .id_salt(("exit", "dirty"))
                .label("simulate unsaved changes")
                .show(ui);
            ui.state_mut::<ExitState>(id).pretend_dirty = dirty;
        });
}

/// Wire into the window's frame after the page content. With no pending
/// changes the OS close proceeds untouched; with changes it vetoes and
/// prompts. `win` is the window closed for real once the user confirms.
pub fn intercept(ui: &mut Ui, win: WindowToken) {
    let id = state_id();
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
                            .clicked()
                        {
                            ui.state_mut::<ExitState>(id).show_dialog = false;
                            ui.close_window(win);
                        }
                        if Button::new()
                            .id_salt(("exit", "cancel"))
                            .label("Cancel")
                            .show(ui)
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
