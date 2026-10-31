//! The session study: the night's settings as a form, and the plan as a list that reorders.

use palantir::{
    Align, Button, ComboBox, Configure, DragValue, Key, Modal, Panel, RadioButton, Sense, Shortcut,
    ShortcutMods, Sizing, Slider, Switch, Text, Ui, VAlign, WidgetId, fmt,
};

use crate::app::model::Observatory;
use crate::app::model::catalog::ObjectId;
use crate::app::model::session::{Binning, CAMERAS, FILTERS};
use crate::app::studies::Study;
use crate::scaffold::api;
use crate::scaffold::header::Header;
use crate::scaffold::tokens::Tokens;

/// Each plan row's height, which a handle's drag counts rows in.
const ROW_HEIGHT: f32 = 32.0;

pub(crate) fn header(ui: &mut Ui, t: Tokens) {
    Header {
        title: "Session",
        blurb: "The night's settings as a form, and the plan in imaging order. Reorder an entry \
                with Alt+↑/↓ or its handle; clearing the plan asks first.",
        keys: &["Tab", "Alt+↑", "Alt+↓", "Delete", "Enter", "Esc"],
        apis: &[
            api!(type palantir::ComboBox),
            api!(type palantir::DragValue),
            api!(type palantir::Slider),
            api!(type palantir::Switch),
            api!(type palantir::RadioButton),
            api!(type palantir::Modal),
        ],
        related_label: "showcase",
        related: Study::Session.showcase_pages(),
        source: "examples/observatory/app/studies/session.rs",
    }
    .show(ui, t);
}

/// What the study keeps between frames: whether the clear dialog is open, and a handle's drag
/// not yet a whole row, and whose handle it is.
#[derive(Debug, Default)]
struct State {
    confirming: bool,
    dragging: Option<ObjectId>,
    drag: f32,
}

pub(crate) fn show(ui: &mut Ui, model: &mut Observatory, t: Tokens) {
    let state_id = WidgetId::from_hash("observatory::session::state");
    ui.with_state::<State, _>(state_id, |ui, state| {
        Panel::hstack()
            .id_salt("session")
            .size((Sizing::FILL, Sizing::FILL))
            .gap(24.0)
            .show(ui, |ui| {
                form(ui, model, t);
                plan(ui, model, state, t);
            });
        if state.confirming {
            confirm(ui, model, state, t);
        }
    });
}

fn field(ui: &mut Ui, label: &'static str, t: Tokens, body: impl FnOnce(&mut Ui)) {
    Panel::hstack()
        .id_salt(label)
        .size((Sizing::FILL, Sizing::HUG))
        .gap(12.0)
        .child_align(Align::v(VAlign::Center))
        .show(ui, |ui| {
            Text::new(label)
                .style(&t.dim())
                .size((Sizing::fixed(110.0), Sizing::HUG))
                .show(ui);
            body(ui);
        });
}

fn form(ui: &mut Ui, model: &mut Observatory, t: Tokens) {
    let s = &mut model.session;
    Panel::vstack()
        .id_salt("form")
        .size((Sizing::fixed(420.0), Sizing::HUG))
        .padding(16.0)
        .gap(12.0)
        .chrome(t.card())
        .show(ui, |ui| {
            Text::new("INSTRUMENT").style(&t.heading()).show(ui);
            field(ui, "Camera", t, |ui| {
                ComboBox::new(&mut s.camera, &CAMERAS)
                    .size((Sizing::fixed(200.0), Sizing::HUG))
                    .show(ui);
            });
            field(ui, "Filter", t, |ui| {
                ComboBox::new(&mut s.filter, &FILTERS)
                    .size((Sizing::fixed(200.0), Sizing::HUG))
                    .show(ui);
            });
            field(ui, "Exposure", t, |ui| {
                DragValue::new(&mut s.exposure_s)
                    .range(1.0..=600.0)
                    .decimals(0)
                    .suffix(" s")
                    .size((Sizing::fixed(120.0), Sizing::HUG))
                    .show(ui);
            });
            field(ui, "Gain", t, |ui| {
                Slider::new(&mut s.gain, 0.0..=300.0)
                    .step(1.0)
                    .size((Sizing::fixed(200.0), Sizing::HUG))
                    .show(ui);
                Text::new(fmt!(ui, "{:.0}", s.gain))
                    .style(&t.mono(t.ink))
                    .show(ui);
            });
            field(ui, "Cooling", t, |ui| {
                Switch::new(&mut s.cooling).show(ui);
                DragValue::new(&mut s.target_c)
                    .range(-30.0..=20.0)
                    .decimals(0)
                    .suffix(" °C")
                    .disabled(!s.cooling)
                    .size((Sizing::fixed(100.0), Sizing::HUG))
                    .show(ui);
            });
            field(ui, "Binning", t, |ui| {
                for binning in Binning::ALL {
                    RadioButton::new(&mut s.binning, binning)
                        .id_salt(binning.label())
                        .label(binning.label())
                        .show(ui);
                }
            });
        });
}

