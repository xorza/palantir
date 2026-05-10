use crate::swatch;
use palantir::{Background, Color, Configure, Corners, Frame, Panel, Sizing, Stroke, Ui};

/// Inner-panel background used for padding/margin demos. The whole
/// point of these demos is to *see* where the parent's bounds are
/// relative to its children — without this, padding is invisible.
/// Picked one shade darker than the showcase card (`#343434`) so the
/// boundary reads against the surrounding card.
fn panel_bg() -> Background {
    Background {
        fill: Color::hex(0x252525),
        radius: Corners::all(4.0),
        ..Default::default()
    }
}

fn tile() -> Background {
    Background {
        fill: swatch::A,
        radius: Corners::all(4.0),
        ..Default::default()
    }
}

pub fn build(ui: &mut Ui) {
    Panel::vstack()
        .auto_id()
        .gap(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            // Padding: parent reserves space inside its border before children.
            cell(ui, "padding", |ui| {
                Panel::hstack()
                    .id_salt("p-row")
                    .size((Sizing::FILL, Sizing::Fixed(60.0)))
                    .padding(20.0)
                    .gap(8.0)
                    .background(panel_bg())
                    .show(ui, |ui| {
                        for i in 0..3 {
                            Frame::new()
                                .id_salt(("p", i))
                                .size((Sizing::Fixed(40.0), Sizing::FILL))
                                .background(tile())
                                .show(ui);
                        }
                    });
            });

            // Margin: child shrinks its slot, the surrounding gap is the margin.
            cell(ui, "margin", |ui| {
                Panel::hstack()
                    .id_salt("m-row")
                    .size((Sizing::FILL, Sizing::Fixed(60.0)))
                    .gap(8.0)
                    .background(panel_bg())
                    .show(ui, |ui| {
                        Frame::new()
                            .id_salt("m1")
                            .size((Sizing::Fixed(60.0), Sizing::Fixed(40.0)))
                            .margin(8.0)
                            .background(tile())
                            .show(ui);
                        Frame::new()
                            .id_salt("m2")
                            .size((Sizing::Fixed(60.0), Sizing::Fixed(40.0)))
                            .margin((16.0, 16.0, 0.0, 0.0))
                            .background(tile())
                            .show(ui);
                    });
            });

            // Negative margin: rendered rect spills past its slot. The orange
            // box is anchored after the blue one, but its left margin pulls it
            // backwards 30px so the two overlap.
            cell(ui, "negative margin", |ui| {
                Panel::hstack()
                    .id_salt("neg-row")
                    .size((Sizing::FILL, Sizing::Fixed(60.0)))
                    .padding(8.0)
                    .background(panel_bg())
                    .show(ui, |ui| {
                        Frame::new()
                            .id_salt("neg-a")
                            .size((Sizing::Fixed(80.0), Sizing::Fixed(40.0)))
                            .background(tile())
                            .show(ui);
                        Frame::new()
                            .id_salt("neg-b")
                            .size((Sizing::Fixed(80.0), Sizing::Fixed(40.0)))
                            .margin((-30.0, 0.0, 0.0, 0.0))
                            .background(Background {
                                fill: swatch::B,
                                stroke: Stroke {
                                    width: 1.0,
                                    color: swatch::B,
                                },
                                radius: Corners::all(4.0),
                            })
                            .show(ui);
                    });
            });
        });
}

fn cell(ui: &mut Ui, id: &'static str, body: impl FnOnce(&mut Ui)) {
    Panel::vstack()
        .id_salt(id)
        .gap(4.0)
        .size((Sizing::FILL, Sizing::Hug))
        .show(ui, body);
}
