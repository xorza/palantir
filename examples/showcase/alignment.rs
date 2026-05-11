use crate::swatch;
use palantir::{Align, Color, Configure, Frame, HAlign, Panel, Sizing, Ui, VAlign};

pub fn build(ui: &mut Ui) {
    Panel::vstack()
        .auto_id()
        .gap(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            // HStack with `child_align(VAlign::Center)`. All children inherit unless
            // they override. The orange one explicitly aligns to the bottom.
            Panel::hstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::Fixed(120.0)))
                .gap(8.0)
                .padding(8.0)
                .child_align(Align::v(VAlign::Center))
                .show(ui, |ui| {
                    chip(ui, "a", swatch::A, Align::default());
                    chip(ui, "b", swatch::A, Align::default());
                    chip(ui, "c-self-bot", swatch::B, Align::v(VAlign::Bottom));
                    chip(ui, "d", swatch::A, Align::default());
                });

            // VStack with `child_align(HAlign::Right)` — children stack vertically,
            // packed to the right edge by default; "b-self-left" overrides.
            Panel::vstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .gap(8.0)
                .padding(8.0)
                .child_align(Align::h(HAlign::Right))
                .show(ui, |ui| {
                    chip(ui, "a-vs", swatch::A, Align::default());
                    chip(ui, "b-self-left", swatch::B, Align::h(HAlign::Left));
                    chip(ui, "c-vs", swatch::A, Align::default());
                });
        });
}

fn chip(ui: &mut Ui, id: &'static str, c: Color, align: Align) {
    Frame::new()
        .id_salt(id)
        .size((Sizing::Fixed(60.0), Sizing::Fixed(30.0)))
        .align(align)
        .background(swatch::swatch_bg(c))
        .show(ui);
}
