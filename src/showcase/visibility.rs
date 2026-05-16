use super::app_state::AppState;
use crate::showcase::swatch;
use palantir::{Color, Configure, Frame, Panel, Sizing, Ui, Visibility};

pub fn build(ui: &mut Ui<AppState>) {
    Panel::vstack()
        .auto_id()
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

fn row<T>(ui: &mut Ui<T>, id: &'static str, middle: Visibility) {
    Panel::hstack()
        .id_salt(id)
        .gap(12.0)
        .size((Sizing::FILL, Sizing::Fixed(60.0)))
        .padding(8.0)
        .show(ui, |ui| {
            tile(ui, (id, "a"), swatch::A, Visibility::Visible);
            tile(ui, (id, "mid"), swatch::B, middle);
            tile(ui, (id, "c"), swatch::C, Visibility::Visible);
        });
}

fn tile<T, I: std::hash::Hash>(ui: &mut Ui<T>, id: I, c: Color, vis: Visibility) {
    Frame::new()
        .id_salt(id)
        .size((Sizing::Fixed(80.0), Sizing::Fixed(40.0)))
        .visibility(vis)
        .background(swatch::swatch_bg(c))
        .show(ui);
}
