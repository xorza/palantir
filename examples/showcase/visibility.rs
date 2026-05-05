use crate::swatch;
use palantir::{Background, Color, Configure, Corners, Frame, Panel, Sizing, Ui, Visibility};

pub fn build(ui: &mut Ui) {
    Panel::vstack()
        .gap(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            // Three rows. Each row has [a, b, c] of equal width with gap; the
            // middle one's visibility is the only difference between rows.
            row(ui, "visible", Visibility::Visible);
            row(ui, "hidden", Visibility::Hidden);
            row(ui, "collapsed", Visibility::Collapsed);
        });
}

fn row(ui: &mut Ui, id: &'static str, middle: Visibility) {
    Panel::hstack()
        .with_id(id)
        .gap(12.0)
        .size((Sizing::FILL, Sizing::Fixed(60.0)))
        .padding(8.0)
        .show(ui, |ui| {
            tile(ui, (id, "a"), swatch::A, Visibility::Visible);
            tile(ui, (id, "mid"), swatch::B, middle);
            tile(ui, (id, "c"), swatch::C, Visibility::Visible);
        });
}

fn tile<I: std::hash::Hash>(ui: &mut Ui, id: I, c: Color, vis: Visibility) {
    Frame::new()
        .with_id(id)
        .size((Sizing::Fixed(80.0), Sizing::Fixed(40.0)))
        .visibility(vis)
        .background(Background {
            fill: c,
            radius: Corners::all(4.0),
            ..Default::default()
        })
        .show(ui);
}
