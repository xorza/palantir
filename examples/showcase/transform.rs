use glam::Vec2;
use palantir::{Color, Element, Frame, HStack, Sizing, Styled, TranslateScale, Ui, VStack, ZStack};

fn tile_color() -> Color {
    Color::rgb(0.30, 0.55, 0.85)
}

pub fn build(ui: &mut Ui) {
    HStack::new()
        .gap(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            // Translate.
            cell(ui, "translate", |ui| {
                ZStack::new()
                    .transform(TranslateScale::from_translation(Vec2::new(40.0, 30.0)))
                    .show(ui, |ui| {
                        tile(ui, "t-tile");
                    });
            });

            // Scale (descendants paint at 1.5×, including stroke widths).
            cell(ui, "scale", |ui| {
                ZStack::new()
                    .transform(TranslateScale::from_scale(1.5))
                    .show(ui, |ui| {
                        tile(ui, "s-tile");
                    });
            });

            // Composed: outer scale 1.25, inner translate (20, 0). Order matters.
            cell(ui, "composed", |ui| {
                ZStack::with_id("outer")
                    .transform(TranslateScale::from_scale(1.25))
                    .show(ui, |ui| {
                        ZStack::with_id("inner")
                            .transform(TranslateScale::from_translation(Vec2::new(20.0, 10.0)))
                            .show(ui, |ui| {
                                tile(ui, "c-tile");
                            });
                    });
            });
        });
}

fn cell(ui: &mut Ui, id: &'static str, body: impl FnOnce(&mut Ui)) {
    VStack::with_id(id)
        .size((Sizing::FILL, Sizing::FILL))
        .padding(12.0)
        .fill(Color::rgb(0.16, 0.18, 0.24))
        .radius(6.0)
        .show(ui, body);
}

fn tile(ui: &mut Ui, id: &'static str) {
    Frame::with_id(id)
        .size((Sizing::Fixed(60.0), Sizing::Fixed(60.0)))
        .fill(tile_color())
        .radius(4.0)
        .show(ui);
}
