//! ContextMenu showcase. Right-click the labeled surface or the
//! button trigger; both auto-open via `ContextMenu::attach`. Items
//! report `clicked()` and the menu auto-closes on click, outside-
//! click, or Esc.

use palantir::{
    Background, Button, Color, Configure, ContextMenu, Corners, Frame, MenuItem, Panel, Sense,
    Sizing, Stroke, Text, Ui, WidgetId,
};

#[derive(Default)]
struct State {
    last_action: Option<&'static str>,
}

pub fn build(ui: &mut Ui) {
    let state_id = WidgetId::from_hash("ctx-menu-showcase");

    Panel::vstack()
        .id_salt("ctx-menu-root")
        .gap(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Text::new(
                "Right-click the surface or the button. Click an item, click \
                 outside, or press Esc to dismiss.",
            )
            .auto_id()
            .show(ui);

            // Status line. Static strings only — no per-frame alloc.
            let label = ui
                .state_mut::<State>(state_id)
                .last_action
                .unwrap_or("last action: (none yet)");
            Text::new(label).id_salt("ctx-menu-status").show(ui);

            // Trigger 1: a button. `ContextMenu::attach` opens on
            // secondary_clicked at the pointer position.
            Panel::hstack()
                .id_salt("ctx-menu-button-row")
                .gap(12.0)
                .show(ui, |ui| {
                    let trigger = Button::new()
                        .id_salt("ctx-menu-button-trigger")
                        .label("right-click me")
                        .show(ui);
                    attach_menu(ui, &trigger, state_id);
                });

            // Trigger 2: a generic Frame surface (Sense::CLICK so it
            // can receive secondary clicks).
            let surface = Frame::new()
                .id_salt("ctx-menu-surface")
                .size((Sizing::FILL, Sizing::Fixed(160.0)))
                .sense(Sense::CLICK)
                .background(Background {
                    fill: Color::hex(0x2a2a2a).into(),
                    stroke: Stroke::solid(Color::hex(0x4a4a4a), 1.0),
                    radius: Corners::all(6.0),
                })
                .show(ui);
            attach_menu(ui, &surface, state_id);
        });
}

fn attach_menu(ui: &mut Ui, trigger: &palantir::Response, state_id: WidgetId) {
    ContextMenu::attach(ui, trigger)
    .show(ui, |ui| {
        if MenuItem::new("Copy").shortcut("⌘C").show(ui).clicked() {
            ui.state_mut::<State>(state_id).last_action = Some("last action: Copy");
        }
        if MenuItem::new("Cut").shortcut("⌘X").show(ui).clicked() {
            ui.state_mut::<State>(state_id).last_action = Some("last action: Cut");
        }
        if MenuItem::new("Paste").shortcut("⌘V").show(ui).clicked() {
            ui.state_mut::<State>(state_id).last_action = Some("last action: Paste");
        }
        MenuItem::separator(ui);
        MenuItem::new("Disabled").enabled(false).show(ui);
        MenuItem::separator(ui);
        if MenuItem::new("Delete").shortcut("⌫").show(ui).clicked() {
            ui.state_mut::<State>(state_id).last_action = Some("last action: Delete");
        }
    });
}
