use palantir::{Color, Element, Frame, HStack, Sizing, Stroke, Styled, Ui, ZStack};

pub fn build(ui: &mut Ui) {
    HStack::new()
        .gap(16.0)
        .clip(false)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            // Left: clipped — child rect spills via negative margin, but the
            // scissor on the panel cuts it at the panel border.
            ZStack::with_id("clipped")
                .size((Sizing::FILL, Sizing::FILL))
                .clip(true)
                .fill(Color::rgb(0.16, 0.20, 0.28))
                .stroke(Stroke {
                    width: 1.5,
                    color: Color::rgb(0.30, 0.36, 0.46),
                })
                .radius(8.0)
                .show(ui, |ui| {
                    spiller(ui, "spilled-clipped");
                });

            // Right: same content, no clip — the spilling rect leaks past the panel.
            ZStack::with_id("unclipped")
                .size((Sizing::FILL, Sizing::FILL))
                .clip(false)
                .fill(Color::rgb(0.16, 0.20, 0.28))
                .stroke(Stroke {
                    width: 1.5,
                    color: Color::rgb(0.30, 0.36, 0.46),
                })
                .radius(8.0)
                .show(ui, |ui| {
                    spiller(ui, "spilled-unclipped");
                });
        });
}

fn spiller(ui: &mut Ui, id: &'static str) {
    Frame::with_id(id)
        .size((Sizing::Fixed(220.0), Sizing::Fixed(80.0)))
        .margin((-40.0, -30.0, 0.0, 0.0))
        .fill(Color::rgb(0.85, 0.45, 0.30))
        .radius(6.0)
        .show(ui);
}
