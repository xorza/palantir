use palantir::{Color, Element, Frame, HStack, Sizing, Stroke, Styled, Ui, VStack};

fn fill_color() -> Color {
    Color::rgba(0.30, 0.55, 0.85, 0.85)
}

pub fn build(ui: &mut Ui) {
    VStack::new()
        .gap(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            // Padding: parent reserves space inside its border before children.
            cell(ui, "padding", |ui| {
                HStack::with_id("p-row")
                    .size((Sizing::FILL, Sizing::Fixed(60.0)))
                    .padding(20.0)
                    .gap(8.0)
                    .fill(Color::rgb(0.20, 0.24, 0.32))
                    .radius(4.0)
                    .show(ui, |ui| {
                        for i in 0..3 {
                            Frame::with_id(("p", i))
                                .size((Sizing::Fixed(40.0), Sizing::FILL))
                                .fill(fill_color())
                                .radius(4.0)
                                .show(ui);
                        }
                    });
            });

            // Margin: child shrinks its slot, the surrounding gap is the margin.
            cell(ui, "margin", |ui| {
                HStack::with_id("m-row")
                    .size((Sizing::FILL, Sizing::Fixed(60.0)))
                    .gap(8.0)
                    .fill(Color::rgb(0.20, 0.24, 0.32))
                    .radius(4.0)
                    .show(ui, |ui| {
                        Frame::with_id("m1")
                            .size((Sizing::Fixed(60.0), Sizing::Fixed(40.0)))
                            .margin(8.0)
                            .fill(fill_color())
                            .radius(4.0)
                            .show(ui);
                        Frame::with_id("m2")
                            .size((Sizing::Fixed(60.0), Sizing::Fixed(40.0)))
                            .margin((16.0, 16.0, 0.0, 0.0))
                            .fill(fill_color())
                            .radius(4.0)
                            .show(ui);
                    });
            });

            // Negative margin: rendered rect spills past its slot. The orange
            // box is anchored after the blue one, but its left margin pulls it
            // backwards 30px so the two overlap.
            cell(ui, "negative margin", |ui| {
                HStack::with_id("neg-row")
                    .size((Sizing::FILL, Sizing::Fixed(60.0)))
                    .padding(8.0)
                    .fill(Color::rgb(0.20, 0.24, 0.32))
                    .radius(4.0)
                    .show(ui, |ui| {
                        Frame::with_id("neg-a")
                            .size((Sizing::Fixed(80.0), Sizing::Fixed(40.0)))
                            .fill(fill_color())
                            .radius(4.0)
                            .show(ui);
                        Frame::with_id("neg-b")
                            .size((Sizing::Fixed(80.0), Sizing::Fixed(40.0)))
                            .margin((-30.0, 0.0, 0.0, 0.0))
                            .fill(Color::rgba(0.85, 0.45, 0.30, 0.85))
                            .stroke(Stroke {
                                width: 1.0,
                                color: Color::rgb(0.85, 0.45, 0.30),
                            })
                            .radius(4.0)
                            .show(ui);
                    });
            });
        });
}

fn cell(ui: &mut Ui, id: &'static str, body: impl FnOnce(&mut Ui)) {
    VStack::with_id(id)
        .gap(4.0)
        .size((Sizing::FILL, Sizing::Hug))
        .show(ui, body);
}
