use glam::Vec2;
use palantir::{Background, Color, Configure, Corners, Frame, Panel, Sizing, TranslateScale, Ui};

fn tile_color() -> Color {
    Color::rgb(0.30, 0.55, 0.85)
}

pub fn build(ui: &mut Ui) {
    Panel::hstack()
        .gap(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            // Translate.
            cell(ui, "translate", |ui| {
                Panel::zstack()
                    .transform(TranslateScale::from_translation(Vec2::new(40.0, 30.0)))
                    .show(ui, |ui| {
                        tile(ui, "t-tile");
                    });
            });

            // Scale (descendants paint at 1.5×, including stroke widths).
            cell(ui, "scale", |ui| {
                Panel::zstack()
                    .transform(TranslateScale::from_scale(1.5))
                    .show(ui, |ui| {
                        tile(ui, "s-tile");
                    });
            });

            // Composed: outer scale 1.25, inner translate (20, 0). Order matters.
            cell(ui, "composed", |ui| {
                Panel::zstack()
                    .with_id("outer")
                    .transform(TranslateScale::from_scale(1.25))
                    .show(ui, |ui| {
                        Panel::zstack()
                            .with_id("inner")
                            .transform(TranslateScale::from_translation(Vec2::new(20.0, 10.0)))
                            .show(ui, |ui| {
                                tile(ui, "c-tile");
                            });
                    });
            });
        });
}

fn cell(ui: &mut Ui, id: &'static str, body: impl FnOnce(&mut Ui)) {
    Panel::vstack()
        .with_id(id)
        .size((Sizing::FILL, Sizing::FILL))
        .padding(12.0)
        .background(Background {
            fill: Color::rgb(0.16, 0.18, 0.24),
            radius: Corners::all(6.0),
            ..Default::default()
        })
        .show(ui, body);
}

fn tile(ui: &mut Ui, id: &'static str) {
    Frame::new()
        .with_id(id)
        .size((Sizing::Fixed(60.0), Sizing::Fixed(60.0)))
        .background(Background {
            fill: tile_color(),
            radius: Corners::all(4.0),
            ..Default::default()
        })
        .show(ui);
}
