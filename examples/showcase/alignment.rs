use palantir::{
    Align, Background, Color, Configure, Corners, Frame, HAlign, Panel, Sizing, Ui, VAlign,
};

fn parent_default() -> Color {
    Color::rgb(0.30, 0.55, 0.85)
}
fn self_override() -> Color {
    Color::rgb(0.85, 0.45, 0.30)
}

pub fn build(ui: &mut Ui) {
    Panel::vstack()
        .gap(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            // HStack with `child_align(VAlign::Center)`. All children inherit unless
            // they override. The orange one explicitly aligns to the bottom.
            Panel::hstack()
                .size((Sizing::FILL, Sizing::Fixed(120.0)))
                .gap(8.0)
                .padding(8.0)
                .child_align(Align::v(VAlign::Center))
                .background(Background {
                    fill: Color::rgb(0.16, 0.18, 0.24),
                    radius: Corners::all(6.0),
                    ..Default::default()
                })
                .show(ui, |ui| {
                    chip(ui, "a", parent_default(), Align::default());
                    chip(ui, "b", parent_default(), Align::default());
                    chip(ui, "c-self-bot", self_override(), Align::v(VAlign::Bottom));
                    chip(ui, "d", parent_default(), Align::default());
                });

            // VStack with `child_align(HAlign::Right)` — children stack vertically,
            // packed to the right edge by default; "b-self-left" overrides.
            Panel::vstack()
                .size((Sizing::FILL, Sizing::FILL))
                .gap(8.0)
                .padding(8.0)
                .child_align(Align::h(HAlign::Right))
                .background(Background {
                    fill: Color::rgb(0.16, 0.18, 0.24),
                    radius: Corners::all(6.0),
                    ..Default::default()
                })
                .show(ui, |ui| {
                    chip(ui, "a-vs", parent_default(), Align::default());
                    chip(ui, "b-self-left", self_override(), Align::h(HAlign::Left));
                    chip(ui, "c-vs", parent_default(), Align::default());
                });
        });
}

fn chip(ui: &mut Ui, id: &'static str, c: Color, align: Align) {
    Frame::new()
        .with_id(id)
        .size((Sizing::Fixed(60.0), Sizing::Fixed(30.0)))
        .align(align)
        .background(Background {
            fill: c,
            radius: Corners::all(4.0),
            ..Default::default()
        })
        .show(ui);
}
