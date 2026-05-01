use palantir::{Color, Element, Frame, HStack, Sizing, Styled, Ui, VStack};

fn fixed() -> Color {
    Color::rgb(0.85, 0.45, 0.30)
}
fn hug() -> Color {
    Color::rgb(0.45, 0.80, 0.55)
}
fn fill() -> Color {
    Color::rgb(0.30, 0.55, 0.85)
}

pub fn build(ui: &mut Ui) {
    VStack::new()
        .gap(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            // Row 1: Fixed sizes — exact pixels, ignores parent.
            row(ui, "fixed", |ui| {
                Frame::with_id("fx-50")
                    .size((Sizing::Fixed(50.0), Sizing::Fixed(40.0)))
                    .fill(fixed())
                    .radius(4.0)
                    .show(ui);
                Frame::with_id("fx-100")
                    .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                    .fill(fixed())
                    .radius(4.0)
                    .show(ui);
                Frame::with_id("fx-200")
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(40.0)))
                    .fill(fixed())
                    .radius(4.0)
                    .show(ui);
            });

            // Row 2: Hug — child's content drives size. Padded frames hug their
            // empty content box (effectively just padding).
            row(ui, "hug", |ui| {
                Frame::with_id("h-1")
                    .size((Sizing::Hug, Sizing::Fixed(40.0)))
                    .padding((20.0, 0.0, 20.0, 0.0))
                    .fill(hug())
                    .radius(4.0)
                    .show(ui);
                Frame::with_id("h-2")
                    .size((Sizing::Hug, Sizing::Fixed(40.0)))
                    .padding((40.0, 0.0, 40.0, 0.0))
                    .fill(hug())
                    .radius(4.0)
                    .show(ui);
            });

            // Row 3: Fill — split leftover by weight. 1 : 2 : 1.
            row(ui, "fill", |ui| {
                Frame::with_id("f-1")
                    .size((Sizing::Fill(1.0), Sizing::Fixed(40.0)))
                    .fill(fill())
                    .radius(4.0)
                    .show(ui);
                Frame::with_id("f-2")
                    .size((Sizing::Fill(2.0), Sizing::Fixed(40.0)))
                    .fill(fill())
                    .radius(4.0)
                    .show(ui);
                Frame::with_id("f-3")
                    .size((Sizing::Fill(1.0), Sizing::Fixed(40.0)))
                    .fill(fill())
                    .radius(4.0)
                    .show(ui);
            });
        });
}

fn row(ui: &mut Ui, id: &'static str, body: impl FnOnce(&mut Ui)) {
    HStack::with_id(id)
        .gap(8.0)
        .size((Sizing::FILL, Sizing::Hug))
        .show(ui, body);
}
