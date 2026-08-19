//! Scroll viewports hosted inside splitter panes — both widgets doing
//! real work in one layout. The outer horizontal splitter holds a
//! vertical scroll list; its right half splits again vertically into a
//! horizontal scroll strip and a two-axis scroll grid.

use crate::support;
use crate::support::{note_style, on_swatch_style, swatch_bg, well_bg};
use palantir::{
    Color, Configure, Panel, Scroll, Sizing, SplitHalf, Splitter, Text, Ui, WidgetId, fmt,
};

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

pub(crate) fn build(ui: &mut Ui) {
    let state_id = WidgetId::from_hash("showcase::scroll::state");
    ui.with_state::<State, _>(state_id, split_panes);
}

fn split_panes(ui: &mut Ui, s: &mut State) {
    Splitter::horizontal(&mut s.h)
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
                Splitter::vertical(&mut s.v)
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

    let readout = fmt!(ui, "split fractions: h = {:.2}   v = {:.2}", s.h, s.v);
    Text::new(readout)
        .id_salt("readout")
        .style(&note_style())
        .show(ui);
}

#[track_caller]
fn pane(ui: &mut Ui, label: &'static str, body: impl FnOnce(&mut Ui)) {
    Panel::vstack()
        .auto_id()
        .size((Sizing::FILL, Sizing::FILL))
        .padding(8.0)
        .gap(6.0)
        .background(well_bg())
        .show(ui, |ui| {
            Text::new(label).style(&support::caption_style()).show(ui);
            body(ui);
        });
}

fn row(ui: &mut Ui, i: u32) {
    Panel::hstack()
        .id_salt(("scroll-row", i))
        .size((Sizing::FILL, Sizing::fixed(28.0)))
        .padding((10.0, 6.0))
        .background(swatch_bg(ramp(i)))
        .show(ui, |ui| {
            let label = fmt!(ui, "row {i:02}");
            Text::new(label)
                .id_salt(("scroll-row-label", i))
                .style(&on_swatch_style())
                .show(ui);
        });
}

fn col(ui: &mut Ui, i: u32) {
    Panel::vstack()
        .id_salt(("scroll-col", i))
        .size((Sizing::fixed(60.0), Sizing::FILL))
        .padding((6.0, 10.0))
        .background(swatch_bg(ramp(i)))
        .show(ui, |ui| {
            let label = fmt!(ui, "col {i:02}");
            Text::new(label)
                .id_salt(("scroll-col-label", i))
                .style(&on_swatch_style())
                .show(ui);
        });
}

fn grid(ui: &mut Ui) {
    // Single Hug-sized child holding a 12×16 colored grid via nested
    // VStack/HStack. Both-axes Scroll measures with INF on both axes, so
    // the inner stacks size to natural content and overflow the viewport
    // on both sides.
    Panel::vstack().id_salt("xy-grid").gap(4.0).show(ui, |ui| {
        for r in 0..16u32 {
            Panel::hstack()
                .id_salt(("xy-row", r))
                .gap(4.0)
                .show(ui, |ui| {
                    for c in 0..12u32 {
                        Panel::hstack()
                            .id_salt(("xy-cell", r, c))
                            .size((Sizing::fixed(60.0), Sizing::fixed(40.0)))
                            .padding((6.0, 4.0))
                            .background(swatch_bg(ramp(r * 12 + c)))
                            .show(ui, |ui| {
                                let label = fmt!(ui, "{r},{c}");
                                Text::new(label)
                                    .id_salt(("xy-cell-label", r, c))
                                    .style(&on_swatch_style().with_font_size(11.0))
                                    .show(ui);
                            });
                    }
                });
        }
    });
}

/// Teal → purple → orange sweep across the scrollable items, so panning
/// shows visible progress. These colors aren't theme — they ARE the demo
/// content, and they stay in the swatch palette's hues.
fn ramp(i: u32) -> Color {
    let t = (i % 40) as f32 / 40.0;
    let (from, to, u) = if t < 0.5 {
        (support::A, support::D, t * 2.0)
    } else {
        (support::D, support::B, (t - 0.5) * 2.0)
    };
    Color::linear_rgb(
        from.r + (to.r - from.r) * u,
        from.g + (to.g - from.g) * u,
        from.b + (to.b - from.b) * u,
    )
}
