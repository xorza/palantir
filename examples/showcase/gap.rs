use palantir::{Background, Color, Configure, Corners, Frame, Panel, Sizing, Ui};

fn tile() -> Color {
    Color::rgb(0.30, 0.55, 0.85)
}

pub fn build(ui: &mut Ui) {
    Panel::vstack()
        .gap(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            row(ui, "0", 0.0);
            row(ui, "8", 8.0);
            row(ui, "24", 24.0);
            row(ui, "48", 48.0);
        });
}

fn row(ui: &mut Ui, id: &'static str, gap: f32) {
    Panel::hstack()
        .with_id(id)
        .size((Sizing::FILL, Sizing::Fixed(48.0)))
        .padding(8.0)
        .gap(gap)
        .background(Background {
            fill: Color::rgb(0.16, 0.18, 0.24),
            radius: Corners::all(4.0),
            ..Default::default()
        })
        .show(ui, |ui| {
            for i in 0..5 {
                Frame::new()
                    .with_id((id, i))
                    .size((Sizing::Fixed(40.0), Sizing::Fixed(32.0)))
                    .background(Background {
                        fill: tile(),
                        radius: Corners::all(4.0),
                        ..Default::default()
                    })
                    .show(ui);
            }
        });
}
