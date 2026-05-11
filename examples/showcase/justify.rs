use crate::swatch;
use palantir::{Configure, Frame, Justify, Panel, Sizing, Ui};

pub fn build(ui: &mut Ui) {
    Panel::vstack()
        .auto_id()
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
    Panel::hstack()
        .id_salt(id)
        .size((Sizing::FILL, Sizing::Fixed(40.0)))
        .padding((6.0, 4.0, 6.0, 4.0))
        .justify(j)
        .show(ui, |ui| {
            for i in 0..3 {
                Frame::new()
                    .id_salt((id, i))
                    .size((Sizing::Fixed(40.0), Sizing::Fixed(28.0)))
                    .background(swatch::swatch_bg(swatch::A))
                    .show(ui);
            }
        });
}
