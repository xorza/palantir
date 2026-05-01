use palantir::{Color, Element, Frame, HStack, Justify, Sizing, Styled, Ui, VStack};

fn tile() -> Color {
    Color::rgb(0.30, 0.55, 0.85)
}

pub fn build(ui: &mut Ui) {
    VStack::new()
        .gap(10.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            row(ui, "start", Justify::Start);
            row(ui, "center", Justify::Center);
            row(ui, "end", Justify::End);
            row(ui, "between", Justify::SpaceBetween);
            row(ui, "around", Justify::SpaceAround);
        });
}

fn row(ui: &mut Ui, id: &'static str, j: Justify) {
    HStack::with_id(id)
        .size((Sizing::FILL, Sizing::Fixed(40.0)))
        .padding((6.0, 4.0, 6.0, 4.0))
        .justify(j)
        .fill(Color::rgb(0.16, 0.18, 0.24))
        .radius(4.0)
        .show(ui, |ui| {
            for i in 0..3 {
                Frame::with_id((id, i))
                    .size((Sizing::Fixed(40.0), Sizing::Fixed(28.0)))
                    .fill(tile())
                    .radius(4.0)
                    .show(ui);
            }
        });
}
