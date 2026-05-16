//! Demos the always-on explicit-WidgetId-collision overlay. Each row
//! deliberately reuses the same `.id_salt(...)` across siblings; the
//! framework disambiguates the duplicates (so state stays intact) and
//! paints a magenta 3px outline over every offender.

use palantir::{Background, Button, Color, Configure, Frame, Panel, Sizing, Text, UiCore};

pub fn build(ui: &mut UiCore) {
    Panel::vstack()
        .auto_id()
        .gap(16.0)
        .padding(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Text::new(
                "Each row below reuses an explicit id across two or more sibling widgets. \
                 They get disambiguated and outlined in magenta — no panic, state survives.",
            )
            .wrapping()
            .show(ui);

            row(ui, |ui| {
                Button::new()
                    .id_salt("idcol-dup-btn")
                    .label("dup A")
                    .show(ui);
                Button::new()
                    .id_salt("idcol-dup-btn")
                    .label("dup B")
                    .show(ui);
                Button::new()
                    .id_salt("idcol-dup-btn")
                    .label("dup C")
                    .show(ui);
            });

            row(ui, |ui| {
                Frame::new()
                    .background(Background::fill(Color::hex(0x3a4a5c)))
                    .id_salt("idcol-dup-frame")
                    .size(60.0)
                    .show(ui);
                Frame::new()
                    .background(Background::fill(Color::hex(0xddaa44)))
                    .id_salt("idcol-dup-frame")
                    .size(60.0)
                    .show(ui);
            });

            row(ui, |ui| {
                Button::new()
                    .id_salt("idcol-clean-a")
                    .label("clean A")
                    .show(ui);
                Button::new()
                    .id_salt("idcol-clean-b")
                    .label("clean B")
                    .show(ui);
            });
        });
}

fn row(ui: &mut UiCore, body: impl FnOnce(&mut UiCore)) {
    Panel::hstack()
        .auto_id()
        .size((Sizing::FILL, Sizing::Hug))
        .gap(8.0)
        .show(ui, body);
}