/// What a plan row asks this frame.
#[derive(Clone, Copy, Debug)]
enum RowAction {
    Up,
    Down,
    Remove,
    Select,
}

fn plan(ui: &mut Ui, model: &mut Observatory, state: &mut State, t: Tokens) {
    Panel::vstack()
        .id_salt("plan")
        .size((Sizing::FILL, Sizing::FILL))
        .padding(16.0)
        .gap(8.0)
        .chrome(t.card())
        .show(ui, |ui| {
            Panel::hstack()
                .id_salt("plan head")
                .size((Sizing::FILL, Sizing::HUG))
                .gap(12.0)
                .child_align(Align::v(VAlign::Center))
                .show(ui, |ui| {
                    Text::new("PLAN").style(&t.heading()).show(ui);
                    let frames: i64 = model.plan.entries().iter().map(|e| e.frames).sum();
                    let total = whole_seconds(model.plan.integration_s(model.session.exposure_s));
                    let (hours, minutes) = (total / 3600, total / 60 % 60);
                    Text::new(fmt!(ui, "{frames} frames · {hours}h {minutes:02}m"))
                        .id_salt("total")
                        .style(&t.mono(t.ink))
                        .size((Sizing::FILL, Sizing::HUG))
                        .show(ui);
                    let empty = model.plan.entries().is_empty();
                    if Button::new()
                        .label("Clear plan…")
                        .disabled(empty)
                        .show(ui)
                        .clicked()
                    {
                        state.confirming = true;
                    }
                });
            if model.plan.entries().is_empty() {
                Text::new("Add targets from the chart's or the catalog's menu.")
                    .style(&t.dim())
                    .show(ui);
                return;
            }
            let mut pending = None;
            for idx in 0..model.plan.entries().len() {
                if let Some(action) = row(ui, model, state, idx, t) {
                    pending = Some((idx, action));
                }
            }
            if let Some((idx, action)) = pending {
                apply(model, idx, action);
            }
        });
}

#[expect(
    clippy::cast_sign_loss,
    reason = "a sum of frame counts times a positive exposure is not negative"
)]
const fn whole_seconds(seconds: f64) -> u64 {
    seconds.round() as u64
}

/// A plan row's id, keyed by its object, so the row's widgets, their focus and a handle's drag
/// move with the entry when it moves.
pub(crate) fn row_id(object: ObjectId) -> WidgetId {
    WidgetId::from_hash(("observatory::session::row", object.0))
}

