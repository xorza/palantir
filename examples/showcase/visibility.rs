use palantir::{Color, Configure, Frame, Panel, Sizing, Styled, Ui, Visibility};

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
    Panel::hstack_with_id(id)
        .gap(12.0)
        .size((Sizing::FILL, Sizing::Fixed(60.0)))
        .fill(Color::rgb(0.16, 0.18, 0.24))
        .padding(8.0)
        .radius(4.0)
        .show(ui, |ui| {
            tile(
                ui,
                (id, "a"),
                Color::rgb(0.30, 0.55, 0.85),
                Visibility::Visible,
            );
            tile(ui, (id, "mid"), Color::rgb(0.85, 0.45, 0.30), middle);
            tile(
                ui,
                (id, "c"),
                Color::rgb(0.45, 0.80, 0.55),
                Visibility::Visible,
            );
        });
}

fn tile<I: std::hash::Hash>(ui: &mut Ui, id: I, c: Color, vis: Visibility) {
    Frame::with_id(id)
        .size((Sizing::Fixed(80.0), Sizing::Fixed(40.0)))
        .visibility(vis)
        .fill(c)
        .radius(4.0)
        .show(ui);
}
