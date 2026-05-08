use crate::swatch;
use palantir::{Background, Color, Configure, Corners, Frame, Panel, Sizing, Ui};

pub fn build(ui: &mut Ui) {
    Panel::vstack()
        .auto_id()
        .gap(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            // Row 1: Fixed sizes — exact pixels, ignores parent.
            row(ui, "fixed", |ui| {
                fixed_box(ui, "fx-50", 50.0, swatch::B);
                fixed_box(ui, "fx-100", 100.0, swatch::B);
                fixed_box(ui, "fx-200", 200.0, swatch::B);
            });

            // Row 2: Hug — child's content drives size. Padded frames hug
            // their empty content box (effectively just padding).
            row(ui, "hug", |ui| {
                hug_box(ui, "h-1", 20.0);
                hug_box(ui, "h-2", 40.0);
            });

            // Row 3: Fill — split leftover by weight. 1 : 2 : 1.
            row(ui, "fill", |ui| {
                fill_box(ui, "f-1", 1.0);
                fill_box(ui, "f-2", 2.0);
                fill_box(ui, "f-3", 1.0);
            });
        });
}

fn row(ui: &mut Ui, id: &'static str, body: impl FnOnce(&mut Ui)) {
    Panel::hstack()
        .id_salt(id)
        .gap(8.0)
        .size((Sizing::FILL, Sizing::Hug))
        .show(ui, body);
}

fn swatch_bg(c: Color) -> Background {
    Background {
        fill: c,
        radius: Corners::all(4.0),
        ..Default::default()
    }
}

fn fixed_box(ui: &mut Ui, id: &'static str, w: f32, c: Color) {
    Frame::new()
        .id_salt(id)
        .size((Sizing::Fixed(w), Sizing::Fixed(40.0)))
        .background(swatch_bg(c))
        .show(ui);
}

fn hug_box(ui: &mut Ui, id: &'static str, pad_x: f32) {
    Frame::new()
        .id_salt(id)
        .size((Sizing::Hug, Sizing::Fixed(40.0)))
        .padding((pad_x, 0.0, pad_x, 0.0))
        .background(swatch_bg(swatch::C))
        .show(ui);
}

fn fill_box(ui: &mut Ui, id: &'static str, weight: f32) {
    Frame::new()
        .id_salt(id)
        .size((Sizing::Fill(weight), Sizing::Fixed(40.0)))
        .background(swatch_bg(swatch::A))
        .show(ui);
}
