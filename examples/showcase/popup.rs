//! Popup-layer showcase. A trigger button toggles `MenuState.open`;
//! while open, `Popup::anchored_to(...).auto_id()` records a side root in the
//! `Popup` layer that paints above the main tree, escapes ancestor
//! clip, and hit-tests on top.
//!
//! NOTE: this example records the popup *inline* — `Popup::show` is
//! called from inside the central `Panel::show` body. v1 of
//! `Ui::layer` rejects mid-recording and will panic on the trigger
//! click. v2 (end-frame reorder, see `docs/popups.md`) lands the
//! mid-recording mechanic; this file is the pinned target for that
//! work.

use palantir::{
    Background, Button, Color, Configure, Corners, Panel, Popup, Rect, Size, Sizing, Stroke, Text,
    Ui, WidgetId,
};

#[derive(Default)]
struct MenuState {
    open: bool,
    last_choice: Option<&'static str>,
}

pub fn build(ui: &mut Ui) {
    let menu_id = WidgetId::from_hash("popup-root");

    let mut trigger_rect: Option<Rect> = None;
    let mut clicked = false;

    Panel::vstack()
        .id_salt("popup-root")
        .gap(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Text::new(
                "Click the button to open a popup. The popup paints \
                      above the main tree on the Popup layer.",
            )
            .auto_id()
            .show(ui);

            Panel::hstack()
                .id_salt("popup-trigger-row")
                .gap(12.0)
                .show(ui, |ui| {
                    let r = Button::new()
                        .id_salt("popup-trigger")
                        .label("menu")
                        .show(ui);
                    if r.clicked() {
                        clicked = true;
                    }
                    trigger_rect = r.rect();

                    let label = ui
                        .state_mut::<MenuState>(menu_id)
                        .last_choice
                        .unwrap_or("(no selection yet)");
                    Text::new(label).auto_id().show(ui);
                });
        });

    if clicked {
        let s = ui.state_mut::<MenuState>(menu_id);
        s.open = !s.open;
    }

    let open = ui.state_mut::<MenuState>(menu_id).open;
    if !open {
        return;
    }

    let Some(trigger) = trigger_rect else {
        return;
    };

    let anchor = Rect {
        min: glam::Vec2::new(trigger.min.x, trigger.min.y + trigger.size.h + 4.0),
        size: Size::new(220.0, 400.0),
    };

    let mut chosen: Option<&'static str> = None;
    let resp = Popup::anchored_to(anchor)
        .id_salt("popup-showcase.menu")
        .padding(6.0)
        .background(Background {
            fill: Color::hex(0x2a2a2a),
            stroke: Stroke {
                width: 1.0,
                color: Color::hex(0x4a4a4a),
            },
            radius: Corners::all(6.0),
        })
        .show(ui, |ui| {
            for label in ["copy", "paste", "delete"] {
                if Button::new()
                    .id_salt(("popup-item", label))
                    .label(label)
                    .size((Sizing::FILL, Sizing::Hug))
                    .show(ui)
                    .clicked()
                {
                    chosen = Some(label);
                }
            }
        });

    let s = ui.state_mut::<MenuState>(menu_id);
    if let Some(label) = chosen {
        s.last_choice = Some(label);
        s.open = false;
    } else if resp.dismissed {
        // Outside click — close the popup.
        s.open = false;
    }
}
