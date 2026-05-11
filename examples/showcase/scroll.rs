use crate::swatch::{on_swatch_text, swatch_bg};
use palantir::{Color, Configure, Panel, Scroll, Sizing, Text, TextStyle, Ui};

pub fn build(ui: &mut Ui) {
    Panel::vstack()
        .auto_id()
        .gap(8.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Text::new(
                "Scroll — hover a card and pan with the wheel / two-finger scroll. \
                 Cards are vertical · horizontal · two-axis.",
            )
            .auto_id()
            .style(TextStyle::default().with_font_size(13.0))
            .show(ui);

            Panel::hstack()
                .auto_id()
                .gap(12.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    card(ui, "v-card", "vertical", |ui| {
                        Scroll::vertical()
                            .id_salt("rows-scroll")
                            .size((Sizing::FILL, Sizing::FILL))
                            .gap(4.0)
                            .show(ui, |ui| {
                                for i in 0..40 {
                                    row(ui, "v", i);
                                }
                            });
                    });

                    card(ui, "h-card", "horizontal", |ui| {
                        Scroll::horizontal()
                            .id_salt("cols-scroll")
                            .size((Sizing::FILL, Sizing::FILL))
                            .gap(4.0)
                            .show(ui, |ui| {
                                for i in 0..40 {
                                    col(ui, i);
                                }
                            });
                    });

                    card(ui, "xy-card", "two-axis", |ui| {
                        Scroll::both()
                            .id_salt("grid-scroll")
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui, |ui| {
                                grid(ui);
                            });
                    });
                });
        });
}

fn card(ui: &mut Ui, key: &'static str, label: &'static str, body: impl FnOnce(&mut Ui)) {
    Panel::vstack()
        .id_salt(key)
        .size((Sizing::FILL, Sizing::FILL))
        .padding(8.0)
        .gap(6.0)
        .show(ui, |ui| {
            Text::new(label)
                .id_salt((key, "title"))
                .style(TextStyle::default().with_font_size(12.0))
                .show(ui);
            body(ui);
        });
}

fn row(ui: &mut Ui, ns: &'static str, i: u32) {
    Panel::hstack()
        .id_salt((ns, "scroll-row", i))
        .size((Sizing::FILL, Sizing::Fixed(28.0)))
        .padding((10.0, 6.0))
        .background(swatch_bg(gradient_color(i)))
        .show(ui, |ui| {
            Text::new(format!("row {i:02}"))
                .id_salt((ns, "scroll-row-label", i))
                .style(on_swatch_text())
                .show(ui);
        });
}

fn col(ui: &mut Ui, i: u32) {
    Panel::vstack()
        .id_salt(("h", "scroll-col", i))
        .size((Sizing::Fixed(60.0), Sizing::FILL))
        .padding((6.0, 10.0))
        .background(swatch_bg(gradient_color(i)))
        .show(ui, |ui| {
            Text::new(format!("col {i:02}"))
                .id_salt(("h", "scroll-col-label", i))
                .style(on_swatch_text())
                .show(ui);
        });
}

fn grid(ui: &mut Ui) {
    // Single Hug-sized child holding a 12×16 colored grid via nested
    // VStack/HStack. Both-axes Scroll measures with INF on both axes,
    // so the inner stacks size to natural content and overflow the
    // viewport on both sides.
    Panel::vstack().id_salt("xy-grid").gap(4.0).show(ui, |ui| {
        for r in 0..16u32 {
            Panel::hstack()
                .id_salt(("xy-row", r))
                .gap(4.0)
                .show(ui, |ui| {
                    for c in 0..12u32 {
                        Panel::hstack()
                            .id_salt(("xy-cell", r, c))
                            .size((Sizing::Fixed(60.0), Sizing::Fixed(40.0)))
                            .padding((6.0, 4.0))
                            .background(swatch_bg(gradient_color(r * 12 + c)))
                            .show(ui, |ui| {
                                Text::new(format!("{r},{c}"))
                                    .id_salt(("xy-cell-label", r, c))
                                    .style(on_swatch_text().with_font_size(11.0))
                                    .show(ui);
                            });
                    }
                });
        }
    });
}

/// Gradient across the scrollable items so panning shows visible progress.
/// The colors aren't theme — they ARE the demo content.
fn gradient_color(i: u32) -> Color {
    let t = (i % 40) as f32 / 40.0;
    Color::rgb(
        0.30 + 0.50 * t,
        0.55 - 0.20 * (t - 0.5).abs(),
        0.85 - 0.55 * t,
    )
}
