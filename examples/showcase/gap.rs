use palantir::{Color, Element, Frame, HStack, Sizing, Styled, Ui, VStack};

fn tile() -> Color {
    Color::rgb(0.30, 0.55, 0.85)
}

pub fn build(ui: &mut Ui) {
    VStack::new()
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
    HStack::with_id(id)
        .size((Sizing::FILL, Sizing::Fixed(48.0)))
        .padding(8.0)
        .gap(gap)
        .fill(Color::rgb(0.16, 0.18, 0.24))
        .radius(4.0)
        .show(ui, |ui| {
            for i in 0..5 {
                Frame::with_id((id, i))
                    .size((Sizing::Fixed(40.0), Sizing::Fixed(32.0)))
                    .fill(tile())
                    .radius(4.0)
                    .show(ui);
            }
        });
}
