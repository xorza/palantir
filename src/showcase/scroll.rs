//! Scroll viewports hosted inside splitter panes — both widgets doing
//! real work in one layout. The outer horizontal splitter holds a
//! vertical scroll list; its right half splits again vertically into a
//! horizontal scroll strip and a two-axis scroll grid. Drag the bars
//! (double-click recenters), hover a pane and wheel / two-finger pan.

use crate::showcase::support;
use crate::showcase::support::{caption_style, on_swatch_text, panel_bg, swatch_bg};
use aperture::{Color, Configure, Panel, Scroll, Sizing, SplitHalf, Splitter, Text, Ui, WidgetId};

#[derive(Debug)]
struct State {
    h: f32,
    v: f32,
}

impl Default for State {
    fn default() -> Self {
        Self { h: 0.45, v: 0.5 }
    }
}

pub fn build(ui: &mut Ui) {
    let state_id = WidgetId::from_hash("showcase::scroll::state");
    let s = ui.state_mut::<State>(state_id);
    let mut h = s.h;
    let mut v = s.v;

    support::page(ui, |ui| {
        support::header(
            ui,
            "Scroll viewports inside splitter panes — drag the bars (double-click \
             recenters); hover a pane and wheel / two-finger scroll.",
        );

        Splitter::horizontal(&mut h)
            .id_salt("split-h")
            .min_pane(120.0)
            .show(ui, |ui, half| match half {
                SplitHalf::First => pane(ui, "vertical", |ui| {
                    Scroll::vertical()
                        .id_salt("rows-scroll")
                        .size((Sizing::FILL, Sizing::FILL))
                        .gap(4.0)
                        .show(ui, |ui| {
                            for i in 0..40 {
                                row(ui, i);
                            }
                        });
                }),
                SplitHalf::Second => {
                    Splitter::vertical(&mut v)
                        .id_salt("split-v")
                        .min_pane(100.0)
                        .show(ui, |ui, half| match half {
                            SplitHalf::First => pane(ui, "horizontal", |ui| {
                                Scroll::horizontal()
                                    .id_salt("cols-scroll")
                                    .size((Sizing::FILL, Sizing::FILL))
                                    .gap(4.0)
                                    .show(ui, |ui| {
                                        for i in 0..40 {
                                            col(ui, i);
                                        }
                                    });
                            }),
                            SplitHalf::Second => pane(ui, "two-axis", |ui| {
                                Scroll::both()
                                    .id_salt("grid-scroll")
                                    .size((Sizing::FILL, Sizing::FILL))
                                    .show(ui, grid);
                            }),
                        });
                }
            });

        let readout = ui.fmt(format_args!("split fractions: h = {h:.2}   v = {v:.2}"));
        Text::new(readout)
            .id_salt("readout")
            .style(caption_style())
            .show(ui);
    });

    let s = ui.state_mut::<State>(state_id);
    s.h = h;
    s.v = v;
}

fn pane(ui: &mut Ui, label: &'static str, body: impl FnOnce(&mut Ui)) {
    Panel::vstack()
        .id_salt((label, "pane"))
        .size((Sizing::FILL, Sizing::FILL))
        .padding(8.0)
        .gap(6.0)
        .background(panel_bg())
        .show(ui, |ui| {
            Text::new(label)
                .id_salt((label, "title"))
                .style(caption_style())
                .show(ui);
            body(ui);
        });
}

fn row(ui: &mut Ui, i: u32) {
    Panel::hstack()
        .id_salt(("scroll-row", i))
        .size((Sizing::FILL, Sizing::Fixed(28.0)))
        .padding((10.0, 6.0))
        .background(swatch_bg(gradient_color(i)))
        .show(ui, |ui| {
            Text::new(format!("row {i:02}"))
                .id_salt(("scroll-row-label", i))
                .style(on_swatch_text())
                .show(ui);
        });
}

fn col(ui: &mut Ui, i: u32) {
    Panel::vstack()
        .id_salt(("scroll-col", i))
        .size((Sizing::Fixed(60.0), Sizing::FILL))
        .padding((6.0, 10.0))
        .background(swatch_bg(gradient_color(i)))
        .show(ui, |ui| {
            Text::new(format!("col {i:02}"))
                .id_salt(("scroll-col-label", i))
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
