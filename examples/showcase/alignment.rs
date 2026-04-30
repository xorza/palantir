use palantir::{Align, Color, Element, Frame, HAlign, HStack, Sizing, Ui, VAlign, VStack};

fn parent_default() -> Color {
    Color::rgb(0.30, 0.55, 0.85)
}
fn self_override() -> Color {
    Color::rgb(0.85, 0.45, 0.30)
}

pub fn build(ui: &mut Ui) {
    VStack::new()
        .gap(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            // HStack with `child_align(VAlign::Center)`. All children inherit unless
            // they override. The orange one explicitly aligns to the bottom.
            HStack::new()
                .size((Sizing::FILL, Sizing::Fixed(120.0)))
                .gap(8.0)
                .padding(8.0)
                .child_align(Align::v(VAlign::Center))
                .fill(Color::rgb(0.16, 0.18, 0.24))
                .radius(6.0)
                .show(ui, |ui| {
                    chip(ui, "a", parent_default(), Align::default());
                    chip(ui, "b", parent_default(), Align::default());
                    chip(ui, "c-self-bot", self_override(), Align::v(VAlign::Bottom));
                    chip(ui, "d", parent_default(), Align::default());
                });

            // VStack with `child_align(HAlign::Right)` — children stack vertically,
            // packed to the right edge by default; "b-self-left" overrides.
            VStack::new()
                .size((Sizing::FILL, Sizing::FILL))
                .gap(8.0)
                .padding(8.0)
                .child_align(Align::h(HAlign::Right))
                .fill(Color::rgb(0.16, 0.18, 0.24))
                .radius(6.0)
                .show(ui, |ui| {
                    chip(ui, "a-vs", parent_default(), Align::default());
                    chip(ui, "b-self-left", self_override(), Align::h(HAlign::Left));
                    chip(ui, "c-vs", parent_default(), Align::default());
                });
        });
}

fn chip(ui: &mut Ui, id: &'static str, c: Color, align: Align) {
    Frame::with_id(id)
        .size((Sizing::Fixed(60.0), Sizing::Fixed(30.0)))
        .align(align)
        .fill(c)
        .radius(4.0)
        .show(ui);
}
