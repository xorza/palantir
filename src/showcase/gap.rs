use super::app_state::AppState;
use crate::showcase::swatch;
use palantir::{Configure, Frame, Panel, Sizing, Ui};

pub fn build(ui: &mut Ui<AppState>) {
    Panel::vstack()
        .auto_id()
        .gap(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            row(ui, "0", 0.0);
            row(ui, "8", 8.0);
            row(ui, "24", 24.0);
            row(ui, "48", 48.0);
        });
}

fn row<T>(ui: &mut Ui<T>, id: &'static str, gap: f32) {
    Panel::hstack()
        .id_salt(id)
        .size((Sizing::FILL, Sizing::Fixed(48.0)))
        .padding(8.0)
        .gap(gap)
        .show(ui, |ui| {
            for i in 0..5 {
                Frame::new()
                    .id_salt((id, i))
                    .size((Sizing::Fixed(40.0), Sizing::Fixed(32.0)))
                    .background(swatch::swatch_bg(swatch::A))
                    .show(ui);
            }
        });
}
