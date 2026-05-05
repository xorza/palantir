use crate::swatch;
use palantir::{Background, Color, Configure, Corners, Frame, Panel, Sizing, Ui};

pub fn build(ui: &mut Ui) {
    Panel::hstack()
        .gap(12.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            cell(ui, "HStack", |ui| {
                Panel::hstack().gap(6.0).show(ui, |ui| {
                    sw(ui, "h-a", 40.0, 40.0, swatch::A);
                    sw(ui, "h-b", 40.0, 40.0, swatch::A);
                    sw(ui, "h-c", 40.0, 40.0, swatch::A);
                });
            });
            cell(ui, "VStack", |ui| {
                Panel::vstack().gap(6.0).show(ui, |ui| {
                    sw(ui, "v-a", 60.0, 24.0, swatch::A);
                    sw(ui, "v-b", 60.0, 24.0, swatch::A);
                    sw(ui, "v-c", 60.0, 24.0, swatch::A);
                });
            });
            cell(ui, "ZStack", |ui| {
                Panel::zstack().show(ui, |ui| {
                    sw(ui, "z-back", 80.0, 80.0, swatch::A);
                    sw(ui, "z-front", 50.0, 50.0, swatch::B);
                });
            });
            cell(ui, "Canvas", |ui| {
                Panel::canvas()
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        positioned(ui, "p1", 10.0, 10.0, swatch::A);
                        positioned(ui, "p2", 60.0, 30.0, swatch::B);
                        positioned(ui, "p3", 30.0, 70.0, swatch::C);
                    });
            });
        });
}

/// Plain layout cell: padding + gap, no decoration.
fn cell(ui: &mut Ui, id: &'static str, body: impl FnOnce(&mut Ui)) {
    Panel::vstack()
        .with_id(id)
        .size((Sizing::FILL, Sizing::FILL))
        .padding(12.0)
        .gap(8.0)
        .show(ui, body);
}

fn sw(ui: &mut Ui, id: &'static str, w: f32, h: f32, c: Color) {
    Frame::new()
        .with_id(id)
        .size((Sizing::Fixed(w), Sizing::Fixed(h)))
        .background(Background {
            fill: c,
            radius: Corners::all(4.0),
            ..Default::default()
        })
        .show(ui);
}

fn positioned(ui: &mut Ui, id: &'static str, x: f32, y: f32, c: Color) {
    Frame::new()
        .with_id(id)
        .position((x, y))
        .size(40.0)
        .background(Background {
            fill: c,
            ..Default::default()
        })
        .show(ui);
}