fn row(
    ui: &mut Ui,
    model: &mut Observatory,
    state: &mut State,
    idx: usize,
    t: Tokens,
) -> Option<RowAction> {
    let entry = model.plan.entries()[idx];
    let id = row_id(entry.object);
    let focused = ui.is_focus_within(id);
    let mut action = None;
    let name = &model.catalog.get(entry.object).name;
    let selected = model.selection == Some(entry.object);
    Panel::hstack()
        .id(id)
        .size((Sizing::FILL, Sizing::fixed(ROW_HEIGHT)))
        .gap(10.0)
        .child_align(Align::v(VAlign::Center))
        .show(ui, |ui| {
            // The handle: a drag of a whole row's height moves the entry one place.
            let handle = Text::new("\u{2630}")
                .id_salt("handle")
                .style(&t.dim())
                .sense(Sense::DRAG)
                .show(ui);
            if let Some(delta) = handle.left.drag.delta() {
                state.dragging = Some(entry.object);
                state.drag += delta.y;
            }
            if state.dragging == Some(entry.object) {
                if state.drag >= ROW_HEIGHT {
                    state.drag -= ROW_HEIGHT;
                    action = Some(RowAction::Down);
                } else if state.drag <= -ROW_HEIGHT {
                    state.drag += ROW_HEIGHT;
                    action = Some(RowAction::Up);
                }
                if !handle.left.drag.is_live() {
                    state.dragging = None;
                    state.drag = 0.0;
                }
            }
            Text::new(fmt!(ui, "{}", idx + 1))
                .style(&t.mono(t.ink_dim))
                .size((Sizing::fixed(24.0), Sizing::HUG))
                .text_align(Align::RIGHT)
                .show(ui);
            if Button::new()
                .id_salt("name")
                .label(fmt!(ui, "{name}"))
                .selected(selected)
                .size((Sizing::fixed(120.0), Sizing::HUG))
                .show(ui)
                .clicked()
            {
                action = Some(RowAction::Select);
            }
            let entry = model.plan.entry_mut(idx);
            ComboBox::new(&mut entry.filter, &FILTERS)
                .id_salt("filter")
                .size((Sizing::fixed(90.0), Sizing::HUG))
                .show(ui);
            DragValue::new(&mut entry.frames)
                .id_salt("frames")
                .range(1.0..=999.0)
                .suffix(" frames")
                .size((Sizing::fixed(120.0), Sizing::HUG))
                .show(ui);
            if Button::new()
                .id_salt("remove")
                .label("\u{d7}")
                .show(ui)
                .clicked()
            {
                action = Some(RowAction::Remove);
            }
        });
    if focused {
        if ui.shortcut_pressed(Shortcut::new(ShortcutMods::ALT, Key::ArrowUp)) {
            action = Some(RowAction::Up);
        }
        if ui.shortcut_pressed(Shortcut::new(ShortcutMods::ALT, Key::ArrowDown)) {
            action = Some(RowAction::Down);
        }
        if ui.key_pressed(Key::Delete) {
            action = Some(RowAction::Remove);
        }
    }
    action
}

/// Apply a row's action; a moved entry keeps its focus, as its row's id follows the object.
fn apply(model: &mut Observatory, idx: usize, action: RowAction) {
    match action {
        RowAction::Up => {
            model.plan.move_up(idx);
        }
        RowAction::Down => {
            model.plan.move_down(idx);
        }
        RowAction::Remove => {
            model.plan.remove(idx);
        }
        RowAction::Select => {
            let object = model.plan.entries()[idx].object;
            model.select(object);
        }
    }
}

/// The dialog that asks before the plan goes: Enter clears, Escape or Cancel keeps it. Its Clear
/// button takes the focus as it opens, so Enter presses it, as WAI-ARIA's dialog pattern puts
/// first focus on the element a person most likely wants
/// (<https://www.w3.org/WAI/ARIA/apg/patterns/dialog-modal/>).
fn confirm(ui: &mut Ui, model: &mut Observatory, state: &mut State, t: Tokens) {
    let count = model.plan.entries().len();
    let mut confirm = None;
    let response = Modal::new().id_salt("clear").show(ui, |ui, close| {
        let mut cleared = false;
        Panel::vstack().gap(16.0).show(ui, |ui| {
            Text::new(fmt!(ui, "Clear all {count} targets of the plan?"))
                .style(&t.body())
                .show(ui);
            Panel::hstack().gap(8.0).show(ui, |ui| {
                if Button::new()
                    .id_salt("cancel")
                    .label("Cancel")
                    .show(ui)
                    .clicked()
                {
                    close.close();
                }
                let clear = Button::new().id_salt("confirm").label("Clear").show(ui);
                confirm = Some(clear.id);
                if clear.clicked() {
                    cleared = true;
                    close.close();
                }
            });
        });
        cleared
    });
    if response.opened
        && let Some(id) = confirm
    {
        ui.set_focus(id);
    }
    if response.inner {
        model.clear_plan();
    }
    if response.closed() {
        state.confirming = false;
    }
}
